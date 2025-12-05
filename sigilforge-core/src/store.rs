//! Secret storage abstraction.
//!
//! This module provides:
//! - [`Secret`] - A wrapper for sensitive values that prevents accidental logging
//! - [`SecretStore`] - Trait for secret storage backends
//! - [`MemoryStore`] - In-memory implementation for testing
//!
//! # Storage Key Convention
//!
//! Keys follow the pattern: `sigilforge/{service}/{account}/{credential_type}`
//!
//! # Example
//!
//! ```rust,ignore
//! use sigilforge_core::store::{Secret, SecretStore, MemoryStore};
//!
//! let store = MemoryStore::new();
//!
//! let secret = Secret::new("super-secret-token");
//! store.set("sigilforge/spotify/personal/access_token", &secret).await.unwrap();
//!
//! let retrieved = store.get("sigilforge/spotify/personal/access_token").await.unwrap();
//! assert_eq!(retrieved.unwrap().expose(), "super-secret-token");
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use thiserror::Error;

/// A secret value that prevents accidental exposure in logs.
///
/// The inner value is only accessible via [`expose()`](Secret::expose).
/// Debug and Display implementations show `[REDACTED]` instead of the value.
#[derive(Clone, Serialize, Deserialize)]
pub struct Secret(String);

impl Secret {
    /// Create a new secret from a string value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Expose the secret value.
    ///
    /// Use sparingly and never log the result.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Consume the secret and return the inner value.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret([REDACTED])")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Secret {}

/// Error type for secret store operations.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The requested secret was not found.
    #[error("secret not found: {key}")]
    NotFound { key: String },

    /// Access to the secret was denied.
    #[error("access denied to secret: {key}")]
    AccessDenied { key: String },

    /// The storage backend encountered an error.
    #[error("backend error: {message}")]
    BackendError { message: String },

    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// The keyring backend is not available.
    #[error("keyring not available: {message}")]
    KeyringUnavailable { message: String },
}

/// Abstraction over secret storage backends.
///
/// Implementations include:
/// - [`MemoryStore`] - In-memory storage for testing
/// - `KeyringStore` (with `keyring-store` feature) - OS keyring
/// - `EncryptedFileStore` (future) - ROPS/SOPS encrypted files
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Retrieve a secret by key.
    ///
    /// Returns `Ok(None)` if the key doesn't exist.
    async fn get(&self, key: &str) -> Result<Option<Secret>, StoreError>;

    /// Store a secret at the given key.
    ///
    /// Overwrites any existing value.
    async fn set(&self, key: &str, secret: &Secret) -> Result<(), StoreError>;

    /// Delete a secret by key.
    ///
    /// Returns `Ok(())` even if the key didn't exist.
    async fn delete(&self, key: &str) -> Result<(), StoreError>;

    /// List all keys matching a prefix.
    ///
    /// Returns an empty vec if no keys match.
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError>;

    /// Check if a key exists without retrieving the value.
    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        Ok(self.get(key).await?.is_some())
    }
}

/// In-memory secret store for testing and development.
///
/// This store is not persistent; data is lost when the process exits.
///
/// # Thread Safety
///
/// This implementation uses interior mutability via `RwLock` and is
/// safe to share across threads.
pub struct MemoryStore {
    data: RwLock<HashMap<String, Secret>>,
}

impl MemoryStore {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Create a memory store with initial data.
    pub fn with_data(data: HashMap<String, Secret>) -> Self {
        Self {
            data: RwLock::new(data),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.data.read().map(|d| d.len()).unwrap_or(0);
        f.debug_struct("MemoryStore")
            .field("keys_count", &count)
            .finish()
    }
}

#[async_trait]
impl SecretStore for MemoryStore {
    async fn get(&self, key: &str) -> Result<Option<Secret>, StoreError> {
        let data = self.data.read().map_err(|e| StoreError::BackendError {
            message: format!("lock poisoned: {}", e),
        })?;
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, secret: &Secret) -> Result<(), StoreError> {
        let mut data = self.data.write().map_err(|e| StoreError::BackendError {
            message: format!("lock poisoned: {}", e),
        })?;
        data.insert(key.to_string(), secret.clone());
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        let mut data = self.data.write().map_err(|e| StoreError::BackendError {
            message: format!("lock poisoned: {}", e),
        })?;
        data.remove(key);
        Ok(())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let data = self.data.read().map_err(|e| StoreError::BackendError {
            message: format!("lock poisoned: {}", e),
        })?;
        let keys: Vec<String> = data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        Ok(keys)
    }
}

// TODO: Implement KeyringStore with the `keyring-store` feature
// TODO: Implement EncryptedFileStore for ROPS/SOPS support

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store_set_get() {
        let store = MemoryStore::new();
        let secret = Secret::new("test-value");

        store.set("test-key", &secret).await.unwrap();
        let retrieved = store.get("test-key").await.unwrap();

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().expose(), "test-value");
    }

    #[tokio::test]
    async fn test_memory_store_get_nonexistent() {
        let store = MemoryStore::new();
        let result = store.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_store_delete() {
        let store = MemoryStore::new();
        let secret = Secret::new("test-value");

        store.set("test-key", &secret).await.unwrap();
        store.delete("test-key").await.unwrap();

        let result = store.get("test-key").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_store_list_keys() {
        let store = MemoryStore::new();

        store
            .set("sigilforge/spotify/personal/token", &Secret::new("t1"))
            .await
            .unwrap();
        store
            .set("sigilforge/spotify/work/token", &Secret::new("t2"))
            .await
            .unwrap();
        store
            .set("sigilforge/github/main/token", &Secret::new("t3"))
            .await
            .unwrap();

        let spotify_keys = store.list_keys("sigilforge/spotify").await.unwrap();
        assert_eq!(spotify_keys.len(), 2);

        let all_keys = store.list_keys("sigilforge").await.unwrap();
        assert_eq!(all_keys.len(), 3);
    }

    #[tokio::test]
    async fn test_memory_store_exists() {
        let store = MemoryStore::new();
        let secret = Secret::new("test-value");

        assert!(!store.exists("test-key").await.unwrap());

        store.set("test-key", &secret).await.unwrap();

        assert!(store.exists("test-key").await.unwrap());
    }

    #[test]
    fn test_secret_debug_redacted() {
        let secret = Secret::new("super-secret");
        let debug = format!("{:?}", secret);
        assert!(!debug.contains("super-secret"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn test_secret_display_redacted() {
        let secret = Secret::new("super-secret");
        let display = format!("{}", secret);
        assert!(!display.contains("super-secret"));
        assert!(display.contains("REDACTED"));
    }
}
