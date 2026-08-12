//! `sigilforge github-app` - GitHub App registration and installation tokens.
//!
//! A GitHub App is the right credential for machine access to private
//! repositories: org-owned, scoped to named repos, no browser, and tokens that
//! expire in an hour. See `docs/GITHUB_APP.md` for the one-time browser setup a
//! human has to do before any of these commands are useful.
//!
//! ```bash
//! # once, after creating the App on github.com
//! sigilforge github-app register raibid-labs \
//!     --app-id 1234567 --installation-id 89012345 \
//!     --key-file ~/Downloads/raibid-labs.2026-01-01.private-key.pem
//!
//! sigilforge github-app list
//! sigilforge github-app token raibid-labs
//!
//! sigilforge github-app argocd-secret raibid-labs \
//!     --repo-url https://github.com/raibid-labs/raibid-fish.git \
//!   | kubectl apply -f -
//! ```
//!
//! These commands talk to `sigilforge-core` directly rather than through the
//! daemon: registration writes to the configured secret store, and minting needs
//! the private key, so there is nothing the daemon would add.
//!
//! On a host with no D-Bus session bus - a server over SSH - run
//! `sigilforge store init` once first. Without a working store these commands
//! fail loudly rather than reporting an App as unregistered.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use sigilforge_core::{
    Account, AccountId, AccountStore, ServiceId,
    account_store::AccountStoreError,
    github_app::{
        ArgoCdRepositorySecret, GITHUB_APP_SERVICE, GitHubAppCredential, GitHubAppTokenManager,
    },
    store::{SecretStore, open_store},
};

/// Subcommands under `sigilforge github-app`.
#[derive(Subcommand)]
pub enum GithubAppCommands {
    /// Register a GitHub App installation and store its private key
    Register {
        /// Account identifier, conventionally the org the App is installed on
        account: String,

        /// The App ID from the App's settings page
        #[arg(long, value_name = "ID")]
        app_id: u64,

        /// The installation ID, the last path segment of the installation URL
        #[arg(long, value_name = "ID")]
        installation_id: u64,

        /// PEM private key file; omit (or pass "-") to read the key from stdin
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,
    },

    /// List registered GitHub Apps
    List,

    /// Print a current installation token, minting one if needed
    Token {
        /// Account identifier
        account: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Mint a new token even if the cached one is still valid
        #[arg(long)]
        force: bool,
    },

    /// Emit an Argo CD repository Secret manifest to stdout
    ///
    /// The manifest contains the App's private key. Pipe it to
    /// `kubectl apply -f -`, or encrypt it with SOPS or sealed-secrets. Do not
    /// commit it.
    ArgocdSecret {
        /// Account identifier
        account: String,

        /// Repository URL the credential applies to
        #[arg(long, value_name = "URL")]
        repo_url: String,

        /// Secret name (default: derived from the repository URL)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,

        /// Namespace Argo CD watches
        #[arg(long, default_value = "argocd", value_name = "NAMESPACE")]
        namespace: String,

        /// Restrict the credential to a single Argo CD project
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
    },

    /// Remove a registered GitHub App and its cached token
    Remove {
        /// Account identifier
        account: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

/// Dispatch a `github-app` subcommand.
pub async fn run(command: GithubAppCommands) -> Result<()> {
    match command {
        GithubAppCommands::Register {
            account,
            app_id,
            installation_id,
            key_file,
        } => register(&account, app_id, installation_id, key_file.as_deref()).await,
        GithubAppCommands::List => list().await,
        GithubAppCommands::Token {
            account,
            format,
            force,
        } => token(&account, &format, force).await,
        GithubAppCommands::ArgocdSecret {
            account,
            repo_url,
            name,
            namespace,
            project,
        } => {
            argocd_secret(
                &account,
                &repo_url,
                name.as_deref(),
                &namespace,
                project.as_deref(),
            )
            .await
        }
        GithubAppCommands::Remove { account, force } => remove(&account, force).await,
    }
}

/// Build a manager over the configured secret store.
///
/// The store is opened and probed here, so this fails on a machine with no
/// working storage instead of a registration vanishing at process exit and
/// failing an hour later, in a cluster, with no key.
///
/// Never falls back to [`MemoryStore`], and never falls back from one persistent
/// backend to another: a key written to the keyring is not in the encrypted
/// file, and silently reading the wrong one is how "no GitHub Apps registered"
/// gets printed at a host that has one.
///
/// [`MemoryStore`]: sigilforge_core::MemoryStore
fn manager() -> Result<GitHubAppTokenManager<Box<dyn SecretStore>>> {
    let store = open_store().map_err(|e| {
        anyhow::anyhow!(
            "{e}\n\nGitHub App private keys must persist, so falling back to in-memory \
             storage would be worse than failing here. On a host with no desktop \
             keyring, run `sigilforge store init` once to set up encrypted file storage."
        )
    })?;

    Ok(GitHubAppTokenManager::new(store))
}

async fn register(
    account: &str,
    app_id: u64,
    installation_id: u64,
    key_file: Option<&std::path::Path>,
) -> Result<()> {
    let pem = read_private_key(key_file)?;

    let credential = GitHubAppCredential::new(app_id, installation_id, pem)
        .context("The private key could not be read as an RSA key")?;

    let manager = manager()?;
    let account_id = AccountId::new(account);

    manager
        .register(&account_id, &credential)
        .await
        .context("Failed to store the GitHub App registration")?;

    record_account(&account_id)?;

    println!(
        "Registered GitHub App for {}/{}",
        GITHUB_APP_SERVICE, account
    );
    println!("  App ID:          {}", app_id);
    println!("  Installation ID: {}", installation_id);
    println!("  Private key:     stored in the configured secret store");
    println!();
    println!(
        "  Reference it as: auth://{}/{}/installation_token",
        GITHUB_APP_SERVICE, account
    );

    Ok(())
}

/// Read the PEM from a file, or from stdin when no path is given.
fn read_private_key(key_file: Option<&std::path::Path>) -> Result<String> {
    match key_file {
        Some(path) if path != std::path::Path::new("-") => std::fs::read_to_string(path)
            .with_context(|| format!("Could not read private key from {}", path.display())),
        _ => {
            if std::io::stdin().is_terminal() {
                eprintln!("Reading PEM private key from stdin; end with Ctrl-D.");
            }

            let mut pem = String::new();
            std::io::stdin()
                .read_to_string(&mut pem)
                .context("Could not read private key from stdin")?;

            if pem.trim().is_empty() {
                anyhow::bail!(
                    "No private key on stdin. Pass --key-file <PATH>, or pipe the PEM in."
                );
            }

            Ok(pem)
        }
    }
}

/// Record the account in accounts.json so `list-accounts` sees it too.
///
/// The keyring cannot enumerate keys, so this file is the only way to know which
/// GitHub Apps are registered.
fn record_account(account: &AccountId) -> Result<()> {
    let store = AccountStore::load()?;
    let service = ServiceId::new(GITHUB_APP_SERVICE);

    match store.add_account(Account::new(service, account.clone(), Vec::new())) {
        Ok(()) => Ok(()),
        // Re-registering an existing App is a normal operation (key rotation).
        Err(AccountStoreError::AlreadyExists { .. }) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn list() -> Result<()> {
    // Open storage *first*. Reporting "No GitHub Apps registered" without having
    // looked is the bug this ordering exists to prevent: on a host with no
    // D-Bus session that message was printed while a perfectly good
    // registration sat in a store nobody could open.
    let manager = manager()?;

    let accounts =
        AccountStore::load()?.list_accounts(Some(&ServiceId::new(GITHUB_APP_SERVICE)))?;

    if accounts.is_empty() {
        println!("No GitHub Apps registered");
        println!("  Register one with: sigilforge github-app register <account> \\");
        println!("      --app-id <ID> --installation-id <ID> --key-file <PATH>");
        return Ok(());
    }

    println!("Registered GitHub Apps:");
    for account in accounts {
        println!("  {}/{}", GITHUB_APP_SERVICE, account.id);

        match manager.load_credential(&account.id).await {
            Ok(credential) => {
                println!("    App ID:          {}", credential.app_id());
                println!("    Installation ID: {}", credential.installation_id());
            }
            Err(e) => {
                // accounts.json and the keyring can drift - say so rather than
                // pretending the registration is intact.
                println!("    Registration unreadable: {}", e);
            }
        }

        match manager.cached_token(&account.id).await {
            Ok(Some(token)) => match token.expires_at {
                Some(expires_at) => println!("    Cached token:    expires {}", expires_at),
                None => println!("    Cached token:    present, expiry unknown"),
            },
            Ok(None) => println!("    Cached token:    none"),
            Err(e) => println!("    Cached token:    unreadable ({})", e),
        }

        println!(
            "    Reference:       auth://{}/{}/installation_token",
            GITHUB_APP_SERVICE, account.id
        );
    }

    Ok(())
}

async fn token(account: &str, format: &str, force: bool) -> Result<()> {
    let manager = manager()?;
    let account_id = AccountId::new(account);

    let token = if force {
        manager.mint_installation_token(&account_id).await
    } else {
        manager.ensure_installation_token(&account_id).await
    }
    .with_context(|| format!("Could not obtain an installation token for {}", account))?;

    match format {
        "json" => {
            let output = serde_json::json!({
                "service": GITHUB_APP_SERVICE,
                "account": account,
                "token": token.access_token.expose(),
                "expires_at": token.expires_at.map(|e| e.to_rfc3339()),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => println!("{}", token.access_token.expose()),
    }

    Ok(())
}

async fn argocd_secret(
    account: &str,
    repo_url: &str,
    name: Option<&str>,
    namespace: &str,
    project: Option<&str>,
) -> Result<()> {
    let manager = manager()?;
    let credential = manager
        .load_credential(&AccountId::new(account))
        .await
        .with_context(|| format!("No GitHub App registered for '{}'", account))?;

    let mut secret =
        ArgoCdRepositorySecret::from_credential(repo_url, &credential)?.with_namespace(namespace);

    if let Some(name) = name {
        secret = secret.with_name(name);
    }
    if let Some(project) = project {
        secret = secret.with_project(project);
    }

    // The warning goes to stderr so it survives being piped into kubectl.
    eprintln!(
        "warning: this manifest contains a private key - pipe it to \
         `kubectl apply -f -` or encrypt it; do not commit it"
    );

    print!("{}", secret.render(credential.private_key_pem()));

    Ok(())
}

async fn remove(account: &str, force: bool) -> Result<()> {
    use std::io::Write;

    let manager = manager()?;
    let account_id = AccountId::new(account);

    if !manager.is_registered(&account_id).await? {
        anyhow::bail!("No GitHub App registered for '{}'", account);
    }

    if !force {
        print!(
            "Remove GitHub App registration {}/{}? [y/N] ",
            GITHUB_APP_SERVICE, account
        );
        std::io::stdout().flush()?;

        let mut response = String::new();
        std::io::stdin().read_line(&mut response)?;

        if !matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled");
            return Ok(());
        }
    }

    manager.remove(&account_id).await?;

    // accounts.json may not have an entry (registration predating it, or a
    // partial removal); missing is not an error here.
    let account_store = AccountStore::load()?;
    let _ = account_store.remove_account(&ServiceId::new(GITHUB_APP_SERVICE), &account_id);

    println!(
        "Removed GitHub App registration {}/{}",
        GITHUB_APP_SERVICE, account
    );

    Ok(())
}
