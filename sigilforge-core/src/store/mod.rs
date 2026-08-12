//! Secret storage abstraction.
//!
//! This module provides:
//! - [`Secret`] - A wrapper for sensitive values that prevents accidental logging
//! - [`SecretStore`] - Trait for secret storage backends
//! - [`MemoryStore`] - In-memory implementation for testing
//! - [`KeyringStore`] - OS keyring implementation (with `keyring-store` feature)
//! - [`EncryptedFileStore`] - age-encrypted file, for headless hosts
//!   (with `encrypted-file-store` feature)
//! - [`open_store`] - Select, **probe**, and return a backend, or fail loudly
//!
//! # Storage Key Convention
//!
//! Keys follow the pattern: `sigilforge/{service}/{account}/{credential_type}`
//!
//! # Picking a backend
//!
//! See [`StoreConfig`] for the precedence rules. The short version:
//!
//! | Situation | Backend |
//! |-----------|---------|
//! | `SIGILFORGE_STORE_BACKEND` or `[storage] backend` is set | exactly that, or an error |
//! | An age identity exists (`sigilforge store init` was run) | [`EncryptedFileStore`] |
//! | The OS keyring answers a round-trip probe | [`KeyringStore`] |
//! | Neither | [`StoreError::NoBackend`] - **never** a silent [`MemoryStore`] |
//!
//! That last row is the whole point. A store that quietly degrades to an empty
//! `HashMap` turns "the keyring is unreachable" into "you have no credentials",
//! which is indistinguishable from data loss and is exactly how a registered
//! GitHub App can appear to have never been registered.
//!
//! # Example
//!
//! ```
//! use sigilforge_core::store::{
//!     open_store_with, Secret, SecretStore, StoreBackend, StoreConfig,
//! };
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let store = open_store_with(&StoreConfig::for_backend(StoreBackend::Memory)).unwrap();
//!
//! let secret = Secret::new("super-secret-token");
//! store.set("sigilforge/spotify/personal/access_token", &secret).await.unwrap();
//!
//! let retrieved = store.get("sigilforge/spotify/personal/access_token").await.unwrap();
//! assert_eq!(retrieved.unwrap().expose(), "super-secret-token");
//! # });
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

mod config;
#[cfg(feature = "encrypted-file-store")]
mod encrypted_file;
#[cfg(feature = "keyring-store")]
mod keyring;
mod memory;

pub use config::{
    BACKEND_ENV_VAR, BackendSource, IDENTITY_ENV_VAR, SECRETS_ENV_VAR, StoreBackend, StoreConfig,
};
#[cfg(feature = "encrypted-file-store")]
pub use encrypted_file::{
    EncryptedFileStore, IDENTITY_FILE_NAME, InitOutcome, SECRETS_FILE_NAME, default_identity_path,
    default_secrets_path,
};
#[cfg(feature = "keyring-store")]
pub use keyring::KeyringStore;
pub use memory::MemoryStore;

/// A secret value that prevents accidental exposure in logs.
///
/// The inner value is only accessible via [`expose()`](Secret::expose).
/// Debug and Display implementations show `[REDACTED]` instead of the value.
/// Memory is automatically zeroed when the secret is dropped.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
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
    /// Note: The returned string is cloned before the Secret is zeroed on drop.
    pub fn into_inner(self) -> String {
        self.0.clone()
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

    /// A specific backend was selected but cannot be used right now.
    ///
    /// Raised by the `probe` methods and by [`open_store`]. Distinct from
    /// [`NotFound`](Self::NotFound): the credential may well exist, we just
    /// cannot reach the place it lives.
    #[error("storage backend '{backend}' is unavailable: {reason}")]
    BackendUnavailable { backend: String, reason: String },

    /// No storage backend is usable, and none was explicitly configured.
    ///
    /// Deliberately fatal. The alternative - quietly using [`MemoryStore`] - is
    /// what makes an unreachable store look like an empty one.
    #[error("no usable secret storage backend\n{details}")]
    NoBackend { details: String },

    /// A key file is readable by users other than its owner.
    #[error("{path} is readable by other users (mode {mode:04o}); run: chmod 600 {path}")]
    InsecurePermissions { path: String, mode: u32 },
}

/// Abstraction over secret storage backends.
///
/// Implementations include:
/// - [`MemoryStore`] - In-memory storage for testing
/// - [`KeyringStore`] (with `keyring-store` feature) - OS keyring
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

/// Blanket implementation of `SecretStore` for `Box<dyn SecretStore>`.
///
/// This allows using `Box<dyn SecretStore>` anywhere a `SecretStore` is expected,
/// enabling dynamic dispatch for secret storage backends.
#[async_trait]
impl SecretStore for Box<dyn SecretStore> {
    async fn get(&self, key: &str) -> Result<Option<Secret>, StoreError> {
        (**self).get(key).await
    }

    async fn set(&self, key: &str, secret: &Secret) -> Result<(), StoreError> {
        (**self).set(key, secret).await
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        (**self).delete(key).await
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        (**self).list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        (**self).exists(key).await
    }
}

/// The keyring service name Sigilforge registers its entries under.
pub const KEYRING_SERVICE_NAME: &str = "sigilforge";

/// The result of asking whether one backend works.
///
/// Returned by [`probe_backends`], which is what `sigilforge store status`
/// prints. `detail` is safe to show a user: it names paths and failures, never
/// secrets or key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendProbe {
    /// The backend that was probed.
    pub backend: StoreBackend,

    /// Whether it is usable right now.
    pub available: bool,

    /// Why, in one line.
    pub detail: String,
}

/// Open the secret store described by the environment and config file.
///
/// Equivalent to `open_store_with(&StoreConfig::load()?)`. This is what CLI
/// commands and the daemon should call.
///
/// # Errors
///
/// - [`StoreError::NoBackend`] if nothing is usable, listing what was tried and
///   why each failed
/// - [`StoreError::BackendUnavailable`] if a backend was explicitly requested
///   and does not work - it is *not* silently replaced with another one
/// - [`StoreError::BackendError`] if the configuration itself is malformed
///
/// # Example
///
/// ```rust,no_run
/// use sigilforge_core::store::open_store;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let store = open_store()?;
/// # drop(store);
/// # Ok(())
/// # }
/// ```
pub fn open_store() -> Result<Box<dyn SecretStore>, StoreError> {
    open_store_with(&StoreConfig::load()?)
}

/// Open the secret store described by an explicit [`StoreConfig`].
///
/// The returned store has already been probed: if this call succeeds, a write
/// followed by a read in a later process will work, as far as anything can tell
/// without doing it.
///
/// # Errors
///
/// See [`open_store`].
///
/// # Example
///
/// ```
/// use sigilforge_core::store::{open_store_with, StoreBackend, StoreConfig};
///
/// // `memory` is only ever used when asked for by name.
/// let store = open_store_with(&StoreConfig::for_backend(StoreBackend::Memory)).unwrap();
/// # drop(store);
/// ```
pub fn open_store_with(config: &StoreConfig) -> Result<Box<dyn SecretStore>, StoreError> {
    if let Some(backend) = config.backend {
        let store = open_backend(backend, config).map_err(|e| {
            tracing::error!(
                "secret storage backend '{}' was requested by {} but is unusable: {}",
                backend,
                config.source_description(),
                e
            );
            e
        })?;

        tracing::info!(
            "using the {} secret store (selected by {})",
            backend,
            config.source_description()
        );
        return Ok(store);
    }

    // Automatic selection. The encrypted file store is tried first because it
    // is deterministic: if an identity exists somebody set it up on purpose,
    // and unlike the keyring it does not depend on the ambient session.
    let mut failures = Vec::new();

    for backend in AUTO_BACKENDS {
        match open_backend(*backend, config) {
            Ok(store) => {
                tracing::info!("using the {} secret store (auto-selected)", backend);
                return Ok(store);
            }
            Err(e) => {
                tracing::debug!("secret storage backend '{}' is unusable: {}", backend, e);
                failures.push(format!("  - {}: {}", backend, one_line(&e.to_string())));
            }
        }
    }

    Err(StoreError::NoBackend {
        details: format!(
            "{}\n\nOn a headless host, run `sigilforge store init` to create an \
             age-encrypted store that needs no D-Bus session. To force a backend, set \
             {}=keyring|encrypted-file|memory.",
            failures.join("\n"),
            BACKEND_ENV_VAR
        ),
    })
}

/// Backends eligible for automatic selection, in order. `memory` is absent by
/// design: it must be asked for by name.
const AUTO_BACKENDS: &[StoreBackend] = &[StoreBackend::EncryptedFile, StoreBackend::Keyring];

/// Report on every backend, for `sigilforge store status` and diagnostics.
///
/// Probing the keyring writes and deletes a sentinel entry; probing the
/// encrypted file store decrypts it. Neither reveals a stored secret.
///
/// # Example
///
/// ```rust,no_run
/// use sigilforge_core::store::{probe_backends, StoreConfig};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// for probe in probe_backends(&StoreConfig::load()?) {
///     println!("{:<15} {}", probe.backend.to_string(), probe.detail);
/// }
/// # Ok(())
/// # }
/// ```
pub fn probe_backends(config: &StoreConfig) -> Vec<BackendProbe> {
    [
        StoreBackend::EncryptedFile,
        StoreBackend::Keyring,
        StoreBackend::Memory,
    ]
    .into_iter()
    .map(|backend| match open_backend(backend, config) {
        Ok(_) => BackendProbe {
            backend,
            available: true,
            detail: describe_available(backend, config),
        },
        Err(e) => BackendProbe {
            backend,
            available: false,
            detail: one_line(&e.to_string()),
        },
    })
    .collect()
}

/// Create the age-encrypted store at the configured (or default) paths.
///
/// This is the first-run step for a headless host: it generates an age identity
/// if there is not one already, writes it `0600`, and creates an empty encrypted
/// store. Idempotent - an existing identity is never regenerated, because doing
/// so would orphan every secret encrypted to the old one.
///
/// **The identity file is the only copy of the key.** Losing it loses every
/// secret in the store. [`InitOutcome::identity_path`] is what to back up.
///
/// # Errors
///
/// Returns [`StoreError::BackendError`] if the files cannot be written, or
/// [`StoreError::InsecurePermissions`] if an existing identity file is readable
/// by other users.
///
/// # Example
///
/// ```rust,no_run
/// use sigilforge_core::store::{init_encrypted_store, StoreConfig};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let outcome = init_encrypted_store(&StoreConfig::load()?)?;
/// println!("back up {} - it cannot be regenerated", outcome.identity_path.display());
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "encrypted-file-store")]
pub fn init_encrypted_store(config: &StoreConfig) -> Result<InitOutcome, StoreError> {
    EncryptedFileStore::initialize_at(
        &resolved_secrets_path(config)?,
        &resolved_identity_path(config)?,
    )
}

/// Construct one backend and prove it works. No fallbacks live here.
fn open_backend(
    backend: StoreBackend,
    config: &StoreConfig,
) -> Result<Box<dyn SecretStore>, StoreError> {
    match backend {
        StoreBackend::Memory => Ok(Box::new(MemoryStore::new())),

        #[cfg(feature = "encrypted-file-store")]
        StoreBackend::EncryptedFile => {
            let store = EncryptedFileStore::open_at(
                &resolved_secrets_path(config)?,
                &resolved_identity_path(config)?,
            )?;
            store.probe()?;
            Ok(Box::new(store))
        }

        #[cfg(not(feature = "encrypted-file-store"))]
        StoreBackend::EncryptedFile => {
            // `config` only carries paths this backend would have used.
            let _ = config;
            Err(StoreError::BackendUnavailable {
                backend: backend.to_string(),
                reason: "this build has the `encrypted-file-store` feature disabled".to_string(),
            })
        }

        #[cfg(feature = "keyring-store")]
        StoreBackend::Keyring => {
            let store = KeyringStore::try_new(KEYRING_SERVICE_NAME)?;
            store.probe()?;
            Ok(Box::new(store))
        }

        #[cfg(not(feature = "keyring-store"))]
        StoreBackend::Keyring => Err(StoreError::BackendUnavailable {
            backend: backend.to_string(),
            reason: "this build has the `keyring-store` feature disabled".to_string(),
        }),
    }
}

/// One line about a backend that works, for `store status`.
fn describe_available(backend: StoreBackend, config: &StoreConfig) -> String {
    match backend {
        StoreBackend::Memory => "usable, but process-local: nothing survives exit".to_string(),
        StoreBackend::Keyring => {
            format!("usable: round-tripped a probe entry under '{KEYRING_SERVICE_NAME}'")
        }
        #[cfg(feature = "encrypted-file-store")]
        StoreBackend::EncryptedFile => match resolved_secrets_path(config) {
            Ok(path) => format!("usable: {}", path.display()),
            Err(_) => "usable".to_string(),
        },
        #[cfg(not(feature = "encrypted-file-store"))]
        StoreBackend::EncryptedFile => {
            let _ = config;
            "usable".to_string()
        }
    }
}

#[cfg(feature = "encrypted-file-store")]
fn resolved_identity_path(config: &StoreConfig) -> Result<std::path::PathBuf, StoreError> {
    match &config.identity_file {
        Some(path) => Ok(path.clone()),
        None => default_identity_path(),
    }
}

#[cfg(feature = "encrypted-file-store")]
fn resolved_secrets_path(config: &StoreConfig) -> Result<std::path::PathBuf, StoreError> {
    match &config.secrets_file {
        Some(path) => Ok(path.clone()),
        None => default_secrets_path(),
    }
}

/// Flatten an error for a one-per-line summary.
fn one_line(message: &str) -> String {
    message
        .split('\n')
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Create a secret store, preferring persistent storage.
///
/// Retained for callers written against the older API. Prefer [`open_store`],
/// which takes the backend from configuration instead of a boolean.
///
/// - `prefer_persistent == true` delegates to [`open_store`]
/// - `prefer_persistent == false` returns a [`MemoryStore`]
///
/// # Compatibility note
///
/// This used to return `Box<dyn SecretStore>` infallibly, substituting a
/// [`MemoryStore`] when the keyring could not be constructed. It now returns a
/// `Result` precisely so that substitution cannot happen behind a caller's
/// back: reading from a store that silently became empty is worse than not
/// getting a store at all.
///
/// # Errors
///
/// See [`open_store`].
///
/// # Example
///
/// ```
/// use sigilforge_core::store::create_store;
///
/// let store = create_store(false).unwrap(); // explicit in-memory store
/// # drop(store);
/// ```
pub fn create_store(prefer_persistent: bool) -> Result<Box<dyn SecretStore>, StoreError> {
    if prefer_persistent {
        open_store()
    } else {
        Ok(Box::new(MemoryStore::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_secret_into_inner() {
        let secret = Secret::new("my-value");
        let inner = secret.into_inner();
        assert_eq!(inner, "my-value");
    }

    #[test]
    fn test_secret_equality() {
        let s1 = Secret::new("same");
        let s2 = Secret::new("same");
        let s3 = Secret::new("different");
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[tokio::test]
    async fn test_box_dyn_secret_store() {
        // Test that Box<dyn SecretStore> works correctly
        let store: Box<dyn SecretStore> = Box::new(MemoryStore::new());

        // Test set and get
        let secret = Secret::new("boxed-secret");
        store.set("boxed-key", &secret).await.unwrap();
        let retrieved = store.get("boxed-key").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().expose(), "boxed-secret");

        // Test exists
        assert!(store.exists("boxed-key").await.unwrap());
        assert!(!store.exists("nonexistent").await.unwrap());

        // Test list_keys
        store.set("prefix/key1", &secret).await.unwrap();
        store.set("prefix/key2", &secret).await.unwrap();
        let keys = store.list_keys("prefix/").await.unwrap();
        assert_eq!(keys.len(), 2);

        // Test delete
        store.delete("boxed-key").await.unwrap();
        assert!(!store.exists("boxed-key").await.unwrap());
    }

    #[tokio::test]
    async fn test_create_store_memory_fallback() {
        // `false` means "give me a memory store", explicitly.
        let store = create_store(false).unwrap();

        // Verify the store is usable by testing basic operations
        let secret = Secret::new("test");
        store.set("test-key", &secret).await.unwrap();
        let retrieved = store.get("test-key").await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_create_store_prefer_keyring() {
        // Depending on the machine this either finds a working persistent
        // backend or finds none. What it must never do is hand back a
        // MemoryStore while implying persistence - the old behaviour, which
        // made an unreachable keyring look like an empty one.
        let secret = Secret::new("test");
        let test_key = "test-key-prefer";

        match create_store(true) {
            Ok(store) => {
                // A store that opened has been probed, so a round-trip must work.
                store.set(test_key, &secret).await.unwrap();
                let retrieved = store.get(test_key).await.unwrap();
                assert_eq!(
                    retrieved.map(|s| s.expose().to_string()),
                    Some("test".to_string()),
                    "open_store returned a backend whose probe passed but which \
                     cannot round-trip a value"
                );
                store.delete(test_key).await.unwrap();
            }
            Err(e) => {
                // Fine on a headless box with no store initialised - but it has
                // to say so, and say what to do about it.
                assert!(
                    matches!(
                        e,
                        StoreError::NoBackend { .. } | StoreError::BackendUnavailable { .. }
                    ),
                    "unexpected error type: {:?}",
                    e
                );
                assert!(
                    e.to_string().contains("sigilforge store init"),
                    "the failure must be actionable, got: {}",
                    e
                );
            }
        }
    }

    #[tokio::test]
    async fn test_explicit_memory_backend_is_honoured() {
        let store = open_store_with(&StoreConfig::for_backend(StoreBackend::Memory)).unwrap();

        store.set("k", &Secret::new("v")).await.unwrap();
        assert_eq!(store.get("k").await.unwrap().unwrap().expose(), "v");
    }

    #[test]
    fn test_explicit_backend_never_falls_back_to_another() {
        // Point the encrypted file store at a directory with no identity in it.
        // The honest answer is an error naming that backend, not a MemoryStore
        // and not the keyring.
        let dir = tempfile::TempDir::new().unwrap();
        let config = StoreConfig {
            backend: Some(StoreBackend::EncryptedFile),
            source: Some(BackendSource::Environment),
            identity_file: Some(dir.path().join("absent.key")),
            secrets_file: Some(dir.path().join("absent.age")),
        };

        let Err(err) = open_store_with(&config) else {
            panic!("an unusable backend must not open");
        };
        assert!(
            matches!(err, StoreError::BackendUnavailable { .. }),
            "expected BackendUnavailable, got {:?}",
            err
        );
        assert!(err.to_string().contains("encrypted-file"), "{}", err);
    }

    #[cfg(feature = "encrypted-file-store")]
    #[tokio::test]
    async fn test_auto_selection_prefers_an_initialised_encrypted_store() {
        let dir = tempfile::TempDir::new().unwrap();
        let secrets = dir.path().join("secrets.age");
        let identity = dir.path().join("age-identity.key");
        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        let config = StoreConfig {
            backend: None,
            source: None,
            identity_file: Some(identity),
            secrets_file: Some(secrets.clone()),
        };

        let store = open_store_with(&config).unwrap();
        store.set("k", &Secret::new("v")).await.unwrap();

        // If auto-selection had picked the keyring or memory, the value would
        // not be sitting in this file.
        assert!(secrets.exists());
        assert_eq!(store.get("k").await.unwrap().unwrap().expose(), "v");
    }

    #[test]
    fn test_no_backend_error_names_every_candidate() {
        // A machine with neither an initialised file store nor a keyring.
        let dir = tempfile::TempDir::new().unwrap();
        let config = StoreConfig {
            backend: None,
            source: None,
            identity_file: Some(dir.path().join("absent.key")),
            secrets_file: Some(dir.path().join("absent.age")),
        };

        let Err(err) = open_store_with(&config) else {
            // This machine has a working keyring; nothing to assert.
            return;
        };

        let message = err.to_string();
        assert!(matches!(err, StoreError::NoBackend { .. }));
        assert!(message.contains("encrypted-file"), "{}", message);
        assert!(message.contains("keyring"), "{}", message);
        assert!(message.contains("sigilforge store init"), "{}", message);
        assert!(message.contains(BACKEND_ENV_VAR), "{}", message);
        // Memory is not a candidate, so it must not be offered as a consolation.
        assert!(!message.contains("- memory:"), "{}", message);
    }

    #[test]
    fn test_probe_backends_covers_all_three() {
        let dir = tempfile::TempDir::new().unwrap();
        let config = StoreConfig {
            identity_file: Some(dir.path().join("absent.key")),
            secrets_file: Some(dir.path().join("absent.age")),
            ..StoreConfig::default()
        };

        let probes = probe_backends(&config);
        assert_eq!(probes.len(), 3);

        let memory = probes
            .iter()
            .find(|p| p.backend == StoreBackend::Memory)
            .unwrap();
        assert!(memory.available);

        let file = probes
            .iter()
            .find(|p| p.backend == StoreBackend::EncryptedFile)
            .unwrap();
        assert!(!file.available);
        assert!(!file.detail.is_empty());
        // The detail is shown to users, so it must be a single tidy line.
        assert!(!file.detail.contains('\n'));
    }

    #[test]
    fn test_store_error_display() {
        let err = StoreError::NotFound {
            key: "test-key".to_string(),
        };
        assert!(err.to_string().contains("not found"));

        let err = StoreError::AccessDenied {
            key: "test-key".to_string(),
        };
        assert!(err.to_string().contains("denied"));

        let err = StoreError::BackendError {
            message: "connection failed".to_string(),
        };
        assert!(err.to_string().contains("connection failed"));

        let err = StoreError::KeyringUnavailable {
            message: "no keyring".to_string(),
        };
        assert!(err.to_string().contains("no keyring"));

        // An unavailable backend names itself, so the message says which one.
        let err = StoreError::BackendUnavailable {
            backend: "keyring".to_string(),
            reason: "no D-Bus session bus".to_string(),
        };
        assert!(err.to_string().contains("keyring"));
        assert!(err.to_string().contains("no D-Bus session bus"));

        let err = StoreError::InsecurePermissions {
            path: "/home/u/.config/sigilforge/age-identity.key".to_string(),
            mode: 0o644,
        };
        assert!(err.to_string().contains("0644"));
        assert!(err.to_string().contains("chmod 600"));
    }
}
