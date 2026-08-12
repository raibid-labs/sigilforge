//! Installation-token minting, caching, and expiry.
//!
//! An installation token is what consumers actually use: a `ghs_...` bearer
//! token, scoped to the repositories the App is installed on, valid for one
//! hour. Minting one costs an RSA signature and a round trip to GitHub, so
//! [`GitHubAppTokenManager`] caches it in the [`SecretStore`] and re-mints only
//! when the cached token is within [`DEFAULT_REFRESH_BUFFER_SECS`] of expiring.
//!
//! # Relationship to [`TokenManager`]
//!
//! [`GitHubAppTokenManager`] is a **sibling** of
//! [`DefaultTokenManager`](crate::token_manager::DefaultTokenManager), not a
//! configuration of it. The OAuth manager's refresh path is
//! `refresh_token -> POST /token`; a GitHub App has no refresh token, and its
//! renewal credential is a private key that signs a fresh assertion each time.
//! Reusing that type would have meant threading a second, unrelated renewal
//! mechanism through it.
//!
//! It does implement the [`TokenManager`] trait, so it drops into generic code
//! (including [`DefaultReferenceResolver`](crate::resolve::DefaultReferenceResolver))
//! unchanged. Three of the trait's five methods are only a partial fit and are
//! documented individually below.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use super::{GITHUB_APP_SERVICE, GitHubAppCredential, GitHubAppError, jwt};
use crate::model::{AccountId, CredentialRef, CredentialType, ServiceId};
use crate::resolve::{ReferenceResolver, ResolveError, ResolvedValue};
use crate::store::{Secret, SecretStore};
use crate::token::{Token, TokenError, TokenInfo, TokenManager, TokenSet};

/// The public GitHub API root.
///
/// GitHub Enterprise Server installations override this via
/// [`GitHubAppTokenManager::with_api_base_url`]; tests point it at a local mock.
pub const GITHUB_API_BASE_URL: &str = "https://api.github.com";

/// Seconds before `expires_at` at which a cached token is treated as spent.
///
/// Installation tokens live an hour, so five minutes of headroom costs almost
/// nothing and removes the class of failure where a token expires mid-request.
pub const DEFAULT_REFRESH_BUFFER_SECS: i64 = 300;

/// The API version header GitHub asks clients to pin.
const GITHUB_API_VERSION: &str = "2022-11-28";

/// GitHub rejects API requests without a `User-Agent`.
const USER_AGENT: &str = concat!("sigilforge/", env!("CARGO_PKG_VERSION"));

/// Cap on how much of an error body is quoted back, so a hostile or enormous
/// response cannot flood the logs.
const MAX_ERROR_BODY_CHARS: usize = 512;

/// Something that can produce a current GitHub App installation token.
///
/// Extracted as an object-safe trait so [`DefaultReferenceResolver`](crate::resolve::DefaultReferenceResolver)
/// can hold one without gaining a second store type parameter.
#[async_trait]
pub trait InstallationTokenSource: Send + Sync {
    /// Return a valid installation token for `account`, minting a new one if the
    /// cached token is missing or close to expiry.
    async fn ensure_installation_token(&self, account: &AccountId)
    -> Result<Token, GitHubAppError>;
}

/// Mints and caches GitHub App installation tokens.
///
/// # Example
///
/// ```rust,no_run
/// # #[cfg(feature = "github-app")]
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use sigilforge_core::{
///     AccountId, MemoryStore,
///     github_app::{GitHubAppCredential, GitHubAppTokenManager},
/// };
///
/// let manager = GitHubAppTokenManager::new(MemoryStore::new());
/// let account = AccountId::new("raibid-labs");
///
/// let pem = std::fs::read_to_string("app.private-key.pem")?;
/// manager
///     .register(&account, &GitHubAppCredential::new(1234567, 89012345, pem)?)
///     .await?;
///
/// let token = manager.ensure_installation_token(&account).await?;
/// // token.access_token.expose() is a `ghs_...` bearer token
/// # Ok(())
/// # }
/// ```
pub struct GitHubAppTokenManager<S: SecretStore> {
    store: S,
    http: reqwest::Client,
    api_base_url: String,
    refresh_buffer: Duration,
    jwt_ttl: Duration,
}

impl<S: SecretStore> GitHubAppTokenManager<S> {
    /// Create a manager backed by `store`, talking to the public GitHub API.
    pub fn new(store: S) -> Self {
        Self {
            store,
            http: reqwest::Client::new(),
            api_base_url: GITHUB_API_BASE_URL.to_string(),
            refresh_buffer: Duration::seconds(DEFAULT_REFRESH_BUFFER_SECS),
            jwt_ttl: Duration::seconds(jwt::DEFAULT_JWT_TTL_SECS),
        }
    }

    /// Point the manager at a different API root (GitHub Enterprise, or a mock).
    ///
    /// Any trailing slash is trimmed.
    pub fn with_api_base_url(mut self, url: impl Into<String>) -> Self {
        self.api_base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Override how long before `expires_at` a cached token is re-minted.
    pub fn with_refresh_buffer(mut self, buffer: Duration) -> Self {
        self.refresh_buffer = buffer;
        self
    }

    /// Override the lifetime of the signed App JWT.
    ///
    /// Clamped to GitHub's ten-minute ceiling by [`AppJwtClaims`](super::AppJwtClaims).
    pub fn with_jwt_ttl(mut self, ttl: Duration) -> Self {
        self.jwt_ttl = ttl;
        self
    }

    /// The API root this manager will call.
    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    /// Borrow the underlying secret store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Store a GitHub App registration for `account`.
    ///
    /// The private key goes into the secret store - the OS keyring in normal
    /// operation - and is never written to a config file. Any previously cached
    /// installation token is discarded, because a token minted by the old
    /// registration says nothing about the new one.
    pub async fn register(
        &self,
        account: &AccountId,
        credential: &GitHubAppCredential,
    ) -> Result<(), GitHubAppError> {
        self.put(
            account,
            CredentialType::AppId,
            &credential.app_id().to_string(),
        )
        .await?;
        self.put(
            account,
            CredentialType::InstallationId,
            &credential.installation_id().to_string(),
        )
        .await?;
        self.store
            .set(
                &Self::key(account, CredentialType::PrivateKey),
                credential.private_key_pem(),
            )
            .await?;

        self.clear_cached_token(account).await?;

        // Read the key back. Some keyring configurations accept a write and then
        // have nothing to return - a failure that otherwise stays invisible until
        // a cluster needs the credential and there is no key. Better to fail here.
        if !self.is_registered(account).await? {
            return Err(GitHubAppError::MalformedRegistration {
                account: account.to_string(),
                message: "the secret store accepted the private key but did not \
                          return it on read; the registration did not persist"
                    .to_string(),
            });
        }

        tracing::info!(
            "Registered GitHub App {} installation {} as {}/{}",
            credential.app_id(),
            credential.installation_id(),
            GITHUB_APP_SERVICE,
            account
        );

        Ok(())
    }

    /// Load the registration for `account`.
    ///
    /// # Errors
    ///
    /// - [`GitHubAppError::NotRegistered`] if no registration exists
    /// - [`GitHubAppError::MalformedRegistration`] if a stored field will not parse
    pub async fn load_credential(
        &self,
        account: &AccountId,
    ) -> Result<GitHubAppCredential, GitHubAppError> {
        let app_id = self.required_u64(account, CredentialType::AppId).await?;
        let installation_id = self
            .required_u64(account, CredentialType::InstallationId)
            .await?;

        let private_key = self
            .get(account, CredentialType::PrivateKey)
            .await?
            .ok_or_else(|| GitHubAppError::NotRegistered {
                account: account.to_string(),
            })?;

        GitHubAppCredential::new(app_id, installation_id, private_key.expose())
    }

    /// Whether a registration exists for `account`.
    pub async fn is_registered(&self, account: &AccountId) -> Result<bool, GitHubAppError> {
        Ok(self
            .store
            .exists(&Self::key(account, CredentialType::PrivateKey))
            .await?)
    }

    /// Delete the registration and any cached token for `account`.
    ///
    /// Succeeds even when nothing was stored.
    pub async fn remove(&self, account: &AccountId) -> Result<(), GitHubAppError> {
        for cred_type in [
            CredentialType::AppId,
            CredentialType::InstallationId,
            CredentialType::PrivateKey,
        ] {
            self.store.delete(&Self::key(account, cred_type)).await?;
        }
        self.clear_cached_token(account).await?;
        Ok(())
    }

    /// Return a valid installation token, minting only when necessary.
    ///
    /// The cached token is reused unless it is missing or within
    /// [`DEFAULT_REFRESH_BUFFER_SECS`] of `expires_at`. This is the method
    /// consumers should call; it is also the body of the
    /// [`InstallationTokenSource`] impl.
    ///
    /// A cached token with no readable expiry is treated as spent rather than as
    /// non-expiring. GitHub always returns `expires_at`, so its absence means the
    /// cache entry is damaged - and the failure mode of guessing wrong in the
    /// other direction is a token that is never renewed.
    pub async fn ensure_installation_token(
        &self,
        account: &AccountId,
    ) -> Result<Token, GitHubAppError> {
        if let Some(token) = self.cached_token(account).await?
            && token.expires_at.is_some()
            && !token.expires_within(self.refresh_buffer)
        {
            tracing::debug!(
                "Using cached installation token for {}/{}",
                GITHUB_APP_SERVICE,
                account
            );
            return Ok(token);
        }

        self.mint_installation_token(account).await
    }

    /// Read the cached installation token, without minting.
    ///
    /// Returns `Ok(None)` if nothing is cached. A cached-but-stale token is still
    /// returned; use [`Self::ensure_installation_token`] for the freshness check.
    pub async fn cached_token(&self, account: &AccountId) -> Result<Option<Token>, GitHubAppError> {
        let Some(secret) = self.get(account, CredentialType::InstallationToken).await? else {
            return Ok(None);
        };

        let mut token = Token::new(secret.expose());
        if let Some(expiry) = self.get(account, CredentialType::TokenExpiry).await?
            && let Some(expires_at) = parse_expiry(expiry.expose())
        {
            token = token.with_expiry(expires_at);
        }

        Ok(Some(token))
    }

    /// Mint a fresh installation token, ignoring and replacing any cached one.
    ///
    /// This performs the full exchange: sign an RS256 JWT, `POST` it to
    /// `/app/installations/{id}/access_tokens`, cache the result.
    ///
    /// Prefer [`Self::ensure_installation_token`] - minting per request wastes a
    /// round trip and burns GitHub's rate limit for no benefit.
    pub async fn mint_installation_token(
        &self,
        account: &AccountId,
    ) -> Result<Token, GitHubAppError> {
        let credential = self.load_credential(account).await?;
        let assertion = jwt::sign_app_jwt(&credential, Utc::now(), self.jwt_ttl)?;

        let url = format!(
            "{}/app/installations/{}/access_tokens",
            self.api_base_url,
            credential.installation_id()
        );

        tracing::debug!(
            "Minting installation token for {}/{} (app {}, installation {})",
            GITHUB_APP_SERVICE,
            account,
            credential.app_id(),
            credential.installation_id()
        );

        let response = self
            .http
            .post(&url)
            .bearer_auth(assertion.expose())
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .send()
            .await
            .map_err(|e| GitHubAppError::Http {
                message: format!("POST {} failed: {}", url, e),
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| GitHubAppError::Http {
            message: format!("could not read response body: {}", e),
        })?;

        if !status.is_success() {
            return Err(GitHubAppError::GitHubApi {
                status: status.as_u16(),
                message: describe_api_error(status.as_u16(), &body),
            });
        }

        let minted: InstallationTokenResponse =
            serde_json::from_str(&body).map_err(|e| GitHubAppError::InvalidResponse {
                message: format!("could not parse access token response: {}", e),
            })?;

        let token = Token::new(minted.token).with_expiry(minted.expires_at);
        self.cache_token(account, &token).await?;

        tracing::info!(
            "Minted GitHub App installation token for {}/{}, expires at {}",
            GITHUB_APP_SERVICE,
            account,
            minted.expires_at
        );

        Ok(token)
    }

    /// Cache a token and its expiry.
    async fn cache_token(&self, account: &AccountId, token: &Token) -> Result<(), GitHubAppError> {
        self.store
            .set(
                &Self::key(account, CredentialType::InstallationToken),
                &token.access_token,
            )
            .await?;

        if let Some(expires_at) = token.expires_at {
            self.put(
                account,
                CredentialType::TokenExpiry,
                &expires_at.to_rfc3339(),
            )
            .await?;
        } else {
            self.store
                .delete(&Self::key(account, CredentialType::TokenExpiry))
                .await?;
        }

        Ok(())
    }

    /// Drop the cached token and expiry.
    async fn clear_cached_token(&self, account: &AccountId) -> Result<(), GitHubAppError> {
        self.store
            .delete(&Self::key(account, CredentialType::InstallationToken))
            .await?;
        self.store
            .delete(&Self::key(account, CredentialType::TokenExpiry))
            .await?;
        Ok(())
    }

    /// Storage key for one field of an account's registration.
    fn key(account: &AccountId, cred_type: CredentialType) -> String {
        CredentialRef::new(GITHUB_APP_SERVICE, account.clone(), cred_type).to_key()
    }

    async fn get(
        &self,
        account: &AccountId,
        cred_type: CredentialType,
    ) -> Result<Option<Secret>, GitHubAppError> {
        Ok(self.store.get(&Self::key(account, cred_type)).await?)
    }

    async fn put(
        &self,
        account: &AccountId,
        cred_type: CredentialType,
        value: &str,
    ) -> Result<(), GitHubAppError> {
        self.store
            .set(&Self::key(account, cred_type), &Secret::new(value))
            .await?;
        Ok(())
    }

    async fn required_u64(
        &self,
        account: &AccountId,
        cred_type: CredentialType,
    ) -> Result<u64, GitHubAppError> {
        let raw = self.get(account, cred_type.clone()).await?.ok_or_else(|| {
            GitHubAppError::NotRegistered {
                account: account.to_string(),
            }
        })?;

        raw.expose()
            .trim()
            .parse::<u64>()
            .map_err(|_| GitHubAppError::MalformedRegistration {
                account: account.to_string(),
                message: format!("{} is not a number", cred_type.as_str()),
            })
    }
}

#[async_trait]
impl<S: SecretStore + Send + Sync + 'static> InstallationTokenSource for GitHubAppTokenManager<S> {
    async fn ensure_installation_token(
        &self,
        account: &AccountId,
    ) -> Result<Token, GitHubAppError> {
        GitHubAppTokenManager::ensure_installation_token(self, account).await
    }
}

/// GitHub App tokens fit the [`TokenManager`] shape only partially; each method
/// documents where the analogy holds and where it stops.
#[async_trait]
impl<S: SecretStore + Send + Sync + 'static> TokenManager for GitHubAppTokenManager<S> {
    /// Exact fit: returns a valid installation token, minting when needed.
    ///
    /// `service` must be `github-app`; any other service is a caller error rather
    /// than something this manager could look up.
    async fn ensure_access_token(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Token, TokenError> {
        Self::require_github_app_service(service)?;
        Ok(self.ensure_installation_token(account).await?)
    }

    /// Partial fit: the returned [`TokenSet`] never has a refresh token, because
    /// GitHub Apps do not issue one - the private key plays that role.
    async fn get_token_set(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Option<TokenSet>, TokenError> {
        Self::require_github_app_service(service)?;
        Ok(self.cached_token(account).await?.map(TokenSet::new))
    }

    /// Partial fit: writes to the local cache only.
    ///
    /// There is no meaningful "store these tokens" step for a GitHub App - tokens
    /// come from minting, not from a flow the caller ran. This exists so cache
    /// seeding (and the trait) work; the refresh token in `token_set`, if any, is
    /// ignored.
    async fn store_token_set(
        &self,
        service: &ServiceId,
        account: &AccountId,
        token_set: TokenSet,
    ) -> Result<(), TokenError> {
        Self::require_github_app_service(service)?;
        self.cache_token(account, &token_set.access_token).await?;
        Ok(())
    }

    /// Partial fit: **local** revocation only.
    ///
    /// GitHub's `DELETE /installation/token` revokes the token used to
    /// authenticate the call, which is not a token this manager can present
    /// on demand. Dropping the cache forces the next call to mint afresh; the
    /// old token remains valid at GitHub until its hour is up.
    ///
    /// The registration itself survives - use [`Self::remove`] to delete that.
    async fn revoke_tokens(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<(), TokenError> {
        Self::require_github_app_service(service)?;
        self.clear_cached_token(account).await?;

        tracing::info!(
            "Cleared cached installation token for {}/{} (GitHub-side token remains valid until expiry)",
            GITHUB_APP_SERVICE,
            account
        );

        Ok(())
    }

    /// Partial fit: reports from the local cache.
    ///
    /// GitHub exposes no introspection endpoint for installation tokens, so
    /// `scopes` is always empty - an installation's permissions live on the App,
    /// not on the token.
    async fn introspect_token(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<TokenInfo, TokenError> {
        Self::require_github_app_service(service)?;

        let token = self
            .cached_token(account)
            .await?
            .ok_or_else(|| TokenError::NotFound {
                service: service.to_string(),
                account: account.to_string(),
            })?;

        Ok(TokenInfo {
            // Same rule as `ensure_installation_token`: no readable expiry means
            // the cache entry cannot be trusted, not that the token is eternal.
            active: token.expires_at.is_some() && !token.expires_within(self.refresh_buffer),
            subject: None,
            client_id: None,
            scopes: Vec::new(),
            expires_at: token.expires_at,
        })
    }
}

impl<S: SecretStore> GitHubAppTokenManager<S> {
    /// Reject service ids this manager cannot serve.
    fn require_github_app_service(service: &ServiceId) -> Result<(), TokenError> {
        if service.as_str() == GITHUB_APP_SERVICE {
            Ok(())
        } else {
            Err(TokenError::ProviderNotConfigured {
                provider: service.to_string(),
            })
        }
    }
}

/// Resolves `auth://github-app/{account}/{credential}` references.
///
/// | Reference | Resolves to |
/// |-----------|-------------|
/// | `auth://github-app/{account}/installation_token` | a fresh installation token |
/// | `auth://github-app/{account}/token` | the same (alias) |
/// | `auth://github-app/{account}/app_id` | the App ID |
/// | `auth://github-app/{account}/installation_id` | the installation ID |
/// | `auth://github-app/{account}/private_key` | the PEM private key |
#[async_trait]
impl<S: SecretStore + Send + Sync + 'static> ReferenceResolver for GitHubAppTokenManager<S> {
    async fn resolve(&self, reference: &str) -> Result<ResolvedValue, ResolveError> {
        let cred_ref =
            CredentialRef::from_auth_uri(reference).map_err(|e| ResolveError::InvalidFormat {
                message: e.to_string(),
            })?;

        self.resolve_ref(&cred_ref).await
    }

    async fn resolve_ref(&self, cred_ref: &CredentialRef) -> Result<ResolvedValue, ResolveError> {
        if cred_ref.service.as_str() != GITHUB_APP_SERVICE {
            return Err(ResolveError::UnsupportedScheme {
                scheme: format!("auth://{}", cred_ref.service),
            });
        }

        match &cred_ref.credential_type {
            // `token` is accepted as an alias so consumers written against the
            // generic `auth://service/account/token` shape keep working.
            CredentialType::InstallationToken | CredentialType::AccessToken => {
                let token = self
                    .ensure_installation_token(&cred_ref.account)
                    .await
                    .map_err(TokenError::from)?;
                Ok(ResolvedValue::Secret(token.access_token))
            }

            cred_type => {
                let key = CredentialRef::new(
                    GITHUB_APP_SERVICE,
                    cred_ref.account.clone(),
                    cred_type.clone(),
                )
                .to_key();

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
        scheme == "auth"
    }
}

impl<S: SecretStore> std::fmt::Debug for GitHubAppTokenManager<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubAppTokenManager")
            .field("api_base_url", &self.api_base_url)
            .field("refresh_buffer_secs", &self.refresh_buffer.num_seconds())
            .field("jwt_ttl_secs", &self.jwt_ttl.num_seconds())
            .finish_non_exhaustive()
    }
}

/// GitHub's `POST /app/installations/{id}/access_tokens` response.
///
/// Deliberately not `Debug`: the `token` field is the credential.
#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Parse a cached expiry, accepting RFC 3339 or a bare Unix timestamp.
///
/// The Unix fallback exists because
/// [`DefaultTokenManager`](crate::token_manager::DefaultTokenManager) writes
/// `token_expiry` as a Unix timestamp while the CLI's direct path writes RFC 3339.
/// Reading both means a store written by either does not silently look expired.
fn parse_expiry(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();

    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }

    raw.parse::<i64>()
        .ok()
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
}

/// Turn a GitHub error response into something actionable.
///
/// GitHub's failure modes here are few and each has a specific cause worth
/// naming, because the generic body ("Bad credentials") does not distinguish
/// a wrong App ID from a wrong installation ID.
fn describe_api_error(status: u16, body: &str) -> String {
    let hint = match status {
        401 => Some(
            "the App JWT was rejected - check the App ID and that the private key \
             belongs to that App, and that this machine's clock is accurate",
        ),
        404 => Some(
            "installation not found - check the installation ID, and that the App \
             is still installed on the org",
        ),
        403 => Some("the App lacks permission for this installation"),
        422 => Some("the requested repositories or permissions exceed the installation's grant"),
        _ => None,
    };

    let body = body.trim();
    let truncated: String = if body.chars().count() > MAX_ERROR_BODY_CHARS {
        body.chars().take(MAX_ERROR_BODY_CHARS).collect::<String>() + "..."
    } else {
        body.to_string()
    };

    match hint {
        Some(hint) if truncated.is_empty() => hint.to_string(),
        Some(hint) => format!("{} ({})", hint, truncated),
        None if truncated.is_empty() => "no response body".to_string(),
        None => truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_app::test_support::TEST_PRIVATE_KEY_PEM;
    use crate::store::MemoryStore;

    fn account() -> AccountId {
        AccountId::new("raibid-labs")
    }

    fn credential() -> GitHubAppCredential {
        GitHubAppCredential::new(1234567, 89012345, TEST_PRIVATE_KEY_PEM).unwrap()
    }

    fn manager() -> GitHubAppTokenManager<MemoryStore> {
        GitHubAppTokenManager::new(MemoryStore::new())
    }

    #[tokio::test]
    async fn test_register_then_load_roundtrip() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();

        let loaded = manager.load_credential(&account()).await.unwrap();
        assert_eq!(loaded.app_id(), 1234567);
        assert_eq!(loaded.installation_id(), 89012345);
        assert_eq!(
            loaded.private_key_pem().expose().trim(),
            TEST_PRIVATE_KEY_PEM.trim()
        );
    }

    #[tokio::test]
    async fn test_register_writes_expected_storage_keys() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();

        let store = manager.store();
        assert!(
            store
                .exists("sigilforge/github-app/raibid-labs/app_id")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("sigilforge/github-app/raibid-labs/installation_id")
                .await
                .unwrap()
        );
        assert!(
            store
                .exists("sigilforge/github-app/raibid-labs/private_key")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_load_unregistered_account_errors() {
        let err = manager().load_credential(&account()).await.unwrap_err();
        assert!(matches!(err, GitHubAppError::NotRegistered { .. }));
    }

    #[tokio::test]
    async fn test_is_registered() {
        let manager = manager();
        assert!(!manager.is_registered(&account()).await.unwrap());

        manager.register(&account(), &credential()).await.unwrap();
        assert!(manager.is_registered(&account()).await.unwrap());
    }

    #[tokio::test]
    async fn test_remove_clears_everything() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();
        manager.remove(&account()).await.unwrap();

        assert!(!manager.is_registered(&account()).await.unwrap());
        assert!(manager.cached_token(&account()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_remove_is_idempotent() {
        let manager = manager();
        manager.remove(&account()).await.unwrap();
        manager.remove(&account()).await.unwrap();
    }

    #[tokio::test]
    async fn test_malformed_app_id_reports_clearly() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();
        manager
            .store()
            .set(
                "sigilforge/github-app/raibid-labs/app_id",
                &Secret::new("not-a-number"),
            )
            .await
            .unwrap();

        let err = manager.load_credential(&account()).await.unwrap_err();
        assert!(matches!(err, GitHubAppError::MalformedRegistration { .. }));
        assert!(err.to_string().contains("app_id"));
    }

    #[tokio::test]
    async fn test_register_invalidates_cached_token() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();

        let token = Token::new("ghs_stale").with_expiry(Utc::now() + Duration::hours(1));
        manager.cache_token(&account(), &token).await.unwrap();
        assert!(manager.cached_token(&account()).await.unwrap().is_some());

        manager.register(&account(), &credential()).await.unwrap();
        assert!(manager.cached_token(&account()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cached_token_roundtrips_expiry() {
        let manager = manager();
        let expires_at = Utc::now() + Duration::minutes(45);
        let token = Token::new("ghs_cached").with_expiry(expires_at);

        manager.cache_token(&account(), &token).await.unwrap();
        let cached = manager.cached_token(&account()).await.unwrap().unwrap();

        assert_eq!(cached.access_token.expose(), "ghs_cached");
        assert_eq!(
            cached.expires_at.unwrap().timestamp(),
            expires_at.timestamp()
        );
    }

    #[tokio::test]
    async fn test_cached_token_absent_when_nothing_stored() {
        assert!(manager().cached_token(&account()).await.unwrap().is_none());
    }

    #[test]
    fn test_parse_expiry_accepts_rfc3339() {
        let parsed = parse_expiry("2026-03-14T15:09:26Z").unwrap();
        assert_eq!(parsed.timestamp(), 1773500966);
    }

    #[test]
    fn test_parse_expiry_accepts_unix_timestamp() {
        // DefaultTokenManager writes this form; reading it avoids treating a
        // perfectly good cached token as unreadable.
        let parsed = parse_expiry("1773500966").unwrap();
        assert_eq!(parsed.timestamp(), 1773500966);
    }

    #[test]
    fn test_parse_expiry_rejects_garbage() {
        assert!(parse_expiry("sometime next tuesday").is_none());
    }

    #[test]
    fn test_describe_api_error_explains_401() {
        let message = describe_api_error(401, r#"{"message":"Bad credentials"}"#);
        assert!(message.contains("App ID"));
        assert!(message.contains("Bad credentials"));
    }

    #[test]
    fn test_describe_api_error_explains_404() {
        let message = describe_api_error(404, "");
        assert!(message.contains("installation ID"));
    }

    #[test]
    fn test_describe_api_error_truncates_huge_bodies() {
        let body = "x".repeat(MAX_ERROR_BODY_CHARS * 4);
        let message = describe_api_error(500, &body);
        assert!(message.chars().count() < MAX_ERROR_BODY_CHARS + 16);
        assert!(message.ends_with("..."));
    }

    #[test]
    fn test_manager_debug_hides_the_store() {
        let rendered = format!("{:?}", manager());
        assert!(rendered.contains("api_base_url"));
        assert!(!rendered.contains("PRIVATE KEY"));
    }

    #[test]
    fn test_with_api_base_url_trims_trailing_slash() {
        let manager = manager().with_api_base_url("https://github.example.com/api/v3/");
        assert_eq!(manager.api_base_url(), "https://github.example.com/api/v3");
    }

    #[tokio::test]
    async fn test_token_manager_rejects_other_services() {
        let manager = manager();
        let result = manager
            .ensure_access_token(&ServiceId::new("spotify"), &account())
            .await;

        assert!(matches!(
            result,
            Err(TokenError::ProviderNotConfigured { .. })
        ));
    }

    #[tokio::test]
    async fn test_token_manager_get_token_set_has_no_refresh_token() {
        let manager = manager();
        let token = Token::new("ghs_cached").with_expiry(Utc::now() + Duration::minutes(45));
        manager.cache_token(&account(), &token).await.unwrap();

        let token_set = manager
            .get_token_set(&super::super::github_app_service_id(), &account())
            .await
            .unwrap()
            .unwrap();

        assert!(token_set.refresh_token.is_none());
    }

    #[tokio::test]
    async fn test_token_manager_revoke_clears_cache_only() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();
        let token = Token::new("ghs_cached").with_expiry(Utc::now() + Duration::minutes(45));
        manager.cache_token(&account(), &token).await.unwrap();

        manager
            .revoke_tokens(&super::super::github_app_service_id(), &account())
            .await
            .unwrap();

        assert!(manager.cached_token(&account()).await.unwrap().is_none());
        // The registration survives - revoking a token is not deregistering.
        assert!(manager.is_registered(&account()).await.unwrap());
    }

    #[tokio::test]
    async fn test_token_manager_introspect_reports_expiry() {
        let manager = manager();
        let expires_at = Utc::now() + Duration::minutes(45);
        manager
            .cache_token(&account(), &Token::new("ghs_x").with_expiry(expires_at))
            .await
            .unwrap();

        let info = manager
            .introspect_token(&super::super::github_app_service_id(), &account())
            .await
            .unwrap();

        assert!(info.active);
        assert!(info.scopes.is_empty());
        assert_eq!(info.expires_at.unwrap().timestamp(), expires_at.timestamp());
    }

    #[tokio::test]
    async fn test_token_manager_introspect_marks_stale_token_inactive() {
        let manager = manager();
        manager
            .cache_token(
                &account(),
                &Token::new("ghs_x").with_expiry(Utc::now() + Duration::minutes(1)),
            )
            .await
            .unwrap();

        let info = manager
            .introspect_token(&super::super::github_app_service_id(), &account())
            .await
            .unwrap();

        // Inside the five-minute refresh buffer, so not usable.
        assert!(!info.active);
    }

    #[tokio::test]
    async fn test_resolver_rejects_other_services() {
        let manager = manager();
        let result = manager.resolve("auth://spotify/personal/token").await;

        assert!(matches!(
            result,
            Err(ResolveError::UnsupportedScheme { .. })
        ));
    }

    #[tokio::test]
    async fn test_resolver_rejects_malformed_reference() {
        let manager = manager();
        let result = manager.resolve("auth://github-app/raibid-labs").await;

        assert!(matches!(result, Err(ResolveError::InvalidFormat { .. })));
    }

    #[tokio::test]
    async fn test_resolver_returns_app_id() {
        let manager = manager();
        manager.register(&account(), &credential()).await.unwrap();

        let value = manager
            .resolve("auth://github-app/raibid-labs/app_id")
            .await
            .unwrap();

        assert_eq!(value.expose(), "1234567");
    }

    #[tokio::test]
    async fn test_resolver_reports_missing_credential() {
        let manager = manager();
        let result = manager
            .resolve("auth://github-app/raibid-labs/app_id")
            .await;

        assert!(matches!(result, Err(ResolveError::NotFound { .. })));
    }

    #[tokio::test]
    async fn test_resolver_supports_auth_scheme_only() {
        let manager = manager();
        assert!(manager.supports_scheme("auth"));
        assert!(!manager.supports_scheme("vals"));
    }
}
