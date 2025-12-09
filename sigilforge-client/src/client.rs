use crate::fallback::{FallbackConfig, FallbackResolver};
use crate::socket::{default_socket_path, DaemonConnection};
use crate::types::{AccessToken, DaemonHealth, Result, SecretValue, SigilforgeError};
use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Trait for obtaining tokens and credentials.
///
/// This trait is implemented by `SigilforgeClient` and can be mocked for testing.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    /// Get a valid access token for the given service and account.
    ///
    /// This will first try the daemon, then fall back to configured fallback
    /// strategies if the daemon is unavailable.
    async fn get_token(&self, service: &str, account: &str) -> Result<AccessToken>;

    /// Get a token, refreshing if necessary.
    ///
    /// Same as `get_token`, but will attempt to refresh an expired token
    /// if a refresh token is available.
    async fn ensure_token(&self, service: &str, account: &str) -> Result<AccessToken>;

    /// Resolve an auth:// reference to its secret value.
    ///
    /// Examples:
    /// - `auth://spotify/personal/token`
    /// - `auth://github/oss/api_key`
    async fn resolve(&self, reference: &str) -> Result<SecretValue>;
}

/// Client for interacting with the Sigilforge authentication daemon.
///
/// The client will first attempt to connect to the daemon. If unavailable,
/// it falls back to configured fallback strategies (environment variables,
/// config files, etc.).
///
/// # Example
///
/// ```no_run
/// use sigilforge_client::SigilforgeClient;
/// use sigilforge_client::TokenProvider;
///
/// #[tokio::main]
/// async fn main() -> sigilforge_client::Result<()> {
///     let client = SigilforgeClient::new();
///
///     // Get a token (tries daemon first, then fallbacks)
///     let token = client.get_token("spotify", "personal").await?;
///     println!("Got token: {}", token.token);
///
///     // Resolve an auth:// URI
///     let api_key = client.resolve("auth://openai/default/api_key").await?;
///     println!("API key: {}", api_key.value);
///
///     Ok(())
/// }
/// ```
pub struct SigilforgeClient {
    daemon: Option<DaemonConnection>,
    fallback: FallbackResolver,
    prefer_daemon: bool,
}

impl SigilforgeClient {
    /// Create a new client with auto-detected socket path and default fallbacks.
    pub fn new() -> Self {
        let daemon = default_socket_path().map(DaemonConnection::new);
        let fallback = FallbackResolver::new(FallbackConfig::default());

        Self {
            daemon,
            fallback,
            prefer_daemon: true,
        }
    }

    /// Create a client with an explicit socket path.
    pub fn with_socket(path: impl Into<PathBuf>) -> Self {
        let daemon = Some(DaemonConnection::new(path.into()));
        let fallback = FallbackResolver::new(FallbackConfig::default());

        Self {
            daemon,
            fallback,
            prefer_daemon: true,
        }
    }

    /// Create a client that only uses fallbacks (no daemon connection).
    pub fn fallback_only(config: FallbackConfig) -> Self {
        Self {
            daemon: None,
            fallback: FallbackResolver::new(config),
            prefer_daemon: false,
        }
    }

    /// Configure the fallback strategy.
    pub fn with_fallback(mut self, config: FallbackConfig) -> Self {
        self.fallback = FallbackResolver::new(config);
        self
    }

    /// Set the daemon connection timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        if let Some(daemon) = self.daemon.take() {
            self.daemon = Some(daemon.with_timeout(timeout));
        }
        self
    }

    /// Disable fallback to daemon (only use fallback strategies).
    pub fn without_daemon(mut self) -> Self {
        self.prefer_daemon = false;
        self
    }

    /// Check if the daemon is available and healthy.
    pub async fn health_check(&self) -> Result<DaemonHealth> {
        match &self.daemon {
            Some(daemon) => daemon.health_check().await,
            None => Err(SigilforgeError::DaemonUnavailable(
                "no daemon configured".to_string(),
            )),
        }
    }

    /// Check if the daemon is currently reachable.
    pub async fn is_daemon_available(&self) -> bool {
        match &self.daemon {
            Some(daemon) => daemon.is_available().await,
            None => false,
        }
    }

    /// Try to get a token from the daemon.
    async fn try_daemon_token(&self, service: &str, account: &str) -> Option<Result<AccessToken>> {
        if !self.prefer_daemon {
            return None;
        }

        let daemon = self.daemon.as_ref()?;

        match daemon.get_token(service, account).await {
            Ok(token) => {
                debug!("got token from daemon for {}/{}", service, account);
                Some(Ok(token))
            }
            Err(SigilforgeError::DaemonUnavailable(msg)) => {
                debug!("daemon unavailable: {}", msg);
                None
            }
            Err(SigilforgeError::Timeout) => {
                warn!("daemon request timed out");
                None
            }
            Err(e) => {
                // Other errors should be returned (account not found, etc.)
                Some(Err(e))
            }
        }
    }

    /// Try to ensure a token from the daemon.
    async fn try_daemon_ensure_token(
        &self,
        service: &str,
        account: &str,
    ) -> Option<Result<AccessToken>> {
        if !self.prefer_daemon {
            return None;
        }

        let daemon = self.daemon.as_ref()?;

        match daemon.ensure_token(service, account).await {
            Ok(token) => {
                debug!("ensured token from daemon for {}/{}", service, account);
                Some(Ok(token))
            }
            Err(SigilforgeError::DaemonUnavailable(msg)) => {
                debug!("daemon unavailable: {}", msg);
                None
            }
            Err(SigilforgeError::Timeout) => {
                warn!("daemon request timed out");
                None
            }
            Err(e) => Some(Err(e)),
        }
    }

    /// Try to resolve a reference from the daemon.
    async fn try_daemon_resolve(&self, reference: &str) -> Option<Result<SecretValue>> {
        if !self.prefer_daemon {
            return None;
        }

        let daemon = self.daemon.as_ref()?;

        match daemon.resolve(reference).await {
            Ok(value) => {
                debug!("resolved {} from daemon", reference);
                Some(Ok(value))
            }
            Err(SigilforgeError::DaemonUnavailable(msg)) => {
                debug!("daemon unavailable: {}", msg);
                None
            }
            Err(SigilforgeError::Timeout) => {
                warn!("daemon request timed out");
                None
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl Default for SigilforgeClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TokenProvider for SigilforgeClient {
    async fn get_token(&self, service: &str, account: &str) -> Result<AccessToken> {
        // Try daemon first
        if let Some(result) = self.try_daemon_token(service, account).await {
            return result;
        }

        // Fall back to configured strategies
        info!(
            "using fallback for token {}/{}",
            service, account
        );
        self.fallback.get_token(service, account).await
    }

    async fn ensure_token(&self, service: &str, account: &str) -> Result<AccessToken> {
        // Try daemon first (with refresh)
        if let Some(result) = self.try_daemon_ensure_token(service, account).await {
            return result;
        }

        // Fall back (can't refresh from fallback, just get token)
        info!(
            "using fallback for token {}/{}",
            service, account
        );
        self.fallback.get_token(service, account).await
    }

    async fn resolve(&self, reference: &str) -> Result<SecretValue> {
        // Try daemon first
        if let Some(result) = self.try_daemon_resolve(reference).await {
            return result;
        }

        // Fall back to configured strategies
        info!("using fallback for {}", reference);
        self.fallback.resolve(reference).await
    }
}

/// Builder for creating a `SigilforgeClient` with custom configuration.
pub struct SigilforgeClientBuilder {
    socket_path: Option<PathBuf>,
    fallback: FallbackConfig,
    timeout: Duration,
    use_daemon: bool,
}

impl SigilforgeClientBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            socket_path: default_socket_path(),
            fallback: FallbackConfig::default(),
            timeout: Duration::from_secs(5),
            use_daemon: true,
        }
    }

    /// Set the socket path.
    pub fn socket_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_path = Some(path.into());
        self
    }

    /// Disable daemon connection.
    pub fn no_daemon(mut self) -> Self {
        self.use_daemon = false;
        self
    }

    /// Set the fallback configuration.
    pub fn fallback(mut self, config: FallbackConfig) -> Self {
        self.fallback = config;
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the client.
    pub fn build(self) -> SigilforgeClient {
        let daemon = if self.use_daemon {
            self.socket_path
                .map(|p| DaemonConnection::new(p).with_timeout(self.timeout))
        } else {
            None
        };

        SigilforgeClient {
            daemon,
            fallback: FallbackResolver::new(self.fallback),
            prefer_daemon: self.use_daemon,
        }
    }
}

impl Default for SigilforgeClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fallback_only_client() {
        // SAFETY: Test-only env var manipulation, no concurrent access
        unsafe { std::env::set_var("SIGILFORGE_TEST_FALLBACK_TOKEN", "fallback-token") };

        let client = SigilforgeClient::fallback_only(FallbackConfig::env_vars());
        let token = client.get_token("test", "fallback").await.unwrap();

        assert_eq!(token.token, "fallback-token");
        assert_eq!(token.token_type, "Bearer");

        // SAFETY: Test-only env var manipulation
        unsafe { std::env::remove_var("SIGILFORGE_TEST_FALLBACK_TOKEN") };
    }

    #[tokio::test]
    async fn test_resolve_with_fallback() {
        // SAFETY: Test-only env var manipulation, no concurrent access
        unsafe { std::env::set_var("SIGILFORGE_CLIENTTEST_RESOLVE_API_KEY", "sk-test-123") };

        let client = SigilforgeClient::fallback_only(FallbackConfig::env_vars());
        let result = client.resolve("auth://clienttest/resolve/api_key").await.unwrap();

        assert_eq!(result.value, "sk-test-123");

        // SAFETY: Test-only env var manipulation
        unsafe { std::env::remove_var("SIGILFORGE_CLIENTTEST_RESOLVE_API_KEY") };
    }

    #[tokio::test]
    async fn test_builder() {
        let client = SigilforgeClientBuilder::new()
            .no_daemon()
            .fallback(FallbackConfig::env_vars())
            .timeout(Duration::from_secs(10))
            .build();

        assert!(!client.is_daemon_available().await);
    }

    #[tokio::test]
    async fn test_default_client_creation() {
        let client = SigilforgeClient::new();
        // Just verify it creates without panicking
        // Daemon likely won't be running in tests
        let _ = client.is_daemon_available().await;
    }
}
