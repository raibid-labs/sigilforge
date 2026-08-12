//! Which secret-store backend to use, and where its files live.
//!
//! Backend selection used to be a boolean (`prefer_keyring`) that fell through
//! to [`MemoryStore`](super::MemoryStore) when the keyring could not be
//! constructed. That is the wrong shape for a credential manager: a process
//! that reads from a store which quietly became a fresh, empty `HashMap`
//! reports "not registered" for a credential that is sitting safely on disk.
//!
//! This module replaces the boolean with an explicit three-way choice plus an
//! `auto` mode, and [`super::open_store`] proves the chosen backend works
//! before handing it out.
//!
//! # Precedence
//!
//! 1. `SIGILFORGE_STORE_BACKEND` environment variable
//! 2. `[storage] backend` in `~/.config/sigilforge/config.toml`
//! 3. `auto` - prefer the encrypted file store if it has been initialised,
//!    otherwise the OS keyring if it answers a round-trip probe
//!
//! `memory` is never chosen automatically. It has to be asked for by name.
//!
//! # Example
//!
//! ```
//! use sigilforge_core::store::{StoreBackend, StoreConfig};
//!
//! // Explicitly pin the backend, bypassing config file and environment.
//! let config = StoreConfig::for_backend(StoreBackend::Memory);
//! assert_eq!(config.backend, Some(StoreBackend::Memory));
//! ```

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use super::StoreError;

/// Environment variable naming the backend explicitly.
///
/// Accepts `keyring`, `encrypted-file`, `memory`, or `auto`.
pub const BACKEND_ENV_VAR: &str = "SIGILFORGE_STORE_BACKEND";

/// Environment variable overriding the age identity (private key) file path.
pub const IDENTITY_ENV_VAR: &str = "SIGILFORGE_AGE_IDENTITY";

/// Environment variable overriding the encrypted secrets file path.
pub const SECRETS_ENV_VAR: &str = "SIGILFORGE_SECRETS_FILE";

/// A secret storage backend.
///
/// # Example
///
/// ```
/// use sigilforge_core::store::StoreBackend;
///
/// assert_eq!(
///     "encrypted-file".parse::<StoreBackend>().unwrap(),
///     StoreBackend::EncryptedFile,
/// );
/// assert_eq!(StoreBackend::Keyring.to_string(), "keyring");
/// assert!("postgres".parse::<StoreBackend>().is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreBackend {
    /// The platform keyring: Secret Service on Linux, Keychain on macOS,
    /// Credential Manager on Windows. Needs a D-Bus session bus on Linux.
    Keyring,

    /// An age-encrypted file plus a separate identity (private key) file.
    /// Needs no session bus, no desktop, and no prompt on read.
    EncryptedFile,

    /// A process-local `HashMap`. Never selected automatically.
    Memory,
}

impl StoreBackend {
    /// The canonical name, as accepted by [`BACKEND_ENV_VAR`] and the config file.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::EncryptedFile => "encrypted-file",
            Self::Memory => "memory",
        }
    }

    /// Whether secrets written to this backend survive process exit.
    ///
    /// ```
    /// use sigilforge_core::store::StoreBackend;
    ///
    /// assert!(StoreBackend::EncryptedFile.is_persistent());
    /// assert!(!StoreBackend::Memory.is_persistent());
    /// ```
    pub fn is_persistent(&self) -> bool {
        !matches!(self, Self::Memory)
    }
}

impl std::fmt::Display for StoreBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StoreBackend {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "keyring" | "os-keyring" | "secret-service" => Ok(Self::Keyring),
            "encrypted-file" | "encrypted" | "age" | "file" => Ok(Self::EncryptedFile),
            "memory" | "mem" => Ok(Self::Memory),
            other => Err(StoreError::BackendError {
                message: format!(
                    "unknown secret store backend '{}'; expected one of: keyring, \
                     encrypted-file, memory, auto",
                    other
                ),
            }),
        }
    }
}

/// Where the store's configuration came from, for honest error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendSource {
    /// Set by [`BACKEND_ENV_VAR`].
    Environment,
    /// Set by `[storage] backend` in the config file.
    ConfigFile,
    /// Chosen by probing, because nothing asked for a specific backend.
    Automatic,
}

impl std::fmt::Display for BackendSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Environment => write!(f, "{} environment variable", BACKEND_ENV_VAR),
            Self::ConfigFile => write!(f, "[storage] backend in the config file"),
            Self::Automatic => write!(f, "automatic selection"),
        }
    }
}

/// Resolved storage configuration.
///
/// Build one with [`StoreConfig::load`] (config file + environment) or
/// [`StoreConfig::for_backend`] (explicit, ignores both).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreConfig {
    /// The requested backend, or `None` for automatic selection.
    pub backend: Option<StoreBackend>,

    /// Where the backend request came from, for error messages.
    pub source: Option<BackendSource>,

    /// Override for the age identity file (the private key).
    pub identity_file: Option<PathBuf>,

    /// Override for the age-encrypted secrets file.
    pub secrets_file: Option<PathBuf>,
}

impl StoreConfig {
    /// Load configuration from the config file, then apply environment overrides.
    ///
    /// A missing config file is not an error; a malformed one is. Silently
    /// ignoring a typo in `backend = "keryring"` would put us back where we
    /// started - using a backend the operator did not ask for.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::BackendError`] if the config file cannot be read or
    /// parsed, or if a backend name in the file or environment is not recognised.
    pub fn load() -> Result<Self, StoreError> {
        let mut config = match Self::default_config_path() {
            Some(path) if path.exists() => Self::from_file(&path)?,
            _ => Self::default(),
        };

        config.apply_env()?;
        Ok(config)
    }

    /// A configuration that pins one backend, ignoring the environment and
    /// config file.
    ///
    /// # Example
    ///
    /// ```
    /// use sigilforge_core::store::{StoreBackend, StoreConfig, open_store_with};
    ///
    /// let store = open_store_with(&StoreConfig::for_backend(StoreBackend::Memory)).unwrap();
    /// # drop(store);
    /// ```
    pub fn for_backend(backend: StoreBackend) -> Self {
        Self {
            backend: Some(backend),
            source: Some(BackendSource::Automatic),
            ..Self::default()
        }
    }

    /// Parse a `config.toml`.
    ///
    /// Only the `[storage]` table is read; other tables (`[daemon]`,
    /// `[providers.*]`, ...) are ignored so one file can hold everything.
    ///
    /// ```toml
    /// [storage]
    /// backend = "encrypted-file"
    /// identity_file = "~/.config/sigilforge/age-identity.key"
    /// secrets_file = "~/.local/share/sigilforge/secrets.age"
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::BackendError`] if the file cannot be read or parsed,
    /// or names an unknown backend.
    pub fn from_file(path: &Path) -> Result<Self, StoreError> {
        let contents = std::fs::read_to_string(path).map_err(|e| StoreError::BackendError {
            message: format!("could not read {}: {}", path.display(), e),
        })?;

        let file: ConfigFile = toml::from_str(&contents).map_err(|e| StoreError::BackendError {
            message: format!("could not parse {}: {}", path.display(), e),
        })?;

        let backend = file
            .storage
            .backend
            .as_deref()
            .filter(|b| !b.eq_ignore_ascii_case("auto"))
            .map(StoreBackend::from_str)
            .transpose()?;

        Ok(Self {
            source: backend.is_some().then_some(BackendSource::ConfigFile),
            backend,
            identity_file: file.storage.identity_file.map(|p| expand_tilde(&p)),
            secrets_file: file.storage.secrets_file.map(|p| expand_tilde(&p)),
        })
    }

    /// The default config file path, `~/.config/sigilforge/config.toml` on Linux.
    ///
    /// Returns `None` when the platform has no home directory to speak of.
    pub fn default_config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "raibid-labs", "sigilforge")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    /// Overlay `SIGILFORGE_*` environment variables onto this configuration.
    fn apply_env(&mut self) -> Result<(), StoreError> {
        if let Some(raw) = non_empty_env(BACKEND_ENV_VAR) {
            if raw.eq_ignore_ascii_case("auto") {
                self.backend = None;
                self.source = None;
            } else {
                self.backend = Some(StoreBackend::from_str(&raw)?);
                self.source = Some(BackendSource::Environment);
            }
        }

        if let Some(raw) = non_empty_env(IDENTITY_ENV_VAR) {
            self.identity_file = Some(expand_tilde(Path::new(&raw)));
        }

        if let Some(raw) = non_empty_env(SECRETS_ENV_VAR) {
            self.secrets_file = Some(expand_tilde(Path::new(&raw)));
        }

        Ok(())
    }

    /// Describe where the backend choice came from, for error messages.
    ///
    /// ```
    /// use sigilforge_core::store::{StoreBackend, StoreConfig};
    ///
    /// let config = StoreConfig::for_backend(StoreBackend::Memory);
    /// assert_eq!(config.source_description(), "automatic selection");
    /// ```
    pub fn source_description(&self) -> String {
        self.source.unwrap_or(BackendSource::Automatic).to_string()
    }
}

/// Read an environment variable, treating "set but empty" as unset.
fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Expand a leading `~/` against the home directory.
///
/// Config files are written by humans, and humans write `~`.
fn expand_tilde(path: &Path) -> PathBuf {
    let Some(rest) = path.to_str().and_then(|s| s.strip_prefix("~/")) else {
        return path.to_path_buf();
    };

    match directories::BaseDirs::new() {
        Some(dirs) => dirs.home_dir().join(rest),
        None => path.to_path_buf(),
    }
}

/// The subset of `config.toml` this module cares about.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    storage: StorageSection,
}

#[derive(Debug, Default, Deserialize)]
struct StorageSection {
    backend: Option<String>,
    identity_file: Option<PathBuf>,
    secrets_file: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        (dir, path)
    }

    fn parse(s: &str) -> StoreBackend {
        s.parse::<StoreBackend>().unwrap()
    }

    #[test]
    fn test_backend_parsing_accepts_aliases() {
        assert_eq!(parse("keyring"), StoreBackend::Keyring);
        assert_eq!(parse("Secret_Service"), StoreBackend::Keyring);
        assert_eq!(parse("encrypted-file"), StoreBackend::EncryptedFile);
        assert_eq!(parse("encrypted_file"), StoreBackend::EncryptedFile);
        assert_eq!(parse("AGE"), StoreBackend::EncryptedFile);
        assert_eq!(parse("  memory  "), StoreBackend::Memory);
    }

    #[test]
    fn test_backend_parsing_rejects_typos_loudly() {
        // A typo must not silently mean "auto" - that is how you end up on a
        // backend nobody asked for.
        let err = "keryring".parse::<StoreBackend>().unwrap_err();
        let message = err.to_string();
        assert!(message.contains("keryring"), "{}", message);
        assert!(message.contains("encrypted-file"), "{}", message);
    }

    #[test]
    fn test_backend_roundtrips_through_display() {
        for backend in [
            StoreBackend::Keyring,
            StoreBackend::EncryptedFile,
            StoreBackend::Memory,
        ] {
            assert_eq!(parse(&backend.to_string()), backend);
        }
    }

    #[test]
    fn test_config_file_selects_backend_and_paths() {
        let (_dir, path) = write_config(
            r#"
            [storage]
            backend = "encrypted-file"
            identity_file = "/keys/identity.key"
            secrets_file = "/data/secrets.age"
            "#,
        );

        let config = StoreConfig::from_file(&path).unwrap();
        assert_eq!(config.backend, Some(StoreBackend::EncryptedFile));
        assert_eq!(config.source, Some(BackendSource::ConfigFile));
        assert_eq!(
            config.identity_file,
            Some(PathBuf::from("/keys/identity.key"))
        );
        assert_eq!(
            config.secrets_file,
            Some(PathBuf::from("/data/secrets.age"))
        );
    }

    #[test]
    fn test_config_file_ignores_unrelated_tables() {
        let (_dir, path) = write_config(
            r#"
            [daemon]
            socket_path = "/run/user/1000/sigilforge.sock"

            [storage]
            backend = "memory"

            [providers.spotify]
            client_id = "abc"
            "#,
        );

        let config = StoreConfig::from_file(&path).unwrap();
        assert_eq!(config.backend, Some(StoreBackend::Memory));
    }

    #[test]
    fn test_config_file_without_storage_section_is_auto() {
        let (_dir, path) = write_config("[daemon]\nlog_level = \"info\"\n");

        let config = StoreConfig::from_file(&path).unwrap();
        assert_eq!(config.backend, None);
        assert_eq!(config.source, None);
    }

    #[test]
    fn test_config_file_auto_is_not_a_backend() {
        let (_dir, path) = write_config("[storage]\nbackend = \"auto\"\n");

        let config = StoreConfig::from_file(&path).unwrap();
        assert_eq!(config.backend, None);
    }

    #[test]
    fn test_malformed_config_file_is_an_error() {
        let (_dir, path) = write_config("[storage\nbackend =");

        let err = StoreConfig::from_file(&path).unwrap_err();
        assert!(err.to_string().contains("could not parse"));
    }

    #[test]
    fn test_unknown_backend_in_config_file_is_an_error() {
        let (_dir, path) = write_config("[storage]\nbackend = \"vault\"\n");

        let err = StoreConfig::from_file(&path).unwrap_err();
        assert!(err.to_string().contains("vault"));
    }

    #[test]
    fn test_for_backend_pins_the_choice() {
        let config = StoreConfig::for_backend(StoreBackend::Keyring);
        assert_eq!(config.backend, Some(StoreBackend::Keyring));
        assert!(config.identity_file.is_none());
    }

    #[test]
    fn test_expand_tilde_leaves_absolute_paths_alone() {
        assert_eq!(
            expand_tilde(Path::new("/etc/sigilforge/id")),
            PathBuf::from("/etc/sigilforge/id")
        );
        // No `~/` prefix means no expansion, even mid-path.
        assert_eq!(
            expand_tilde(Path::new("keys/~/id")),
            PathBuf::from("keys/~/id")
        );
    }

    #[test]
    fn test_source_description_mentions_the_env_var() {
        let config = StoreConfig {
            source: Some(BackendSource::Environment),
            ..StoreConfig::default()
        };
        assert!(config.source_description().contains(BACKEND_ENV_VAR));
    }
}
