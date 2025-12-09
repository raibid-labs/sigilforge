//! Sigilforge Client Library
//!
//! A lightweight Rust client for interacting with the Sigilforge authentication daemon.
//! Provides async methods for requesting tokens and secrets, with fallback support for
//! when the daemon is unavailable (development, CI, etc.).
//!
//! # Overview
//!
//! Sigilforge is a credential management daemon that handles OAuth token refresh,
//! API key storage, and secure credential resolution. This client library provides
//! a simple interface for applications to obtain credentials without managing
//! the complexity of token lifecycle.
//!
//! # Features
//!
//! - **Daemon Connection**: Connect to the Sigilforge daemon via Unix socket (or named pipe on Windows)
//! - **Fallback Support**: Fall back to environment variables or config files when daemon unavailable
//! - **auth:// URI Resolution**: Parse and resolve `auth://service/account/type` references
//! - **Token Lifecycle**: Automatic token refresh through the daemon
//!
//! # Quick Start
//!
//! ```no_run
//! use sigilforge_client::{SigilforgeClient, TokenProvider};
//!
//! #[tokio::main]
//! async fn main() -> sigilforge_client::Result<()> {
//!     // Create client with default configuration
//!     let client = SigilforgeClient::new();
//!
//!     // Get a token (tries daemon first, then fallbacks)
//!     let token = client.get_token("spotify", "personal").await?;
//!     println!("Authorization: {}", token.authorization_header());
//!
//!     // Resolve an auth:// URI
//!     let api_key = client.resolve("auth://openai/default/api_key").await?;
//!     println!("API Key: {}", api_key.value);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Fallback Configuration
//!
//! When the Sigilforge daemon isn't running, the client can fall back to other
//! credential sources:
//!
//! ```
//! use sigilforge_client::{SigilforgeClient, FallbackConfig};
//!
//! // Use only environment variables
//! let client = SigilforgeClient::fallback_only(FallbackConfig::env_vars());
//!
//! // Custom environment variable prefix
//! let client = SigilforgeClient::fallback_only(
//!     FallbackConfig::env_vars_with_prefix("MYAPP")
//! );
//! ```
//!
//! ## Environment Variable Format
//!
//! Environment variables follow the pattern: `{PREFIX}_{SERVICE}_{ACCOUNT}_{TYPE}`
//!
//! Examples:
//! - `SIGILFORGE_SPOTIFY_PERSONAL_TOKEN`
//! - `SIGILFORGE_GITHUB_OSS_API_KEY`
//! - `SIGILFORGE_OPENAI_DEFAULT_API_KEY`
//!
//! # auth:// URI Format
//!
//! The `auth://` URI scheme provides a standard way to reference credentials:
//!
//! ```text
//! auth://{service}/{account}/{credential_type}
//! ```
//!
//! Supported credential types:
//! - `token` - OAuth access token
//! - `refresh_token` - OAuth refresh token
//! - `api_key` - Static API key
//! - `client_id` - OAuth client ID
//! - `client_secret` - OAuth client secret
//!
//! Examples:
//! - `auth://spotify/personal/token`
//! - `auth://github/oss/api_key`
//! - `auth://gmail/work/refresh_token`
//!
//! # Feature Flags
//!
//! - `fallback-env` (default): Enable environment variable fallback
//! - `fallback-config` (default): Enable TOML config file fallback
//! - `fusabi-host-functions`: Enable Fusabi host function integration

mod client;
pub mod fallback;
pub mod resolve;
pub mod socket;
pub mod types;

// Re-export main types from client module
pub use client::{SigilforgeClient, SigilforgeClientBuilder, TokenProvider};

// Re-export from other modules
pub use fallback::{FallbackConfig, FallbackResolver};
pub use resolve::{is_auth_uri, AuthRef};
pub use socket::{default_socket_path, DaemonConnection};
pub use types::{AccessToken, CredentialType, DaemonHealth, Result, SecretValue, SigilforgeError};

// Note: Fusabi host function integration is provided through fusabi-stdlib-ext.
// To use Sigilforge from Fusabi scripts, enable the "sigilforge" feature in
// fusabi-stdlib-ext, which provides the sigilforge module with get_token,
// ensure_token, resolve, and is_available functions.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_api_accessible() {
        // Verify all public types are accessible
        let _: AccessToken = AccessToken::bearer("test");
        let _: CredentialType = CredentialType::Token;
        let _: FallbackConfig = FallbackConfig::env_vars();
    }

    #[test]
    fn test_auth_ref_parsing() {
        let auth_ref = AuthRef::parse("auth://spotify/personal/token").unwrap();
        assert_eq!(auth_ref.service, "spotify");
        assert_eq!(auth_ref.account, "personal");
        assert_eq!(auth_ref.credential_type, CredentialType::Token);
    }

    #[tokio::test]
    async fn test_client_with_env_fallback() {
        // SAFETY: Test-only env var manipulation, no concurrent access
        unsafe { std::env::set_var("SIGILFORGE_TEST_INTEGRATION_TOKEN", "integration-test-token") };

        let client = SigilforgeClient::fallback_only(FallbackConfig::env_vars());
        let token = client.get_token("test", "integration").await.unwrap();

        assert_eq!(token.token, "integration-test-token");

        // SAFETY: Test-only env var manipulation
        unsafe { std::env::remove_var("SIGILFORGE_TEST_INTEGRATION_TOKEN") };
    }
}
