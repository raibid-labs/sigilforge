//! `sigilforge store` - set up and inspect secret storage.
//!
//! Sigilforge keeps secrets in the OS keyring when there is one. On a server
//! reached over SSH there is not: the Secret Service needs a D-Bus **session**
//! bus and an unlocked keyring daemon, and a plain SSH login has neither. That
//! is what these commands are for.
//!
//! ```bash
//! # once, on a headless host
//! sigilforge store init
//!
//! # what is available, and why
//! sigilforge store status
//! ```
//!
//! `init` generates an [age](https://age-encryption.org) identity, writes it
//! `0600`, and creates an empty encrypted store. Everything afterwards -
//! `github-app register`, `add-account`, the daemon - picks it up automatically.

use anyhow::{Context, Result};
use clap::Subcommand;
use sigilforge_core::store::{
    BACKEND_ENV_VAR, EncryptedFileStore, IDENTITY_ENV_VAR, SECRETS_ENV_VAR, StoreBackend,
    StoreConfig, init_encrypted_store, open_store_with, probe_backends,
};

/// Subcommands under `sigilforge store`.
#[derive(Subcommand)]
pub enum StoreCommands {
    /// Set up encrypted file storage for a host with no keyring
    ///
    /// Generates an age identity (private key) and an empty encrypted store,
    /// both readable only by you. The identity is the only thing that can
    /// decrypt the store: back it up, because it cannot be regenerated.
    Init,

    /// Show which storage backend is in use, and what each one reports
    Status,
}

/// Dispatch a `store` subcommand.
pub fn run(command: StoreCommands) -> Result<()> {
    match command {
        StoreCommands::Init => init(),
        StoreCommands::Status => status(),
    }
}

fn init() -> Result<()> {
    let config = StoreConfig::load().context("Could not read the storage configuration")?;

    let outcome =
        init_encrypted_store(&config).context("Could not initialise the encrypted secret store")?;

    if outcome.created_identity {
        println!("Created an age identity for this host.");
    } else {
        println!("An age identity already exists; keeping it.");
        println!("(Generating a new one would orphan every secret already stored.)");
    }

    println!();
    println!(
        "  Identity (private key): {}",
        outcome.identity_path.display()
    );
    println!(
        "  Encrypted store:        {}",
        outcome.secrets_path.display()
    );
    println!("  Recipient (public key): {}", outcome.public_key);
    println!();
    println!("BACK UP THE IDENTITY FILE.");
    println!(
        "  It is the only key to {}. There is no escrow and no recovery:",
        outcome.secrets_path.display()
    );
    println!("  lose it and every secret in the store is unrecoverable.");
    println!("  Copy it somewhere offline - not next to the store, and not into git.");
    println!();

    // Prove the thing works before telling anyone it does.
    match open_store_with(&StoreConfig::for_backend(StoreBackend::EncryptedFile)) {
        Ok(_) => println!("Verified: the store encrypts and decrypts."),
        Err(e) => {
            // Only possible if config and environment disagree about the paths.
            eprintln!("warning: the store was created but is not usable: {}", e);
        }
    }

    println!();
    println!("Sigilforge will now use it automatically. To pin it explicitly:");
    println!("  export {}=encrypted-file", BACKEND_ENV_VAR);

    Ok(())
}

fn status() -> Result<()> {
    let config = StoreConfig::load().context("Could not read the storage configuration")?;

    // Probe once. Each probe writes and deletes a keyring sentinel or decrypts
    // the store, so there is no reason to do it three times for one report.
    let probes = probe_backends(&config);

    println!("Secret storage");
    println!();

    match config.backend {
        Some(backend) => {
            let usable = probes.iter().any(|p| p.backend == backend && p.available);
            println!(
                "  Selected:  {} (forced by {}){}",
                backend,
                config.source_description(),
                if usable { "" } else { " - NOT USABLE" }
            );
        }
        None => {
            // Mirrors automatic selection: the first persistent backend that works.
            let chosen = probes
                .iter()
                .find(|p| p.available && p.backend != StoreBackend::Memory);
            match chosen {
                Some(p) => println!("  Selected:  {} (auto)", p.backend),
                None => println!("  Selected:  none - no usable backend"),
            }
        }
    }

    println!();
    println!("Backends");
    for probe in &probes {
        println!(
            "  {:<15} {:<14} {}",
            probe.backend.to_string(),
            if probe.available {
                "available"
            } else {
                "unavailable"
            },
            probe.detail
        );
    }

    println!();
    println!("Paths");
    if let Some(path) = StoreConfig::default_config_path() {
        let exists = if path.exists() { "" } else { " (absent)" };
        println!("  config       {}{}", path.display(), exists);
    }
    match &config.identity_file {
        Some(path) => println!("  identity     {} (overridden)", path.display()),
        None => match sigilforge_core::store::default_identity_path() {
            Ok(path) => {
                let exists = if path.exists() { "" } else { " (absent)" };
                println!("  identity     {}{}", path.display(), exists);
            }
            Err(_) => println!("  identity     unavailable"),
        },
    }
    match &config.secrets_file {
        Some(path) => println!("  secrets      {} (overridden)", path.display()),
        None => match sigilforge_core::store::default_secrets_path() {
            Ok(path) => {
                let exists = if path.exists() { "" } else { " (absent)" };
                println!("  secrets      {}{}", path.display(), exists);
            }
            Err(_) => println!("  secrets      unavailable"),
        },
    }

    println!();
    println!("Overrides");
    println!("  {:<28} {}", BACKEND_ENV_VAR, env_display(BACKEND_ENV_VAR));
    println!(
        "  {:<28} {}",
        IDENTITY_ENV_VAR,
        env_display(IDENTITY_ENV_VAR)
    );
    println!("  {:<28} {}", SECRETS_ENV_VAR, env_display(SECRETS_ENV_VAR));

    if !EncryptedFileStore::is_initialized() && config.identity_file.is_none() {
        println!();
        println!("No encrypted store on this host. If it has no desktop keyring, run:");
        println!("  sigilforge store init");
    }

    Ok(())
}

fn env_display(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => "(unset)".to_string(),
    }
}
