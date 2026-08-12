//! The headless case, end to end: no D-Bus session bus, no keyring daemon,
//! no desktop, no prompt.
//!
//! Every test here is written to pass in an environment started with
//!
//! ```bash
//! env -u DBUS_SESSION_BUS_ADDRESS -u XDG_RUNTIME_DIR cargo test
//! ```
//!
//! and none of them touch the platform keyring, so they behave identically with
//! and without one. They cover the sequence a server actually performs: set the
//! store up once, write a credential from one process, read it from another.

#![cfg(feature = "encrypted-file-store")]

use std::path::{Path, PathBuf};

use sigilforge_core::store::{
    EncryptedFileStore, Secret, SecretStore, StoreBackend, StoreConfig, StoreError, open_store_with,
};
use tempfile::TempDir;

/// A config pointing at a fresh, initialised store in a temporary directory.
///
/// The identity and the secrets deliberately live in different directories, as
/// they do in the real default layout.
fn initialised_store() -> (TempDir, StoreConfig) {
    let dir = TempDir::new().unwrap();
    let identity = dir.path().join("config").join("age-identity.key");
    let secrets = dir.path().join("data").join("secrets.age");

    EncryptedFileStore::initialize_at(&secrets, &identity).unwrap();

    let config = StoreConfig {
        backend: Some(StoreBackend::EncryptedFile),
        identity_file: Some(identity),
        secrets_file: Some(secrets),
        ..StoreConfig::default()
    };

    (dir, config)
}

#[tokio::test]
async fn secret_written_in_one_process_is_readable_in_the_next() {
    let (_dir, config) = initialised_store();

    // "Process" one: register.
    {
        let store = open_store_with(&config).unwrap();
        store
            .set(
                "sigilforge/github-app/raibid-labs/private_key",
                &Secret::new("-----BEGIN RSA PRIVATE KEY-----"),
            )
            .await
            .unwrap();
    }

    // "Process" two: read it back. This is the exact sequence that failed on a
    // headless host - `register` in one invocation, `list` in another.
    let store = open_store_with(&config).unwrap();
    let found = store
        .get("sigilforge/github-app/raibid-labs/private_key")
        .await
        .unwrap();

    assert_eq!(
        found.unwrap().expose(),
        "-----BEGIN RSA PRIVATE KEY-----",
        "a credential written by one process must survive into the next"
    );
}

#[tokio::test]
async fn keys_can_be_enumerated_unlike_the_keyring() {
    let (_dir, config) = initialised_store();
    let store = open_store_with(&config).unwrap();

    for key in [
        "sigilforge/github-app/raibid-labs/app_id",
        "sigilforge/github-app/raibid-labs/installation_id",
        "sigilforge/spotify/personal/access_token",
    ] {
        store.set(key, &Secret::new("x")).await.unwrap();
    }

    let app_keys = store.list_keys("sigilforge/github-app/").await.unwrap();
    assert_eq!(app_keys.len(), 2);
}

#[tokio::test]
async fn an_unreadable_store_errors_rather_than_reading_as_empty() {
    // The defect this whole backend exists alongside: "storage is unavailable"
    // must never be reported as "you have no credentials".
    let (_dir, config) = initialised_store();

    {
        let store = open_store_with(&config).unwrap();
        store.set("k", &Secret::new("v")).await.unwrap();
    }

    // Corrupt the ciphertext, as a truncated copy or a bad restore would.
    let secrets = config.secrets_file.clone().unwrap();
    std::fs::write(&secrets, b"age-encryption.org/v1\ntruncated").unwrap();

    let store = EncryptedFileStore::open_at(&secrets, config.identity_file.as_ref().unwrap())
        .expect("the identity is still fine, so opening is fine");

    let err = store.get("k").await.unwrap_err();
    assert!(
        matches!(err, StoreError::BackendUnavailable { .. }),
        "expected BackendUnavailable, got {:?}",
        err
    );

    // And the same for enumeration, which is what `list` uses.
    assert!(store.list_keys("").await.is_err());
}

#[cfg(unix)]
#[test]
fn an_identity_file_readable_by_others_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, config) = initialised_store();
    let identity: &Path = config.identity_file.as_ref().unwrap();

    // Loosen it, as an over-eager `chmod -R` or a bad `tar` restore would.
    std::fs::set_permissions(identity, std::fs::Permissions::from_mode(0o644)).unwrap();

    let Err(err) = open_store_with(&config) else {
        panic!("a world-readable private key must not be silently trusted");
    };

    assert!(
        matches!(err, StoreError::InsecurePermissions { mode: 0o644, .. }),
        "expected InsecurePermissions, got {:?}",
        err
    );
    assert!(err.to_string().contains("chmod 600"), "{}", err);
}

#[cfg(unix)]
#[test]
fn a_new_store_is_created_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let (_dir, config) = initialised_store();

    for path in [
        config.identity_file.as_ref().unwrap(),
        config.secrets_file.as_ref().unwrap(),
    ] {
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} was created {:o}", path.display(), mode);
    }
}

#[test]
fn an_uninitialised_store_says_what_to_run() {
    let dir = TempDir::new().unwrap();
    let config = StoreConfig {
        backend: Some(StoreBackend::EncryptedFile),
        identity_file: Some(dir.path().join("age-identity.key")),
        secrets_file: Some(dir.path().join("secrets.age")),
        ..StoreConfig::default()
    };

    let Err(err) = open_store_with(&config) else {
        panic!("there is no identity here, so this must not open");
    };

    let message = err.to_string();
    assert!(message.contains("sigilforge store init"), "{}", message);
}

#[test]
fn the_secrets_file_is_a_real_age_file_readable_by_the_age_cli() {
    // Interoperability is the escape hatch: if Sigilforge is unavailable, the
    // operator can still recover their secrets with `age -d -i <identity>`.
    let (_dir, config) = initialised_store();
    let bytes = std::fs::read(config.secrets_file.as_ref().unwrap()).unwrap();

    assert!(
        bytes.starts_with(b"age-encryption.org/v1"),
        "expected an age v1 header"
    );
}

#[tokio::test]
async fn secret_values_never_appear_in_the_ciphertext() {
    let (_dir, config) = initialised_store();
    let store = open_store_with(&config).unwrap();

    let needle = "ghs_thisisnotarealtokenatall";
    store.set("k", &Secret::new(needle)).await.unwrap();

    let bytes = std::fs::read(config.secrets_file.as_ref().unwrap()).unwrap();
    assert!(
        !bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes()),
        "the secret value was found in the store file in the clear"
    );
}

#[cfg(feature = "github-app")]
#[tokio::test]
async fn a_github_app_registration_survives_a_process_boundary() {
    use sigilforge_core::{
        AccountId,
        github_app::{GitHubAppCredential, GitHubAppTokenManager, test_support},
    };

    let (_dir, config) = initialised_store();
    let account = AccountId::new("raibid-labs");
    let credential =
        GitHubAppCredential::new(1234567, 89012345, test_support::TEST_PRIVATE_KEY_PEM).unwrap();

    // `sigilforge github-app register`
    {
        let manager = GitHubAppTokenManager::new(open_store_with(&config).unwrap());
        manager.register(&account, &credential).await.unwrap();
    }

    // `sigilforge github-app list`, in a separate invocation
    let manager = GitHubAppTokenManager::new(open_store_with(&config).unwrap());
    assert!(
        manager.is_registered(&account).await.unwrap(),
        "the App must still be registered in the next process"
    );

    // `sigilforge github-app argocd-secret`, which needs the key itself
    let loaded = manager.load_credential(&account).await.unwrap();
    assert_eq!(loaded.app_id(), 1234567);
    assert_eq!(loaded.installation_id(), 89012345);
    assert_eq!(
        loaded.private_key_pem().expose(),
        test_support::TEST_PRIVATE_KEY_PEM,
        "the PEM must come back byte for byte, or nothing can sign a JWT"
    );
}

#[test]
fn default_paths_keep_the_key_away_from_the_ciphertext() {
    let identity: PathBuf = match sigilforge_core::store::default_identity_path() {
        Ok(path) => path,
        Err(_) => return, // no home directory in this environment
    };
    let secrets = sigilforge_core::store::default_secrets_path().unwrap();

    assert_ne!(
        identity.parent(),
        secrets.parent(),
        "a key stored beside the ciphertext it unlocks is not much of a key"
    );
}
