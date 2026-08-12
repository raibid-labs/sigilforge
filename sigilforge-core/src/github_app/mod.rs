//! GitHub App authentication.
//!
//! GitHub Apps authenticate differently from the user-facing OAuth flows in
//! [`crate::oauth`]. There is no browser, no authorization code, and no refresh
//! token. Instead an App proves its identity by signing a short-lived **RS256
//! JWT** with its private key, then exchanges that JWT for an **installation
//! access token** scoped to the repositories the App is installed on.
//!
//! ```text
//!   app id + installation id + PEM private key   (stored once, in the keyring)
//!                     |
//!                     v
//!   RS256 JWT  iss = app id, iat = now - 60s, exp <= now + 10min
//!                     |
//!                     v
//!   POST /app/installations/{installation_id}/access_tokens
//!                     |
//!                     v
//!   ghs_... installation token, expires_at ~1 hour out  (cached, re-minted)
//! ```
//!
//! ## Why this exists
//!
//! Argo CD and CI need read access to private repositories in an organization
//! that disallows deploy keys. The alternative - a broad personal access token
//! copied into a cluster `Secret` - is exactly the credential sprawl Sigilforge
//! exists to prevent. A GitHub App is org-owned, scoped to named repositories,
//! needs no browser, and mints tokens that expire in an hour.
//!
//! ## Module layout
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`jwt`] | Claim construction and RS256 signing |
//! | [`token`] | Installation-token minting, caching, and expiry |
//! | [`argocd`] | Rendering an Argo CD repository `Secret` manifest |
//!
//! ## Storage keys
//!
//! Registration material lives in the configured [`SecretStore`](crate::store::SecretStore)
//! under the fixed service id [`GITHUB_APP_SERVICE`]:
//!
//! ```text
//! sigilforge/github-app/{account}/app_id
//! sigilforge/github-app/{account}/installation_id
//! sigilforge/github-app/{account}/private_key         <- the PEM, never on disk in plaintext
//! sigilforge/github-app/{account}/installation_token  <- cache
//! sigilforge/github-app/{account}/token_expiry        <- cache, RFC 3339
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! # #[cfg(feature = "github-app")]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use sigilforge_core::{
//!     AccountId, MemoryStore,
//!     github_app::{GitHubAppCredential, GitHubAppTokenManager},
//! };
//!
//! let manager = GitHubAppTokenManager::new(MemoryStore::new());
//! let account = AccountId::new("raibid-labs");
//!
//! // One-time registration (the PEM comes from the GitHub App settings page).
//! let pem = std::fs::read_to_string("raibid-labs.private-key.pem")?;
//! let credential = GitHubAppCredential::new(1234567, 89012345, pem)?;
//! manager.register(&account, &credential).await?;
//!
//! // Thereafter: a cached token, re-minted only when it is close to expiring.
//! let token = manager.ensure_installation_token(&account).await?;
//! println!("token expires at {:?}", token.expires_at);
//! # Ok(())
//! # }
//! ```

pub mod argocd;
pub mod jwt;
pub mod token;

use std::fmt;

use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::sha2::Sha256;
use thiserror::Error;

use crate::model::ServiceId;
use crate::store::{Secret, StoreError};
use crate::token::TokenError;

pub use argocd::{ArgoCdRepositorySecret, default_secret_name};
pub use jwt::{
    AppJwtClaims, CLOCK_SKEW_SECS, DEFAULT_JWT_TTL_SECS, MAX_JWT_TTL_SECS, sign_app_jwt,
};
pub use token::{
    DEFAULT_REFRESH_BUFFER_SECS, GITHUB_API_BASE_URL, GitHubAppTokenManager,
    InstallationTokenSource,
};

/// The fixed service identifier under which GitHub App credentials are stored.
///
/// GitHub App credentials are addressed as `auth://github-app/{account}/installation_token`,
/// where `{account}` names the installation (conventionally the org or user the
/// App is installed on).
pub const GITHUB_APP_SERVICE: &str = "github-app";

/// Build the [`ServiceId`] for GitHub App credentials.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use sigilforge_core::github_app::github_app_service_id;
///
/// assert_eq!(github_app_service_id().as_str(), "github-app");
/// # }
/// ```
pub fn github_app_service_id() -> ServiceId {
    ServiceId::new(GITHUB_APP_SERVICE)
}

/// Error type for GitHub App authentication.
#[derive(Debug, Error)]
pub enum GitHubAppError {
    /// The PEM private key could not be parsed as a PKCS#1 or PKCS#8 RSA key.
    ///
    /// The message never contains key material - only the structural failure.
    #[error("invalid GitHub App private key: {message}")]
    InvalidPrivateKey { message: String },

    /// Signing the JWT failed.
    #[error("failed to sign GitHub App JWT: {message}")]
    JwtSigning { message: String },

    /// No GitHub App is registered for the requested account.
    #[error("no GitHub App registered for account '{account}'")]
    NotRegistered { account: String },

    /// A stored registration field was present but malformed.
    #[error("malformed GitHub App registration for '{account}': {message}")]
    MalformedRegistration { account: String, message: String },

    /// The HTTP request to the GitHub API could not be completed.
    #[error("GitHub API request failed: {message}")]
    Http { message: String },

    /// The GitHub API returned a non-success status.
    #[error("GitHub API returned {status}: {message}")]
    GitHubApi { status: u16, message: String },

    /// The GitHub API response could not be parsed.
    #[error("unexpected GitHub API response: {message}")]
    InvalidResponse { message: String },

    /// A repository URL could not be interpreted.
    #[error("invalid repository URL: {url}")]
    InvalidRepoUrl { url: String },

    /// Error from the underlying secret store.
    #[error("storage error: {0}")]
    Storage(#[from] StoreError),
}

impl From<GitHubAppError> for TokenError {
    fn from(err: GitHubAppError) -> Self {
        match err {
            GitHubAppError::Storage(e) => TokenError::StorageError(e),
            GitHubAppError::NotRegistered { account } => TokenError::NotFound {
                service: GITHUB_APP_SERVICE.to_string(),
                account,
            },
            GitHubAppError::Http { message } => TokenError::NetworkError { message },
            other => TokenError::RefreshFailed {
                message: other.to_string(),
            },
        }
    }
}

/// A registered GitHub App installation: app id, installation id, and private key.
///
/// The private key is held in a [`Secret`] and is never rendered by [`fmt::Debug`],
/// [`fmt::Display`], or serialization. Construct with [`GitHubAppCredential::new`],
/// which validates that the PEM actually parses - registering an unusable key and
/// only discovering it at mint time would be a poor trade.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use sigilforge_core::github_app::GitHubAppCredential;
///
/// let pem = sigilforge_core::github_app::test_support::TEST_PRIVATE_KEY_PEM;
/// let credential = GitHubAppCredential::new(1234567, 89012345, pem).unwrap();
///
/// assert_eq!(credential.app_id(), 1234567);
/// assert_eq!(credential.installation_id(), 89012345);
///
/// // The key never leaks through Debug.
/// let rendered = format!("{:?}", credential);
/// assert!(!rendered.contains("PRIVATE KEY"));
/// assert!(rendered.contains("REDACTED"));
/// # }
/// ```
#[derive(Clone)]
pub struct GitHubAppCredential {
    app_id: u64,
    installation_id: u64,
    private_key_pem: Secret,
}

impl GitHubAppCredential {
    /// Create a credential, validating that the PEM parses as an RSA private key.
    ///
    /// Accepts both the PKCS#1 (`BEGIN RSA PRIVATE KEY`) form that GitHub hands
    /// out and the PKCS#8 (`BEGIN PRIVATE KEY`) form that `openssl pkcs8` produces.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubAppError::InvalidPrivateKey`] if the PEM is not a readable
    /// RSA private key.
    pub fn new(
        app_id: u64,
        installation_id: u64,
        private_key_pem: impl Into<String>,
    ) -> Result<Self, GitHubAppError> {
        let pem = private_key_pem.into();
        // Validate eagerly; a key that cannot sign is not a credential.
        parse_private_key(&pem)?;

        Ok(Self {
            app_id,
            installation_id,
            private_key_pem: Secret::new(pem),
        })
    }

    /// The numeric GitHub App ID (the `App ID` field on the App's settings page).
    pub fn app_id(&self) -> u64 {
        self.app_id
    }

    /// The numeric installation ID (the trailing path segment of the
    /// `.../settings/installations/{id}` URL).
    pub fn installation_id(&self) -> u64 {
        self.installation_id
    }

    /// The PEM private key, still wrapped.
    ///
    /// Callers that must hand the PEM to something else (for example an Argo CD
    /// `Secret` manifest) go through [`Secret::expose`] deliberately.
    pub fn private_key_pem(&self) -> &Secret {
        &self.private_key_pem
    }

    /// Build an RS256 signing key from the stored PEM.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubAppError::InvalidPrivateKey`] if the PEM cannot be parsed.
    pub fn signing_key(&self) -> Result<SigningKey<Sha256>, GitHubAppError> {
        let key = parse_private_key(self.private_key_pem.expose())?;
        Ok(SigningKey::<Sha256>::new(key))
    }
}

/// Hand-written so the private key can never reach a log line, even if the
/// struct gains fields later.
impl fmt::Debug for GitHubAppCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHubAppCredential")
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .field("private_key_pem", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for GitHubAppCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GitHub App {} installation {}",
            self.app_id, self.installation_id
        )
    }
}

/// Parse a PEM private key, trying PKCS#1 then PKCS#8.
///
/// Error text is deliberately generic: parser errors can echo input fragments,
/// and this input is key material.
fn parse_private_key(pem: &str) -> Result<RsaPrivateKey, GitHubAppError> {
    let trimmed = pem.trim();

    if trimmed.is_empty() {
        return Err(GitHubAppError::InvalidPrivateKey {
            message: "private key is empty".to_string(),
        });
    }

    if let Ok(key) = RsaPrivateKey::from_pkcs1_pem(trimmed) {
        return Ok(key);
    }

    RsaPrivateKey::from_pkcs8_pem(trimmed).map_err(|_| GitHubAppError::InvalidPrivateKey {
        message: "expected a PKCS#1 ('BEGIN RSA PRIVATE KEY') or PKCS#8 \
                  ('BEGIN PRIVATE KEY') PEM-encoded RSA private key"
            .to_string(),
    })
}

/// Fixtures for exercising GitHub App code without a real App.
///
/// The key below is a throwaway 2048-bit RSA key generated for this repository's
/// tests and doc examples. It authenticates nothing and guards nothing. Real keys
/// belong in the OS keyring, never in source.
pub mod test_support {
    /// A test-only 2048-bit RSA private key in PKCS#1 PEM form.
    pub const TEST_PRIVATE_KEY_PEM: &str = include_str!("test_key.pem");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_accepts_pkcs1_pem() {
        let credential =
            GitHubAppCredential::new(1, 2, test_support::TEST_PRIVATE_KEY_PEM).unwrap();
        assert_eq!(credential.app_id(), 1);
        assert_eq!(credential.installation_id(), 2);
    }

    #[test]
    fn test_credential_rejects_garbage_pem() {
        let err = GitHubAppCredential::new(1, 2, "not a key at all").unwrap_err();
        assert!(matches!(err, GitHubAppError::InvalidPrivateKey { .. }));
    }

    #[test]
    fn test_credential_rejects_empty_pem() {
        let err = GitHubAppCredential::new(1, 2, "   \n  ").unwrap_err();
        assert!(matches!(err, GitHubAppError::InvalidPrivateKey { .. }));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn test_credential_debug_redacts_private_key() {
        let credential =
            GitHubAppCredential::new(1234567, 89012345, test_support::TEST_PRIVATE_KEY_PEM)
                .unwrap();
        let rendered = format!("{:?}", credential);

        assert!(rendered.contains("1234567"));
        assert!(rendered.contains("89012345"));
        assert!(rendered.contains("REDACTED"));
        assert!(!rendered.contains("PRIVATE KEY"));
        assert!(!rendered.contains("MII"));
    }

    #[test]
    fn test_credential_display_redacts_private_key() {
        let credential =
            GitHubAppCredential::new(42, 99, test_support::TEST_PRIVATE_KEY_PEM).unwrap();
        let rendered = format!("{}", credential);

        assert!(!rendered.contains("PRIVATE KEY"));
        assert!(rendered.contains("42"));
        assert!(rendered.contains("99"));
    }

    #[test]
    fn test_invalid_key_error_does_not_echo_input() {
        // A parser that echoed its input would leak key bytes into logs.
        let almost = "-----BEGIN RSA PRIVATE KEY-----\nc2VjcmV0LWtleS1ieXRlcw==\n-----END RSA PRIVATE KEY-----";
        let err = GitHubAppCredential::new(1, 2, almost).unwrap_err();
        assert!(!err.to_string().contains("c2VjcmV0"));
    }

    #[test]
    fn test_service_id() {
        assert_eq!(github_app_service_id().as_str(), "github-app");
    }

    #[test]
    fn test_error_maps_to_token_error() {
        let err = GitHubAppError::NotRegistered {
            account: "raibid-labs".to_string(),
        };
        let token_err: TokenError = err.into();
        assert!(matches!(token_err, TokenError::NotFound { .. }));

        let err = GitHubAppError::Http {
            message: "connection refused".to_string(),
        };
        let token_err: TokenError = err.into();
        assert!(matches!(token_err, TokenError::NetworkError { .. }));
    }
}
