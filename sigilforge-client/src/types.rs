use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// An OAuth access token with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    /// The token value.
    pub token: String,
    /// Token type (usually "Bearer").
    pub token_type: String,
    /// When the token expires (if known).
    pub expires_at: Option<DateTime<Utc>>,
}

impl AccessToken {
    /// Create a new access token.
    pub fn new(token: impl Into<String>, token_type: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            token_type: token_type.into(),
            expires_at: None,
        }
    }

    /// Create a Bearer token.
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::new(token, "Bearer")
    }

    /// Set the expiration time.
    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Check if the token is expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| exp < Utc::now())
            .unwrap_or(false)
    }

    /// Check if the token expires within the given duration.
    pub fn expires_within(&self, duration: chrono::Duration) -> bool {
        self.expires_at
            .map(|exp| exp < Utc::now() + duration)
            .unwrap_or(false)
    }

    /// Get the Authorization header value.
    pub fn authorization_header(&self) -> String {
        format!("{} {}", self.token_type, self.token)
    }
}

/// A resolved secret value from Sigilforge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretValue {
    /// The secret value.
    pub value: String,
    /// Optional metadata about the secret.
    pub metadata: Option<serde_json::Value>,
}

impl SecretValue {
    /// Create a new secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            metadata: None,
        }
    }

    /// Set metadata for the secret.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Errors that can occur when interacting with Sigilforge.
#[derive(Debug, thiserror::Error)]
pub enum SigilforgeError {
    /// The Sigilforge daemon is not available.
    #[error("daemon not available: {0}")]
    DaemonUnavailable(String),

    /// The requested account was not found.
    #[error("account not found: {service}/{account}")]
    AccountNotFound { service: String, account: String },

    /// Authentication is required for the account.
    #[error("authentication required for {service}/{account}")]
    AuthRequired { service: String, account: String },

    /// Token refresh failed.
    #[error("token expired and refresh failed: {0}")]
    RefreshFailed(String),

    /// Invalid auth:// reference format.
    #[error("invalid reference: {0}")]
    InvalidReference(String),

    /// Network or I/O error.
    #[error("network error: {0}")]
    NetworkError(String),

    /// No fallback configured for the service/account.
    #[error("fallback not configured for {service}/{account}")]
    NoFallback { service: String, account: String },

    /// Configuration error.
    #[error("configuration error: {0}")]
    ConfigError(String),

    /// JSON-RPC error from daemon.
    #[error("daemon error: {message} (code: {code})")]
    DaemonError { code: i32, message: String },

    /// Timeout waiting for daemon response.
    #[error("daemon request timed out")]
    Timeout,

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for Sigilforge operations.
pub type Result<T> = std::result::Result<T, SigilforgeError>;

/// Type of credential being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    /// OAuth access token (short-lived).
    Token,
    /// OAuth refresh token (long-lived).
    RefreshToken,
    /// Static API key.
    ApiKey,
    /// OAuth client ID.
    ClientId,
    /// OAuth client secret.
    ClientSecret,
}

impl CredentialType {
    /// Convert to environment variable suffix.
    pub fn env_suffix(&self) -> &'static str {
        match self {
            Self::Token => "TOKEN",
            Self::RefreshToken => "REFRESH_TOKEN",
            Self::ApiKey => "API_KEY",
            Self::ClientId => "CLIENT_ID",
            Self::ClientSecret => "CLIENT_SECRET",
        }
    }
}

impl FromStr for CredentialType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "token" | "access_token" => Ok(Self::Token),
            "refresh_token" => Ok(Self::RefreshToken),
            "api_key" | "apikey" => Ok(Self::ApiKey),
            "client_id" | "clientid" => Ok(Self::ClientId),
            "client_secret" | "clientsecret" => Ok(Self::ClientSecret),
            _ => Err(format!("unknown credential type: {}", s)),
        }
    }
}

impl fmt::Display for CredentialType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Token => write!(f, "token"),
            Self::RefreshToken => write!(f, "refresh_token"),
            Self::ApiKey => write!(f, "api_key"),
            Self::ClientId => write!(f, "client_id"),
            Self::ClientSecret => write!(f, "client_secret"),
        }
    }
}

/// Health status of the Sigilforge daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHealth {
    /// Whether the daemon is running.
    pub running: bool,
    /// Daemon version (if available).
    pub version: Option<String>,
    /// Number of configured accounts.
    pub account_count: Option<u32>,
}
