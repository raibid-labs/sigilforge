use crate::resolve::AuthRef;
use crate::types::{AccessToken, CredentialType, Result, SecretValue, SigilforgeError};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, trace};

/// Configuration for fallback behavior when daemon is unavailable.
#[derive(Debug, Clone)]
pub enum FallbackConfig {
    /// No fallback - fail if daemon unavailable.
    None,

    /// Read from environment variables.
    ///
    /// Format: `{prefix}_{SERVICE}_{ACCOUNT}_{TYPE}`
    /// Example: `SIGILFORGE_SPOTIFY_PERSONAL_TOKEN`
    EnvVars {
        /// Prefix for environment variables (default: "SIGILFORGE").
        prefix: String,
    },

    /// Read from a TOML config file.
    #[cfg(feature = "fallback-config")]
    ConfigFile {
        /// Path to the config file.
        path: PathBuf,
    },

    /// Chain multiple fallback strategies.
    ///
    /// Tries each in order until one succeeds.
    Chain(Vec<FallbackConfig>),
}

impl Default for FallbackConfig {
    fn default() -> Self {
        // Default chain: try env vars first, then config file
        let mut chain = vec![
            FallbackConfig::EnvVars {
                prefix: "SIGILFORGE".to_string(),
            },
        ];

        #[cfg(feature = "fallback-config")]
        {
            if let Some(config_path) = default_config_path() {
                chain.push(FallbackConfig::ConfigFile { path: config_path });
            }
        }

        FallbackConfig::Chain(chain)
    }
}

impl FallbackConfig {
    /// Create an env vars fallback with default prefix.
    pub fn env_vars() -> Self {
        Self::EnvVars {
            prefix: "SIGILFORGE".to_string(),
        }
    }

    /// Create an env vars fallback with custom prefix.
    pub fn env_vars_with_prefix(prefix: impl Into<String>) -> Self {
        Self::EnvVars {
            prefix: prefix.into(),
        }
    }

    /// Create a config file fallback.
    #[cfg(feature = "fallback-config")]
    pub fn config_file(path: impl Into<PathBuf>) -> Self {
        Self::ConfigFile { path: path.into() }
    }

    /// Chain multiple fallback strategies.
    pub fn chain(strategies: Vec<FallbackConfig>) -> Self {
        Self::Chain(strategies)
    }
}

/// Get the default config file path.
#[cfg(feature = "fallback-config")]
fn default_config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "sigilforge")
        .map(|dirs| dirs.config_dir().join("credentials.toml"))
}

/// Fallback resolver for when daemon is unavailable.
pub struct FallbackResolver {
    config: FallbackConfig,
}

impl FallbackResolver {
    /// Create a new fallback resolver.
    pub fn new(config: FallbackConfig) -> Self {
        Self { config }
    }

    /// Try to resolve a token using fallback strategies.
    pub async fn get_token(
        &self,
        service: &str,
        account: &str,
    ) -> Result<AccessToken> {
        let auth_ref = AuthRef::new(service, account, CredentialType::Token);
        let value = self.resolve_ref(&auth_ref).await?;
        Ok(AccessToken::bearer(value.value))
    }

    /// Try to resolve a credential using fallback strategies.
    pub async fn resolve(&self, reference: &str) -> Result<SecretValue> {
        let auth_ref = AuthRef::parse(reference)?;
        self.resolve_ref(&auth_ref).await
    }

    /// Resolve an AuthRef using configured fallback strategies.
    pub async fn resolve_ref(&self, auth_ref: &AuthRef) -> Result<SecretValue> {
        self.resolve_with_config(&self.config, auth_ref).await
    }

    fn resolve_with_config<'a>(
        &'a self,
        config: &'a FallbackConfig,
        auth_ref: &'a AuthRef,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<SecretValue>> + Send + 'a>> {
        Box::pin(async move {
            match config {
                FallbackConfig::None => {
                    Err(SigilforgeError::NoFallback {
                        service: auth_ref.service.clone(),
                        account: auth_ref.account.clone(),
                    })
                }

                FallbackConfig::EnvVars { prefix } => {
                    self.resolve_from_env(prefix, auth_ref)
                }

                #[cfg(feature = "fallback-config")]
                FallbackConfig::ConfigFile { path } => {
                    self.resolve_from_config_file(path, auth_ref).await
                }

                FallbackConfig::Chain(strategies) => {
                    for strategy in strategies {
                        match self.resolve_with_config(strategy, auth_ref).await {
                            Ok(value) => return Ok(value),
                            Err(e) => {
                                trace!("fallback strategy failed: {}", e);
                                continue;
                            }
                        }
                    }
                    Err(SigilforgeError::NoFallback {
                        service: auth_ref.service.clone(),
                        account: auth_ref.account.clone(),
                    })
                }
            }
        })
    }

    fn resolve_from_env(&self, prefix: &str, auth_ref: &AuthRef) -> Result<SecretValue> {
        let env_var = format!(
            "{}_{}_{}_{}",
            prefix,
            auth_ref.service.to_uppercase(),
            auth_ref.account.to_uppercase(),
            auth_ref.credential_type.env_suffix()
        );

        debug!("looking for env var: {}", env_var);

        match std::env::var(&env_var) {
            Ok(value) => {
                debug!("found credential in env var {}", env_var);
                Ok(SecretValue::new(value))
            }
            Err(_) => {
                Err(SigilforgeError::NoFallback {
                    service: auth_ref.service.clone(),
                    account: auth_ref.account.clone(),
                })
            }
        }
    }

    #[cfg(feature = "fallback-config")]
    async fn resolve_from_config_file(
        &self,
        path: &PathBuf,
        auth_ref: &AuthRef,
    ) -> Result<SecretValue> {
        use tokio::fs;

        debug!("looking for credential in config file: {:?}", path);

        let content = fs::read_to_string(path).await.map_err(|e| {
            SigilforgeError::ConfigError(format!("failed to read config file: {}", e))
        })?;

        let config: CredentialsConfig = toml::from_str(&content).map_err(|e| {
            SigilforgeError::ConfigError(format!("failed to parse config file: {}", e))
        })?;

        let key = format!("{}.{}", auth_ref.service, auth_ref.account);
        let cred_type = auth_ref.credential_type.to_string();

        if let Some(service_config) = config.credentials.get(&auth_ref.service) {
            if let Some(account_config) = service_config.get(&auth_ref.account) {
                if let Some(value) = account_config.get(&cred_type) {
                    debug!("found credential in config file for {}", key);
                    return Ok(SecretValue::new(value.clone()));
                }
            }
        }

        Err(SigilforgeError::NoFallback {
            service: auth_ref.service.clone(),
            account: auth_ref.account.clone(),
        })
    }
}

/// TOML config file structure for credentials.
#[cfg(feature = "fallback-config")]
#[derive(Debug, serde::Deserialize)]
struct CredentialsConfig {
    /// Nested map: service -> account -> credential_type -> value
    #[serde(default)]
    credentials: HashMap<String, HashMap<String, HashMap<String, String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_env_var_fallback() {
        std::env::set_var("SIGILFORGE_TEST_DEV_TOKEN", "test-token-123");

        let resolver = FallbackResolver::new(FallbackConfig::env_vars());
        let token = resolver.get_token("test", "dev").await.unwrap();

        assert_eq!(token.token, "test-token-123");
        assert_eq!(token.token_type, "Bearer");

        std::env::remove_var("SIGILFORGE_TEST_DEV_TOKEN");
    }

    #[tokio::test]
    async fn test_env_var_api_key() {
        std::env::set_var("SIGILFORGE_OPENAI_DEFAULT_API_KEY", "sk-test-key");

        let resolver = FallbackResolver::new(FallbackConfig::env_vars());
        let result = resolver.resolve("auth://openai/default/api_key").await.unwrap();

        assert_eq!(result.value, "sk-test-key");

        std::env::remove_var("SIGILFORGE_OPENAI_DEFAULT_API_KEY");
    }

    #[tokio::test]
    async fn test_custom_prefix() {
        std::env::set_var("MYAPP_SPOTIFY_PERSONAL_TOKEN", "custom-token");

        let resolver = FallbackResolver::new(FallbackConfig::env_vars_with_prefix("MYAPP"));
        let token = resolver.get_token("spotify", "personal").await.unwrap();

        assert_eq!(token.token, "custom-token");

        std::env::remove_var("MYAPP_SPOTIFY_PERSONAL_TOKEN");
    }

    #[tokio::test]
    async fn test_fallback_none() {
        let resolver = FallbackResolver::new(FallbackConfig::None);
        let result = resolver.get_token("missing", "account").await;

        assert!(matches!(result, Err(SigilforgeError::NoFallback { .. })));
    }

    #[tokio::test]
    async fn test_chain_fallback() {
        // Set up second env var (first one won't exist)
        std::env::set_var("BACKUP_GITHUB_OSS_API_KEY", "backup-key");

        let resolver = FallbackResolver::new(FallbackConfig::chain(vec![
            FallbackConfig::env_vars_with_prefix("PRIMARY"),  // won't have the var
            FallbackConfig::env_vars_with_prefix("BACKUP"),   // has the var
        ]));

        let result = resolver.resolve("auth://github/oss/api_key").await.unwrap();
        assert_eq!(result.value, "backup-key");

        std::env::remove_var("BACKUP_GITHUB_OSS_API_KEY");
    }
}
