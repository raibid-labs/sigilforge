//! Reference resolution for `auth://` URIs and vals-style references.
//!
//! This module provides:
//! - [`ResolvedValue`] - The result of resolving a reference
//! - [`ReferenceResolver`] - Trait for resolving credential references
//! - Support for `auth://service/account/credential` URIs
//! - Optional support for `vals:ref+...` external references

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::CredentialRef;
use crate::store::Secret;

/// Error type for reference resolution.
#[derive(Debug, Error)]
pub enum ResolveError {
    /// The reference format is invalid.
    #[error("invalid reference format: {message}")]
    InvalidFormat { message: String },

    /// The referenced credential was not found.
    #[error("credential not found: {reference}")]
    NotFound { reference: String },

    /// The reference scheme is not supported.
    #[error("unsupported scheme: {scheme}")]
    UnsupportedScheme { scheme: String },

    /// Error from the underlying store.
    #[error("storage error: {0}")]
    StorageError(#[from] crate::store::StoreError),

    /// Error from token operations.
    #[error("token error: {0}")]
    TokenError(#[from] crate::token::TokenError),

    /// Error calling external resolver (e.g., vals).
    #[error("external resolver error: {message}")]
    ExternalError { message: String },
}

/// The result of resolving a reference.
#[derive(Debug, Clone)]
pub enum ResolvedValue {
    /// A secret value (token, API key, etc.).
    Secret(Secret),

    /// A non-secret string value.
    Plain(String),

    /// Multiple resolved values (for batch resolution).
    Multiple(Vec<(String, ResolvedValue)>),
}

impl ResolvedValue {
    /// Get the value as a string, exposing secrets.
    ///
    /// Use sparingly; prefer keeping values wrapped.
    pub fn expose(&self) -> String {
        match self {
            Self::Secret(s) => s.expose().to_string(),
            Self::Plain(s) => s.clone(),
            Self::Multiple(_) => "[multiple values]".to_string(),
        }
    }

    /// Check if this is a secret value.
    pub fn is_secret(&self) -> bool {
        matches!(self, Self::Secret(_))
    }
}

/// Trait for resolving credential references.
///
/// This is the primary interface for consumers to obtain credentials.
/// It handles:
/// - `auth://` URIs for Sigilforge-managed credentials
/// - Token refresh when needed
/// - Optional external reference resolution (vals-style)
///
/// # Example
///
/// ```rust,ignore
/// use sigilforge_core::ReferenceResolver;
///
/// async fn get_spotify_token(resolver: &impl ReferenceResolver) -> String {
///     let value = resolver.resolve("auth://spotify/personal/token").await.unwrap();
///     value.expose()
/// }
/// ```
#[async_trait]
pub trait ReferenceResolver: Send + Sync {
    /// Resolve a reference string to its value.
    ///
    /// Supported formats:
    /// - `auth://service/account/credential_type` - Sigilforge credential
    /// - `vals:ref+vault://path` - External vals reference (if enabled)
    async fn resolve(&self, reference: &str) -> Result<ResolvedValue, ResolveError>;

    /// Resolve a structured credential reference.
    ///
    /// This is more type-safe than string-based resolution.
    async fn resolve_ref(&self, cred_ref: &CredentialRef) -> Result<ResolvedValue, ResolveError>;

    /// Resolve multiple references in batch.
    ///
    /// More efficient than resolving one at a time when many references
    /// need to be resolved together.
    async fn resolve_batch(
        &self,
        references: &[String],
    ) -> Result<Vec<(String, Result<ResolvedValue, ResolveError>)>, ResolveError> {
        let mut results = Vec::with_capacity(references.len());
        for reference in references {
            let result = self.resolve(reference).await;
            results.push((reference.clone(), result));
        }
        Ok(results)
    }

    /// Check if a reference scheme is supported.
    fn supports_scheme(&self, scheme: &str) -> bool;
}

/// Configuration for reference resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverConfig {
    /// Enable `auth://` URI resolution (default: true).
    pub enable_auth_scheme: bool,

    /// Enable vals-style reference resolution.
    pub enable_vals: bool,

    /// Path to vals binary (if not in PATH).
    pub vals_path: Option<String>,

    /// Cache resolved values for this duration (seconds).
    pub cache_ttl_secs: Option<u64>,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            enable_auth_scheme: true,
            enable_vals: false,
            vals_path: None,
            cache_ttl_secs: None,
        }
    }
}

/// Default implementation of ReferenceResolver.
///
/// This resolver handles:
/// - `auth://` URIs for Sigilforge-managed credentials
/// - Token refresh when fetching access tokens
///
/// # Example
///
/// ```rust,ignore
/// use sigilforge_core::{
///     DefaultReferenceResolver,
///     token_manager::DefaultTokenManager,
///     store::MemoryStore,
///     provider::ProviderRegistry,
/// };
///
/// let store = MemoryStore::new();
/// let providers = ProviderRegistry::with_defaults();
/// let token_manager = DefaultTokenManager::new(store.clone(), providers);
/// let resolver = DefaultReferenceResolver::new(store, token_manager);
///
/// let value = resolver.resolve("auth://spotify/personal/token").await?;
/// ```
#[cfg(feature = "oauth")]
pub struct DefaultReferenceResolver<S, T>
where
    S: crate::store::SecretStore,
    T: crate::token::TokenManager,
{
    store: S,
    token_manager: T,
    config: ResolverConfig,
}

#[cfg(feature = "oauth")]
impl<S, T> DefaultReferenceResolver<S, T>
where
    S: crate::store::SecretStore,
    T: crate::token::TokenManager,
{
    /// Create a new resolver with the given store and token manager.
    pub fn new(store: S, token_manager: T) -> Self {
        Self {
            store,
            token_manager,
            config: ResolverConfig::default(),
        }
    }

    /// Create a new resolver with custom configuration.
    pub fn with_config(store: S, token_manager: T, config: ResolverConfig) -> Self {
        Self {
            store,
            token_manager,
            config,
        }
    }
}

#[cfg(feature = "oauth")]
#[async_trait]
impl<S, T> ReferenceResolver for DefaultReferenceResolver<S, T>
where
    S: crate::store::SecretStore + Send + Sync + 'static,
    T: crate::token::TokenManager + Send + Sync + 'static,
{
    async fn resolve(&self, reference: &str) -> Result<ResolvedValue, ResolveError> {
        // Check for auth:// scheme
        if reference.starts_with("auth://") {
            if !self.config.enable_auth_scheme {
                return Err(ResolveError::UnsupportedScheme {
                    scheme: "auth".to_string(),
                });
            }

            let cred_ref = CredentialRef::from_auth_uri(reference).map_err(|e| {
                ResolveError::InvalidFormat {
                    message: e.to_string(),
                }
            })?;

            return self.resolve_ref(&cred_ref).await;
        }

        // Check for vals:ref+ scheme
        if reference.starts_with("vals:ref+") {
            if !self.config.enable_vals {
                return Err(ResolveError::UnsupportedScheme {
                    scheme: "vals".to_string(),
                });
            }

            // TODO: Shell out to vals for external resolution
            return Err(ResolveError::ExternalError {
                message: "vals resolution not yet implemented".to_string(),
            });
        }

        Err(ResolveError::InvalidFormat {
            message: format!("unknown reference format: {}", reference),
        })
    }

    async fn resolve_ref(&self, cred_ref: &CredentialRef) -> Result<ResolvedValue, ResolveError> {
        use crate::model::CredentialType;

        match &cred_ref.credential_type {
            // For access tokens, use the token manager (handles refresh)
            CredentialType::AccessToken => {
                let token = self
                    .token_manager
                    .ensure_access_token(&cred_ref.service, &cred_ref.account)
                    .await?;
                Ok(ResolvedValue::Secret(Secret::new(
                    token.access_token.expose(),
                )))
            }

            // For other credential types, fetch directly from store
            _ => {
                let key = cred_ref.to_key();
                match self.store.get(&key).await? {
                    Some(secret) => Ok(ResolvedValue::Secret(secret)),
                    None => Err(ResolveError::NotFound {
                        reference: cred_ref.to_auth_uri(),
                    }),
                }
            }
        }
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        match scheme {
            "auth" => self.config.enable_auth_scheme,
            "vals" => self.config.enable_vals,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolved_value_expose() {
        let secret = ResolvedValue::Secret(Secret::new("secret-token"));
        assert_eq!(secret.expose(), "secret-token");

        let plain = ResolvedValue::Plain("plain-value".to_string());
        assert_eq!(plain.expose(), "plain-value");
    }

    #[test]
    fn test_resolved_value_is_secret() {
        let secret = ResolvedValue::Secret(Secret::new("token"));
        assert!(secret.is_secret());

        let plain = ResolvedValue::Plain("value".to_string());
        assert!(!plain.is_secret());
    }
}
