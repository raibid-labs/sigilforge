# Sigilforge Interfaces

This document defines the trait-level API contracts and reference formats for Sigilforge.

## Core Traits

### SecretStore

The `SecretStore` trait abstracts secret storage backends (keyring, encrypted files, memory).

```rust
use async_trait::async_trait;

/// A secret value stored in the backend.
/// Wraps sensitive data to prevent accidental logging.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self;
    pub fn expose(&self) -> &str;
}

/// Error type for secret store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("secret not found: {key}")]
    NotFound { key: String },

    #[error("access denied to secret: {key}")]
    AccessDenied { key: String },

    #[error("backend error: {message}")]
    BackendError { message: String },

    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

/// Abstraction over secret storage backends.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Retrieve a secret by key.
    /// Returns None if the key doesn't exist.
    async fn get(&self, key: &str) -> Result<Option<Secret>, StoreError>;

    /// Store a secret at the given key.
    /// Overwrites any existing value.
    async fn set(&self, key: &str, secret: &Secret) -> Result<(), StoreError>;

    /// Delete a secret by key.
    /// Returns Ok(()) even if the key didn't exist.
    async fn delete(&self, key: &str) -> Result<(), StoreError>;

    /// List all keys matching a prefix.
    /// Returns an empty vec if no keys match.
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError>;

    /// Check if a key exists without retrieving the value.
    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        Ok(self.get(key).await?.is_some())
    }
}
```

#### Key Naming Convention

Keys follow a hierarchical pattern:
```
sigilforge/{service}/{account}/{credential_type}
```

| Credential Type | Description |
|-----------------|-------------|
| `access_token` | Current access token |
| `refresh_token` | OAuth refresh token |
| `token_expiry` | Access token expiry timestamp |
| `api_key` | Static API key |
| `client_secret` | OAuth client secret (provider-level) |
| `app_id` | GitHub App ID |
| `installation_id` | GitHub App installation ID |
| `private_key` | GitHub App PEM private key |
| `installation_token` | GitHub App installation access token (cache) |

**Examples:**
```
sigilforge/spotify/personal/access_token
sigilforge/spotify/personal/refresh_token
sigilforge/github/work/api_key
sigilforge/gmail/_provider/client_secret    # Provider-level, not account-level
sigilforge/github-app/raibid-labs/private_key
```

---

### TokenManager

The `TokenManager` trait handles token lifecycle, including fetching, caching, and refreshing.

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// An access token with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    /// The access token value.
    pub access_token: Secret,

    /// Token type (usually "Bearer").
    pub token_type: String,

    /// When the token expires (if known).
    pub expires_at: Option<DateTime<Utc>>,
}

impl Token {
    /// Check if the token is expired or will expire within the buffer period.
    pub fn is_expired(&self) -> bool;

    /// Check if the token will expire within the given duration.
    pub fn expires_within(&self, duration: chrono::Duration) -> bool;
}

/// A pair of access and refresh tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub token_type: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
}

/// Error type for token operations.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("account not found: {service}/{account}")]
    AccountNotFound { service: String, account: String },

    #[error("no refresh token available for {service}/{account}")]
    NoRefreshToken { service: String, account: String },

    #[error("token refresh failed: {message}")]
    RefreshFailed { message: String },

    #[error("authentication required for {service}/{account}")]
    AuthRequired { service: String, account: String },

    #[error("provider not configured: {service}")]
    ProviderNotConfigured { service: String },

    #[error("store error: {0}")]
    StoreError(#[from] StoreError),

    #[error("network error: {0}")]
    NetworkError(String),
}

/// Manages token lifecycle for OAuth-based services.
#[async_trait]
pub trait TokenManager: Send + Sync {
    /// Get a valid access token, refreshing if necessary.
    ///
    /// This is the primary method consumers should use. It:
    /// 1. Checks for a cached token
    /// 2. Validates token expiry (with 5-minute buffer)
    /// 3. Refreshes using refresh_token if expired
    /// 4. Returns error if no valid token and no refresh possible
    async fn ensure_access_token(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Token, TokenError>;

    /// Force a token refresh, even if current token is valid.
    async fn refresh_token(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Token, TokenError>;

    /// Store a new token set (typically after OAuth flow completion).
    async fn store_tokens(
        &self,
        service: &ServiceId,
        account: &AccountId,
        tokens: TokenSet,
    ) -> Result<(), TokenError>;

    /// Revoke tokens for an account (if provider supports it).
    async fn revoke_tokens(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<(), TokenError>;

    /// Get token metadata without exposing the token value.
    async fn get_token_info(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<TokenInfo, TokenError>;
}

/// Token metadata without sensitive values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub token_type: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub is_expired: bool,
}
```

---

### ReferenceResolver

The `ReferenceResolver` trait handles resolving credential references from various formats.

```rust
use async_trait::async_trait;

/// A resolved credential value.
#[derive(Debug, Clone)]
pub enum ResolvedValue {
    /// An access token (may need refresh).
    Token(Token),

    /// A static secret (API key, etc.).
    Secret(Secret),

    /// Raw string value.
    String(String),
}

/// Error type for reference resolution.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("invalid reference format: {reference}")]
    InvalidFormat { reference: String },

    #[error("unknown scheme: {scheme}")]
    UnknownScheme { scheme: String },

    #[error("resolution failed: {message}")]
    ResolutionFailed { message: String },

    #[error("token error: {0}")]
    TokenError(#[from] TokenError),

    #[error("external resolver error: {0}")]
    ExternalError(String),
}

/// Resolves credential references to actual values.
#[async_trait]
pub trait ReferenceResolver: Send + Sync {
    /// Resolve a reference string to a credential value.
    ///
    /// Supported formats:
    /// - `auth://service/account/token` - OAuth access token
    /// - `auth://service/account/api_key` - Static API key
    /// - `vals:ref+...` - External vals-style reference
    async fn resolve(&self, reference: &str) -> Result<ResolvedValue, ResolveError>;

    /// Check if this resolver can handle the given reference.
    fn can_resolve(&self, reference: &str) -> bool;
}
```

---

### InstallationTokenSource

GitHub App installation tokens are **minted**, not refreshed. The renewal
credential is an RSA private key that signs a fresh assertion, not a refresh
token, so `TokenManager`'s refresh path does not apply. `GitHubAppTokenManager`
is a sibling implementation rather than a configuration of `DefaultTokenManager`.

```rust
use async_trait::async_trait;

/// Something that can produce a current GitHub App installation token.
///
/// Object-safe, so `DefaultReferenceResolver` can hold one without gaining a
/// second store type parameter.
#[async_trait]
pub trait InstallationTokenSource: Send + Sync {
    /// Return a valid installation token, minting a new one if the cached token
    /// is missing or within the refresh buffer of `expires_at`.
    async fn ensure_installation_token(
        &self,
        account: &AccountId,
    ) -> Result<Token, GitHubAppError>;
}
```

`GitHubAppTokenManager` implements `InstallationTokenSource`, `TokenManager`, and
`ReferenceResolver`. Its `TokenManager` impl is a partial fit and says so:

| Method | Fit |
|--------|-----|
| `ensure_access_token` | Exact. Requires `service == "github-app"`. |
| `get_token_set` | Partial. `refresh_token` is always `None`. |
| `store_token_set` | Partial. Writes the local cache only. |
| `revoke_tokens` | Partial. **Local** revocation; GitHub has no endpoint to revoke another installation's token, so the old token stays valid until it expires. |
| `introspect_token` | Partial. Reports from cache; `scopes` is always empty because permissions live on the App, not the token. |

---

## Reference Formats

### auth:// URI Scheme

The `auth://` scheme provides access to Sigilforge-managed credentials.

**Format:**
```
auth://{service}/{account}/{credential_type}
```

**Components:**
| Component | Description | Example |
|-----------|-------------|---------|
| `service` | Service identifier | `spotify`, `gmail`, `github` |
| `account` | Account identifier within service | `personal`, `work`, `lab` |
| `credential_type` | Type of credential | `token`, `api_key`, `refresh_token` |

**Examples:**

| Reference | Description |
|-----------|-------------|
| `auth://spotify/personal/token` | Spotify access token for "personal" account |
| `auth://gmail/work/token` | Gmail access token for "work" account |
| `auth://github/oss/api_key` | GitHub API key (PAT) for "oss" account |
| `auth://openai/default/api_key` | OpenAI API key for "default" account |
| `auth://github-app/raibid-labs/installation_token` | GitHub App installation token for the "raibid-labs" installation |

**Credential Types:**

| Type | Description | Typical Use |
|------|-------------|-------------|
| `token` | Access token (refreshed automatically) | API calls requiring OAuth |
| `api_key` | Static API key | Services with key-based auth |
| `refresh_token` | OAuth refresh token | Rarely accessed directly |
| `client_id` | OAuth client ID | Provider configuration |
| `client_secret` | OAuth client secret | Provider configuration |
| `installation_token` | GitHub App installation token (minted on demand) | Machine access to private repos |
| `app_id` | GitHub App ID | Argo CD / CI configuration |
| `installation_id` | GitHub App installation ID | Argo CD / CI configuration |
| `private_key` | GitHub App PEM private key | Argo CD `Secret` generation |

### Reserved service: `github-app`

The service id `github-app` addresses GitHub App installations rather than an
OAuth provider. The `account` component names the installation - conventionally
the organization the App is installed on.

```
auth://github-app/{account}/installation_token
```

Resolution mints a fresh installation token if the cached one is missing or
within five minutes of expiry. `token` is accepted as an alias for
`installation_token`, so consumers written against the generic
`auth://service/account/token` shape keep working.

Resolving these requires a resolver built with
`DefaultReferenceResolver::with_github_app`, or the `GitHubAppTokenManager`
itself (which implements `ReferenceResolver`). Without it, resolution returns
`ResolveError::NotConfigured` rather than silently serving a stale cached value.

See [GITHUB_APP.md](GITHUB_APP.md) for the full mechanism and setup.

### vals-style References

Sigilforge can resolve external references using the `vals` tool or compatible syntax.

**Supported Formats:**
```
vals:ref+vault://path/to/secret#key
vals:ref+sops://path/to/file.yaml#key.path
vals:ref+gcpsecrets://project/secret/version
vals:ref+awssecrets://region/secret-name#key
```

**Resolution Process:**
1. Sigilforge detects `vals:` prefix
2. Shells out to `vals` CLI if installed
3. Returns resolved value or error

**Example in Config:**
```yaml
providers:
  custom_service:
    api_key: "vals:ref+vault://secret/custom#api_key"
```

---

## Consumer Usage Patterns

### Direct Library Usage

For applications linking `sigilforge-core` directly:

```rust
use sigilforge_core::{
    ServiceId, AccountId, TokenManager,
    stores::KeyringStore,
    auth::DefaultTokenManager,
};

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize store and manager
    let store = KeyringStore::new()?;
    let manager = DefaultTokenManager::new(store);

    // Get a token
    let service = ServiceId::new("spotify");
    let account = AccountId::new("personal");

    let token = manager.ensure_access_token(&service, &account).await?;

    println!("Token expires at: {:?}", token.expires_at);

    // Use the token
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.spotify.com/v1/me")
        .bearer_auth(token.access_token.expose())
        .send()
        .await?;

    Ok(())
}
```

### Reference Resolution

For config-driven credential access:

```rust
use sigilforge_core::{ReferenceResolver, DefaultResolver};

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let resolver = DefaultResolver::new()?;

    // Resolve from config value
    let config_value = "auth://spotify/personal/token";
    let resolved = resolver.resolve(config_value).await?;

    match resolved {
        ResolvedValue::Token(token) => {
            println!("Got token expiring at {:?}", token.expires_at);
        }
        ResolvedValue::Secret(secret) => {
            println!("Got static secret");
        }
        _ => {}
    }

    Ok(())
}
```

### Via Daemon API

For applications using the daemon:

```rust
use sigilforge_client::SigilforgeClient;

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to daemon
    let client = SigilforgeClient::connect().await?;

    // Request a token
    let token = client.get_token("spotify", "personal").await?;

    println!("Got token: {}", token.access_token);

    // Or resolve a reference
    let value = client.resolve("auth://gmail/work/token").await?;

    Ok(())
}
```

### JSON-RPC API (Raw)

For non-Rust clients or debugging:

**Get Token:**
```json
// Request
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "get_token",
  "params": {
    "service": "spotify",
    "account": "personal"
  }
}

// Response
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "access_token": "BQC...",
    "token_type": "Bearer",
    "expires_at": "2024-01-15T10:30:00Z"
  }
}
```

**Resolve Reference:**
```json
// Request
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "resolve",
  "params": {
    "reference": "auth://spotify/personal/token"
  }
}

// Response
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "type": "token",
    "value": {
      "access_token": "BQC...",
      "token_type": "Bearer",
      "expires_at": "2024-01-15T10:30:00Z"
    }
  }
}
```

**List Accounts:**
```json
// Request
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "list_accounts",
  "params": {}
}

// Response
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "accounts": [
      {
        "service": "spotify",
        "id": "personal",
        "scopes": ["user-read-private", "playlist-read-private"],
        "created_at": "2024-01-10T15:30:00Z"
      },
      {
        "service": "gmail",
        "id": "work",
        "scopes": ["https://www.googleapis.com/auth/gmail.readonly"],
        "created_at": "2024-01-12T09:00:00Z"
      }
    ]
  }
}
```

**Error Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "account not found: spotify/unknown",
    "data": {
      "service": "spotify",
      "account": "unknown"
    }
  }
}
```

---

## Type Definitions

### Core Domain Types

```rust
/// Identifier for a service (e.g., "spotify", "gmail").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceId(String);

impl ServiceId {
    pub fn new(id: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

/// Identifier for an account within a service.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(id: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

/// Full account metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub service: ServiceId,
    pub id: AccountId,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
}

/// Reference to a stored credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialRef {
    pub service: ServiceId,
    pub account: AccountId,
    pub credential_type: CredentialType,
}

/// Type of credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialType {
    AccessToken,
    RefreshToken,
    ApiKey,
    ClientId,
    ClientSecret,
    AppId,
    InstallationId,
    PrivateKey,
    InstallationToken,
    Custom(String),
}

impl CredentialRef {
    /// Convert to storage key.
    pub fn to_key(&self) -> String {
        format!(
            "sigilforge/{}/{}/{}",
            self.service.as_str(),
            self.account.as_str(),
            self.credential_type.as_str()
        )
    }

    /// Parse from auth:// URI.
    pub fn from_auth_uri(uri: &str) -> Result<Self, ResolveError>;
}
```

---

## Integration with Scryforge

Scryforge will use Sigilforge for all credential resolution:

```rust
// In Scryforge provider configuration
pub struct ProviderConfig {
    pub name: String,
    pub token_ref: String,  // e.g., "auth://gmail/personal/token"
    // ...
}

// During provider initialization
async fn init_provider(config: &ProviderConfig) -> Result<Provider, Error> {
    let resolver = sigilforge_core::DefaultResolver::new()?;
    let token = resolver.resolve(&config.token_ref).await?;

    // Use token to initialize provider client
    // ...
}
```

This allows Scryforge configs to reference credentials without embedding them:

```toml
# scryforge.toml
[[providers]]
name = "gmail"
token = "auth://gmail/personal/token"

[[providers]]
name = "spotify"
token = "auth://spotify/main/token"

[[providers]]
name = "custom_api"
api_key = "auth://custom/default/api_key"
```
