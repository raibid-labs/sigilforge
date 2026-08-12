//! Age-encrypted file secret storage, for hosts with no session bus.
//!
//! [`KeyringStore`](super::KeyringStore) needs a D-Bus session bus and an
//! unlocked Secret Service provider. A server reached over SSH has neither, so
//! every write fails and - worse, before this backend existed - every read came
//! back "not found". This store is the answer for that machine: it needs no
//! session bus, no desktop, no agent, and no prompt on read, so a daemon or a CI
//! job can use it unattended.
//!
//! # Shape on disk
//!
//! Two files, deliberately in two different directories:
//!
//! | File | Default location | Contents |
//! |------|------------------|----------|
//! | Identity | `~/.config/sigilforge/age-identity.key` | one age X25519 secret key, `0600` |
//! | Secrets | `~/.local/share/sigilforge/secrets.age` | the whole store, encrypted to that key's recipient, `0600` |
//!
//! The identity is the *only* thing that can decrypt the secrets file, and it
//! never sits next to it: back up the data directory without shipping the key,
//! or ship the key through a different channel. **Lose the identity file and
//! every secret in the store is gone** - there is no recovery, no escrow, and no
//! password reset. Back it up somewhere offline before you store anything you
//! cannot re-issue.
//!
//! # Why age
//!
//! The [`age`] crate is a pure-Rust implementation of the age format
//! (X25519 + ChaCha20-Poly1305, authenticated). It is a maintained library, not
//! a subprocess: shelling out to `sops`, `age`, or `gpg` would reintroduce the
//! exact failure this backend exists to remove, namely a dependency that is
//! missing on a minimal server. The file it writes is also readable by the
//! standard `age` and `rage` CLIs, so the data outlives this program:
//!
//! ```bash
//! age -d -i ~/.config/sigilforge/age-identity.key \
//!     ~/.local/share/sigilforge/secrets.age
//! ```
//!
//! # Concurrency
//!
//! Reads decrypt the file fresh every time, so a value written by one process is
//! visible to the next. Writes take an advisory lock file next to the store and
//! rewrite it atomically (write to a temporary file, `fsync`, rename), so a
//! crash mid-write cannot leave a truncated store.
//!
//! # Example
//!
//! ```
//! # #[cfg(feature = "encrypted-file-store")]
//! # {
//! use sigilforge_core::store::{EncryptedFileStore, Secret, SecretStore};
//!
//! let dir = tempfile::tempdir().unwrap();
//! let secrets = dir.path().join("secrets.age");
//! let identity = dir.path().join("age-identity.key");
//!
//! // One-time setup: generate the identity and an empty store.
//! EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();
//!
//! let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
//!
//! tokio::runtime::Runtime::new().unwrap().block_on(async {
//!     store
//!         .set("sigilforge/github-app/acme/private_key", &Secret::new("-----BEGIN..."))
//!         .await
//!         .unwrap();
//!
//!     // A separate handle - as a separate process would - sees the value.
//!     let reopened = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
//!     let found = reopened.get("sigilforge/github-app/acme/private_key").await.unwrap();
//!     assert_eq!(found.unwrap().expose(), "-----BEGIN...");
//! });
//! # }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use age::secrecy::ExposeSecret;
use age::x25519::{Identity, Recipient};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::{Secret, SecretStore, StoreError};

/// Default file name for the age identity, inside the config directory.
pub const IDENTITY_FILE_NAME: &str = "age-identity.key";

/// Default file name for the encrypted store, inside the data directory.
pub const SECRETS_FILE_NAME: &str = "secrets.age";

/// Format version written into the encrypted payload.
const STORE_FORMAT_VERSION: u32 = 1;

/// How long to wait for another process to finish writing before giving up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// A lock file older than this is assumed to belong to a process that died.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60);

/// How long to sleep between attempts to take the write lock.
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// An age-encrypted file backing a [`SecretStore`].
///
/// Construct with [`open`](Self::open) (default paths) or
/// [`open_at`](Self::open_at). Both require the identity file to already exist;
/// [`initialize`](Self::initialize) creates it.
///
/// # Example
///
/// ```rust,no_run
/// # #[cfg(feature = "encrypted-file-store")]
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use sigilforge_core::store::EncryptedFileStore;
///
/// // First run on a new machine.
/// let init = EncryptedFileStore::initialize()?;
/// println!("back up {}", init.identity_path.display());
///
/// // Every run after that.
/// let store = EncryptedFileStore::open()?;
/// store.probe()?;
/// # Ok(())
/// # }
/// # #[cfg(not(feature = "encrypted-file-store"))]
/// # fn main() {}
/// ```
pub struct EncryptedFileStore {
    secrets_path: PathBuf,
    identity_path: PathBuf,
    identity: Identity,
    recipient: Recipient,
}

/// What [`EncryptedFileStore::initialize`] did, so a caller can tell the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    /// Where the identity (private key) was written or found.
    pub identity_path: PathBuf,

    /// Where the encrypted store was written or found.
    pub secrets_path: PathBuf,

    /// The age recipient (public key) matching the identity. Safe to share.
    pub public_key: String,

    /// `true` if this call generated a new identity, `false` if one already existed.
    pub created_identity: bool,

    /// `true` if this call created an empty store file.
    pub created_store: bool,
}

impl EncryptedFileStore {
    /// Open the store at the platform default paths.
    ///
    /// # Errors
    ///
    /// - [`StoreError::BackendUnavailable`] if the identity file does not exist
    ///   (run [`initialize`](Self::initialize) first) or cannot be parsed
    /// - [`StoreError::InsecurePermissions`] if the identity file is readable by
    ///   anyone but its owner
    pub fn open() -> Result<Self, StoreError> {
        Self::open_at(&default_secrets_path()?, &default_identity_path()?)
    }

    /// Open the store at explicit paths.
    ///
    /// Neither path is created. The identity file must exist and be `0600`; the
    /// secrets file may be absent, which reads as an empty store.
    ///
    /// # Errors
    ///
    /// See [`open`](Self::open).
    pub fn open_at(secrets_path: &Path, identity_path: &Path) -> Result<Self, StoreError> {
        let identity = load_identity(identity_path)?;
        let recipient = identity.to_public();

        Ok(Self {
            secrets_path: secrets_path.to_path_buf(),
            identity_path: identity_path.to_path_buf(),
            identity,
            recipient,
        })
    }

    /// Create the identity and an empty store at the platform default paths.
    ///
    /// Idempotent: an existing identity is kept, never regenerated. Regenerating
    /// would orphan every secret already encrypted to the old key.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::BackendError`] if the files cannot be created, or
    /// [`StoreError::InsecurePermissions`] if an existing identity file is
    /// world- or group-readable.
    pub fn initialize() -> Result<InitOutcome, StoreError> {
        Self::initialize_at(&default_secrets_path()?, &default_identity_path()?)
    }

    /// Create the identity and an empty store at explicit paths.
    ///
    /// See [`initialize`](Self::initialize).
    ///
    /// # Errors
    ///
    /// See [`initialize`](Self::initialize).
    pub fn initialize_at(
        secrets_path: &Path,
        identity_path: &Path,
    ) -> Result<InitOutcome, StoreError> {
        let created_identity = if identity_path.exists() {
            ensure_owner_only(identity_path)?;
            false
        } else {
            write_new_identity(identity_path)?;
            true
        };

        let identity = load_identity(identity_path)?;
        let recipient = identity.to_public();

        let created_store = if secrets_path.exists() {
            false
        } else {
            let empty = StoreFile::default();
            write_encrypted(secrets_path, &recipient, &empty)?;
            true
        };

        Ok(InitOutcome {
            identity_path: identity_path.to_path_buf(),
            secrets_path: secrets_path.to_path_buf(),
            public_key: recipient.to_string(),
            created_identity,
            created_store,
        })
    }

    /// Whether a store has been initialised at the default paths.
    ///
    /// Only the identity file matters: the secrets file is created on first write.
    pub fn is_initialized() -> bool {
        default_identity_path().is_ok_and(|path| path.exists())
    }

    /// Whether a store has been initialised at the given identity path.
    pub fn is_initialized_at(identity_path: &Path) -> bool {
        identity_path.exists()
    }

    /// Prove this backend actually works, right now.
    ///
    /// Encrypts and decrypts a sentinel to confirm the identity is usable, then -
    /// if a store file exists - decrypts and parses it, which is what catches the
    /// interesting failure: an identity that does not match the file it is
    /// pointed at. Cheap enough to run on every process start.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::BackendUnavailable`] with the reason.
    pub fn probe(&self) -> Result<(), StoreError> {
        const SENTINEL: &[u8] = b"sigilforge-probe";

        let ciphertext = age::encrypt(&self.recipient, SENTINEL).map_err(|e| {
            StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: format!("age encryption failed: {}", e),
            }
        })?;

        let plaintext = age::decrypt(&self.identity, &ciphertext).map_err(|e| {
            StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: format!("age decryption failed: {}", e),
            }
        })?;

        if plaintext != SENTINEL {
            return Err(StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: "age round-trip returned different bytes".to_string(),
            });
        }

        // The real test: can this identity read the file it is paired with?
        self.read_store()?;

        Ok(())
    }

    /// Path of the encrypted store file.
    pub fn secrets_path(&self) -> &Path {
        &self.secrets_path
    }

    /// Path of the identity file. The file's *contents* are never exposed.
    pub fn identity_path(&self) -> &Path {
        &self.identity_path
    }

    /// Decrypt and parse the store. An absent file is an empty store.
    fn read_store(&self) -> Result<StoreFile, StoreError> {
        let ciphertext = match fs::read(&self.secrets_path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StoreFile::default()),
            Err(e) => {
                return Err(StoreError::BackendUnavailable {
                    backend: super::StoreBackend::EncryptedFile.to_string(),
                    reason: format!("could not read {}: {}", self.secrets_path.display(), e),
                });
            }
        };

        if ciphertext.is_empty() {
            // An empty store is still a valid age file of a few hundred bytes,
            // and writes are atomic, so zero bytes never means "no secrets yet".
            // It means truncation - a failed copy, a full disk, a stray `>`.
            // Reporting that as an empty store is the silent data loss this
            // whole backend was written to end.
            return Err(StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: format!(
                    "{} is zero bytes, which means it was truncated rather than left \
                     empty; restore it from backup, or delete it to start over (which \
                     discards every secret it held)",
                    self.secrets_path.display()
                ),
            });
        }

        let mut plaintext = age::decrypt(&self.identity, &ciphertext).map_err(|e| {
            StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: format!(
                    "could not decrypt {} with the identity at {}: {}",
                    self.secrets_path.display(),
                    self.identity_path.display(),
                    e
                ),
            }
        })?;

        let parsed = serde_json::from_slice::<StoreFile>(&plaintext).map_err(|e| {
            StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                // `e` reports a position, not the bytes at it, so nothing leaks.
                reason: format!(
                    "{} decrypted but is not a valid store: {}",
                    self.secrets_path.display(),
                    e
                ),
            }
        });

        plaintext.zeroize();
        let parsed = parsed?;

        if parsed.version > STORE_FORMAT_VERSION {
            return Err(StoreError::BackendUnavailable {
                backend: super::StoreBackend::EncryptedFile.to_string(),
                reason: format!(
                    "{} was written by a newer Sigilforge (store format {}, this build \
                     understands {})",
                    self.secrets_path.display(),
                    parsed.version,
                    STORE_FORMAT_VERSION
                ),
            });
        }

        Ok(parsed)
    }

    /// Read, modify, and rewrite the store while holding the write lock.
    async fn update<F>(&self, mutate: F) -> Result<(), StoreError>
    where
        F: FnOnce(&mut StoreFile),
    {
        let _lock = WriteLock::acquire(&self.secrets_path).await?;

        let mut store = self.read_store()?;
        mutate(&mut store);
        write_encrypted(&self.secrets_path, &self.recipient, &store)
    }
}

impl std::fmt::Debug for EncryptedFileStore {
    /// Paths only. The identity and the decrypted secrets never appear.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedFileStore")
            .field("secrets_path", &self.secrets_path)
            .field("identity_path", &self.identity_path)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl SecretStore for EncryptedFileStore {
    async fn get(&self, key: &str) -> Result<Option<Secret>, StoreError> {
        Ok(self
            .read_store()?
            .secrets
            .get(key)
            .map(|value| Secret::new(value.as_str())))
    }

    async fn set(&self, key: &str, secret: &Secret) -> Result<(), StoreError> {
        let key = key.to_string();
        let value = secret.expose().to_string();

        self.update(move |store| {
            store.secrets.insert(key, value);
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        let key = key.to_string();

        self.update(move |store| {
            store.secrets.remove(&key);
        })
        .await
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        // Unlike the platform keyrings, this backend can actually enumerate.
        Ok(self
            .read_store()?
            .secrets
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
}

/// The decrypted payload: a flat map of storage key to secret value.
#[derive(Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    secrets: BTreeMap<String, String>,
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            version: STORE_FORMAT_VERSION,
            secrets: BTreeMap::new(),
        }
    }
}

impl Drop for StoreFile {
    /// Wipe decrypted secret values rather than leaving them in freed heap.
    fn drop(&mut self) {
        for value in self.secrets.values_mut() {
            value.zeroize();
        }
    }
}

/// The default identity path: `~/.config/sigilforge/age-identity.key`.
pub fn default_identity_path() -> Result<PathBuf, StoreError> {
    Ok(project_dirs()?.config_dir().join(IDENTITY_FILE_NAME))
}

/// The default secrets path: `~/.local/share/sigilforge/secrets.age`.
///
/// Deliberately a different directory from the identity, so that backing up one
/// does not back up the other.
pub fn default_secrets_path() -> Result<PathBuf, StoreError> {
    Ok(project_dirs()?.data_dir().join(SECRETS_FILE_NAME))
}

fn project_dirs() -> Result<directories::ProjectDirs, StoreError> {
    directories::ProjectDirs::from("com", "raibid-labs", "sigilforge").ok_or_else(|| {
        StoreError::BackendUnavailable {
            backend: super::StoreBackend::EncryptedFile.to_string(),
            reason: "no home directory to place the store in; set SIGILFORGE_AGE_IDENTITY \
                     and SIGILFORGE_SECRETS_FILE"
                .to_string(),
        }
    })
}

/// Read and parse the identity file, refusing anything other people can read.
fn load_identity(path: &Path) -> Result<Identity, StoreError> {
    if !path.exists() {
        return Err(StoreError::BackendUnavailable {
            backend: super::StoreBackend::EncryptedFile.to_string(),
            reason: format!(
                "no age identity at {}; run `sigilforge store init` to create one",
                path.display()
            ),
        });
    }

    ensure_owner_only(path)?;

    let mut contents = fs::read_to_string(path).map_err(|e| StoreError::BackendUnavailable {
        backend: super::StoreBackend::EncryptedFile.to_string(),
        reason: format!("could not read {}: {}", path.display(), e),
    })?;

    // age identity files carry `# created:` / `# public key:` comment lines.
    let parsed = contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| StoreError::BackendUnavailable {
            backend: super::StoreBackend::EncryptedFile.to_string(),
            reason: format!("{} contains no age identity", path.display()),
        })
        .and_then(|line| {
            // `Identity`'s parse error is a fixed &'static str, so no key
            // material can reach a log line through it.
            line.parse::<Identity>()
                .map_err(|e| StoreError::BackendUnavailable {
                    backend: super::StoreBackend::EncryptedFile.to_string(),
                    reason: format!(
                        "{} is not a valid age identity ({}); it should hold one \
                         AGE-SECRET-KEY-1... line",
                        path.display(),
                        e
                    ),
                })
        });

    contents.zeroize();
    parsed
}

/// Generate a fresh identity and write it `0600`, in `age-keygen` format.
fn write_new_identity(path: &Path) -> Result<(), StoreError> {
    create_parent_dir(path)?;

    let identity = Identity::generate();
    let public_key = identity.to_public().to_string();
    let secret = identity.to_string();

    let created = chrono::Utc::now().to_rfc3339();
    let mut contents = format!(
        "# created: {}\n# public key: {}\n{}\n",
        created,
        public_key,
        secret.expose_secret()
    );

    let result = write_private_file(path, contents.as_bytes());
    contents.zeroize();
    result
}

/// Serialize, encrypt, and atomically replace the store file.
fn write_encrypted(
    path: &Path,
    recipient: &Recipient,
    store: &StoreFile,
) -> Result<(), StoreError> {
    create_parent_dir(path)?;

    let mut plaintext = serde_json::to_vec(store)?;
    let encrypted = age::encrypt(recipient, &plaintext);
    plaintext.zeroize();

    let ciphertext = encrypted.map_err(|e| StoreError::BackendError {
        message: format!("could not encrypt {}: {}", path.display(), e),
    })?;

    write_private_file(path, &ciphertext)
}

/// Write `contents` to `path` atomically, owner-readable only.
///
/// A temporary file in the same directory is written, flushed to disk, and
/// renamed over the target, so a crash cannot leave a half-written store.
fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    let temp_path = temp_sibling(path);

    let write = || -> std::io::Result<()> {
        // Remove a temp file left by a previous crash; `create_new` would fail on it.
        let _ = fs::remove_file(&temp_path);

        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);

        // On Windows `rename` fails if the destination exists.
        #[cfg(windows)]
        let _ = fs::remove_file(path);

        fs::rename(&temp_path, path)
    };

    write().map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        StoreError::BackendError {
            message: format!("could not write {}: {}", path.display(), e),
        }
    })
}

/// Create the parent directory, owner-accessible only.
fn create_parent_dir(path: &Path) -> Result<(), StoreError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    if parent.as_os_str().is_empty() || parent.exists() {
        return Ok(());
    }

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }

    builder
        .create(parent)
        .map_err(|e| StoreError::BackendError {
            message: format!("could not create {}: {}", parent.display(), e),
        })
}

/// Reject a key file that group or other can read.
///
/// A `0644` private key is a finding, not a warning: trusting it silently is how
/// a secret ends up readable by every account on a shared build host.
#[cfg(unix)]
fn ensure_owner_only(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path).map_err(|e| StoreError::BackendUnavailable {
        backend: super::StoreBackend::EncryptedFile.to_string(),
        reason: format!("could not stat {}: {}", path.display(), e),
    })?;

    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(StoreError::InsecurePermissions {
            path: path.display().to_string(),
            mode,
        });
    }

    Ok(())
}

/// Windows has no mode bits; ACLs are out of scope.
#[cfg(not(unix))]
fn ensure_owner_only(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}", std::process::id()));
    path.with_file_name(name)
}

/// An advisory lock held for the duration of a read-modify-write.
///
/// Not a kernel lock: it is a file whose existence means "someone is writing".
/// That is enough for the processes that share this store, all of which take it
/// the same way, and it needs no extra dependency.
struct WriteLock {
    path: PathBuf,
}

impl WriteLock {
    async fn acquire(store_path: &Path) -> Result<Self, StoreError> {
        let mut name = store_path.file_name().unwrap_or_default().to_os_string();
        name.push(".lock");
        let path = store_path.with_file_name(name);

        create_parent_dir(&path)?;

        let deadline = SystemTime::now() + LOCK_TIMEOUT;

        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::break_if_stale(&path) {
                        continue;
                    }
                    if SystemTime::now() >= deadline {
                        return Err(StoreError::BackendError {
                            message: format!(
                                "timed out waiting for the lock at {}; if no other \
                                 Sigilforge process is running, delete it",
                                path.display()
                            ),
                        });
                    }
                    tokio::time::sleep(LOCK_POLL_INTERVAL).await;
                }
                Err(e) => {
                    return Err(StoreError::BackendError {
                        message: format!("could not create the lock at {}: {}", path.display(), e),
                    });
                }
            }
        }
    }

    /// Remove a lock whose owner evidently died. Returns whether it removed one.
    fn break_if_stale(path: &Path) -> bool {
        let age = fs::metadata(path).and_then(|m| m.modified()).and_then(|m| {
            SystemTime::now()
                .duration_since(m)
                .map_err(|_| std::io::Error::other("lock file is dated in the future"))
        });

        match age {
            Ok(age) if age > LOCK_STALE_AFTER => {
                tracing::warn!(
                    "removing stale secret store lock at {} ({}s old)",
                    path.display(),
                    age.as_secs()
                );
                fs::remove_file(path).is_ok()
            }
            _ => false,
        }
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A store on a fresh temporary directory. Nothing here touches D-Bus, a
    /// keyring daemon, or the user's real config.
    fn test_store() -> (EncryptedFileStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let secrets = dir.path().join("data").join("secrets.age");
        let identity = dir.path().join("config").join("age-identity.key");

        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();
        let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn test_round_trip_without_a_session_bus() {
        let (store, _dir) = test_store();

        store
            .set(
                "sigilforge/github-app/acme/private_key",
                &Secret::new("pem"),
            )
            .await
            .unwrap();

        let found = store
            .get("sigilforge/github-app/acme/private_key")
            .await
            .unwrap();
        assert_eq!(found.unwrap().expose(), "pem");
    }

    #[tokio::test]
    async fn test_value_survives_reopening_the_store() {
        // The bug this backend exists to fix: a write that vanishes when the
        // process does. A second handle stands in for a second process.
        let dir = TempDir::new().unwrap();
        let secrets = dir.path().join("secrets.age");
        let identity = dir.path().join("age-identity.key");
        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        {
            let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
            store.set("k", &Secret::new("v")).await.unwrap();
        }

        let reopened = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
        assert_eq!(reopened.get("k").await.unwrap().unwrap().expose(), "v");
    }

    #[tokio::test]
    async fn test_get_missing_key_is_none_not_an_error() {
        let (store, _dir) = test_store();
        assert!(store.get("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_is_idempotent() {
        let (store, _dir) = test_store();

        store.set("k", &Secret::new("v")).await.unwrap();
        store.delete("k").await.unwrap();
        store.delete("k").await.unwrap();

        assert!(store.get("k").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_keys_filters_by_prefix() {
        let (store, _dir) = test_store();

        store
            .set(
                "sigilforge/spotify/personal/access_token",
                &Secret::new("a"),
            )
            .await
            .unwrap();
        store
            .set("sigilforge/spotify/work/access_token", &Secret::new("b"))
            .await
            .unwrap();
        store
            .set("sigilforge/github/main/api_key", &Secret::new("c"))
            .await
            .unwrap();

        let spotify = store.list_keys("sigilforge/spotify").await.unwrap();
        assert_eq!(spotify.len(), 2);

        let all = store.list_keys("sigilforge").await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_exists_matches_get() {
        let (store, _dir) = test_store();

        assert!(!store.exists("k").await.unwrap());
        store.set("k", &Secret::new("v")).await.unwrap();
        assert!(store.exists("k").await.unwrap());
    }

    #[tokio::test]
    async fn test_overwrite_replaces_the_value() {
        let (store, _dir) = test_store();

        store.set("k", &Secret::new("first")).await.unwrap();
        store.set("k", &Secret::new("second")).await.unwrap();

        assert_eq!(store.get("k").await.unwrap().unwrap().expose(), "second");
    }

    #[tokio::test]
    async fn test_secret_bytes_are_not_on_disk_in_the_clear() {
        let (store, _dir) = test_store();

        store
            .set("k", &Secret::new("correct-horse-battery-staple"))
            .await
            .unwrap();

        let on_disk = fs::read(store.secrets_path()).unwrap();
        assert!(
            !on_disk
                .windows(28)
                .any(|w| w == b"correct-horse-battery-staple")
        );
        // It really is an age file.
        assert!(on_disk.starts_with(b"age-encryption.org/"));
    }

    #[test]
    fn test_identity_file_is_created_0600() {
        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        let secrets = dir.path().join("secrets.age");

        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&identity).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "identity mode was {:o}", mode);
            let mode = fs::metadata(&secrets).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "secrets mode was {:o}", mode);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_world_readable_identity_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        let secrets = dir.path().join("secrets.age");
        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        fs::set_permissions(&identity, fs::Permissions::from_mode(0o644)).unwrap();

        let err = EncryptedFileStore::open_at(&secrets, &identity).unwrap_err();
        assert!(
            matches!(err, StoreError::InsecurePermissions { mode: 0o644, .. }),
            "expected InsecurePermissions, got {:?}",
            err
        );
        // The message has to tell the operator what to type.
        assert!(err.to_string().contains("chmod 600"));
    }

    #[cfg(unix)]
    #[test]
    fn test_group_readable_identity_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        let secrets = dir.path().join("secrets.age");
        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        fs::set_permissions(&identity, fs::Permissions::from_mode(0o640)).unwrap();

        assert!(matches!(
            EncryptedFileStore::open_at(&secrets, &identity),
            Err(StoreError::InsecurePermissions { .. })
        ));
    }

    #[test]
    fn test_open_without_initialize_says_what_to_run() {
        let dir = TempDir::new().unwrap();
        let err = EncryptedFileStore::open_at(
            &dir.path().join("secrets.age"),
            &dir.path().join("age-identity.key"),
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("sigilforge store init"), "{}", message);
    }

    #[test]
    fn test_initialize_is_idempotent_and_keeps_the_key() {
        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        let secrets = dir.path().join("secrets.age");

        let first = EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();
        assert!(first.created_identity);
        assert!(first.created_store);

        let second = EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();
        assert!(!second.created_identity);
        assert!(!second.created_store);

        // Re-initialising must not roll the key; that would orphan every secret.
        assert_eq!(first.public_key, second.public_key);
    }

    #[test]
    fn test_probe_succeeds_on_a_fresh_store() {
        let (store, _dir) = test_store();
        store.probe().unwrap();
    }

    #[test]
    fn test_probe_rejects_a_mismatched_identity() {
        let dir = TempDir::new().unwrap();
        let secrets = dir.path().join("secrets.age");
        let mine = dir.path().join("mine.key");
        let theirs = dir.path().join("theirs.key");

        EncryptedFileStore::initialize_at(&secrets, &mine).unwrap();
        write_new_identity(&theirs).unwrap();

        let wrong = EncryptedFileStore::open_at(&secrets, &theirs).unwrap();
        let err = wrong.probe().unwrap_err();

        assert!(matches!(err, StoreError::BackendUnavailable { .. }));
        assert!(err.to_string().contains("could not decrypt"), "{}", err);
    }

    #[tokio::test]
    async fn test_corrupt_store_errors_rather_than_reading_empty() {
        // The whole point: a store we cannot read must not look like an empty one.
        let (store, _dir) = test_store();
        store.set("k", &Secret::new("v")).await.unwrap();

        fs::write(store.secrets_path(), b"age-encryption.org/v1\nnot really").unwrap();

        let err = store.get("k").await.unwrap_err();
        assert!(matches!(err, StoreError::BackendUnavailable { .. }));

        let err = store.list_keys("").await.unwrap_err();
        assert!(matches!(err, StoreError::BackendUnavailable { .. }));
    }

    #[tokio::test]
    async fn test_truncated_store_file_is_not_mistaken_for_an_empty_one() {
        let (store, _dir) = test_store();
        store.set("k", &Secret::new("v")).await.unwrap();

        // What a failed copy or a full disk leaves behind.
        fs::write(store.secrets_path(), b"").unwrap();

        let err = store.get("k").await.unwrap_err();
        assert!(matches!(err, StoreError::BackendUnavailable { .. }));
        assert!(err.to_string().contains("truncated"), "{}", err);
    }

    #[tokio::test]
    async fn test_absent_store_file_reads_as_empty() {
        let dir = TempDir::new().unwrap();
        let secrets = dir.path().join("secrets.age");
        let identity = dir.path().join("age-identity.key");
        write_new_identity(&identity).unwrap();

        // Identity but no store file: legitimately empty, not an error.
        let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
        assert!(store.get("k").await.unwrap().is_none());
        assert!(store.list_keys("").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_store_from_a_newer_format_is_refused() {
        let (store, _dir) = test_store();

        let future = StoreFile {
            version: STORE_FORMAT_VERSION + 1,
            secrets: BTreeMap::new(),
        };
        write_encrypted(store.secrets_path(), &store.recipient, &future).unwrap();

        let err = store.get("k").await.unwrap_err();
        assert!(err.to_string().contains("newer Sigilforge"), "{}", err);
    }

    #[test]
    fn test_debug_never_prints_key_material() {
        let (store, _dir) = test_store();

        let rendered = format!("{:?}", store);
        assert!(rendered.contains("EncryptedFileStore"));
        assert!(!rendered.contains("AGE-SECRET-KEY"));
        assert!(!rendered.to_lowercase().contains("identity: "));
    }

    #[test]
    fn test_identity_file_does_not_contain_the_key_in_a_comment() {
        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        write_new_identity(&identity).unwrap();

        let contents = fs::read_to_string(&identity).unwrap();
        // age-keygen layout: two comments then the key.
        assert!(contents.contains("# created:"));
        assert!(contents.contains("# public key: age1"));
        assert_eq!(
            contents
                .lines()
                .filter(|l| l.starts_with("AGE-SECRET-KEY-1"))
                .count(),
            1
        );
    }

    #[test]
    fn test_identity_file_with_only_comments_is_rejected() {
        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        write_private_file(&identity, b"# created: yesterday\n# public key: age1xyz\n").unwrap();

        let err = EncryptedFileStore::open_at(&dir.path().join("s.age"), &identity).unwrap_err();
        assert!(err.to_string().contains("no age identity"), "{}", err);
    }

    #[test]
    fn test_garbage_identity_file_does_not_echo_its_contents() {
        let dir = TempDir::new().unwrap();
        let identity = dir.path().join("age-identity.key");
        write_private_file(&identity, b"AGE-SECRET-KEY-1DEADBEEFNOTAREALKEY\n").unwrap();

        let err = EncryptedFileStore::open_at(&dir.path().join("s.age"), &identity).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("not a valid age identity"), "{}", message);
        assert!(!message.contains("DEADBEEF"), "{}", message);
    }

    #[tokio::test]
    async fn test_write_lock_is_released_after_a_write() {
        let (store, _dir) = test_store();

        store.set("a", &Secret::new("1")).await.unwrap();
        // A second write would block forever if the guard leaked.
        store.set("b", &Secret::new("2")).await.unwrap();

        let mut name = store.secrets_path().file_name().unwrap().to_os_string();
        name.push(".lock");
        assert!(!store.secrets_path().with_file_name(name).exists());
    }

    #[tokio::test]
    async fn test_concurrent_writes_do_not_lose_values() {
        let dir = TempDir::new().unwrap();
        let secrets = dir.path().join("secrets.age");
        let identity = dir.path().join("age-identity.key");
        EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

        let mut tasks = Vec::new();
        for i in 0..8 {
            let secrets = secrets.clone();
            let identity = identity.clone();
            tasks.push(tokio::spawn(async move {
                let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
                store
                    .set(&format!("key-{}", i), &Secret::new(format!("value-{}", i)))
                    .await
                    .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let store = EncryptedFileStore::open_at(&secrets, &identity).unwrap();
        assert_eq!(store.list_keys("key-").await.unwrap().len(), 8);
    }

    #[tokio::test]
    async fn test_stale_lock_is_broken_rather_than_deadlocking() {
        let (store, _dir) = test_store();

        let mut name = store.secrets_path().file_name().unwrap().to_os_string();
        name.push(".lock");
        let lock_path = store.secrets_path().with_file_name(name);

        fs::write(&lock_path, b"").unwrap();
        let stale = SystemTime::now() - LOCK_STALE_AFTER - Duration::from_secs(60);
        let file = fs::File::options().write(true).open(&lock_path).unwrap();
        file.set_modified(stale).unwrap();
        drop(file);

        store.set("k", &Secret::new("v")).await.unwrap();
        assert_eq!(store.get("k").await.unwrap().unwrap().expose(), "v");
    }

    #[test]
    fn test_default_paths_are_in_different_directories() {
        // Key next to ciphertext is not much of a key.
        let (Ok(identity), Ok(secrets)) = (default_identity_path(), default_secrets_path()) else {
            return; // no home directory in this environment
        };
        assert_ne!(identity.parent(), secrets.parent());
    }
}
