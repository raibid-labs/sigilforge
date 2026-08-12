//! Sigilforge CLI
//!
//! Command-line interface for managing credentials in Sigilforge.
//!
//! # Usage
//!
//! ```bash
//! # Add a new account (starts OAuth flow)
//! sigilforge add-account spotify personal
//!
//! # List all configured accounts
//! sigilforge list-accounts
//!
//! # Get a fresh access token
//! sigilforge get-token spotify personal
//!
//! # Register a GitHub App and print an installation token
//! sigilforge github-app register raibid-labs \
//!     --app-id 1234567 --installation-id 89012345 --key-file app.private-key.pem
//! sigilforge github-app token raibid-labs
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use sigilforge_core::{
    AccountId, CredentialType, ServiceId,
    account_store::AccountStore,
    oauth::pkce::PkceFlow,
    provider::ProviderRegistry,
    store::{SecretStore, open_store},
};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

mod client;
mod github_app;
mod store_cmd;

/// Open the secret store, or fail with an error a human can act on.
///
/// Every direct-mode command goes through this. It exists because the previous
/// shape - `KeyringStore::try_new(...)` with a `MemoryStore` fallback - turned
/// "storage is unreachable" into "you have no credentials", which reads exactly
/// like data loss. See `sigilforge store status`.
fn secret_store() -> Result<Box<dyn SecretStore>> {
    open_store().map_err(|e| {
        anyhow::anyhow!(
            "{e}\n\nSecret storage is unavailable, so Sigilforge cannot tell whether a \
             credential exists. Run `sigilforge store status` to see the backends, or \
             `sigilforge store init` to set up encrypted file storage on a host with no \
             desktop keyring."
        )
    })
}

/// Warn when account metadata is readable but the secrets behind it are not.
///
/// `accounts.json` is plain JSON on disk; the credentials are not. Listing the
/// first while the second is unreachable would show a healthy-looking inventory
/// of tokens that cannot be read.
fn warn_if_storage_unavailable() {
    if let Err(e) = open_store() {
        eprintln!(
            "warning: secret storage is unavailable, so the credentials behind these \
             accounts cannot be read.\n{}\nRun `sigilforge store status` for detail.",
            e
        );
    }
}

#[derive(Parser)]
#[command(name = "sigilforge")]
#[command(about = "Credential management for the raibid-labs ecosystem")]
#[command(version)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new account for a service
    AddAccount {
        /// Service name (e.g., spotify, gmail, github)
        service: String,

        /// Account identifier (e.g., personal, work)
        account: String,

        /// OAuth scopes to request (comma-separated)
        #[arg(short, long)]
        scopes: Option<String>,
    },

    /// List all configured accounts
    ListAccounts {
        /// Filter by service name
        #[arg(short, long)]
        service: Option<String>,
    },

    /// Get a fresh access token for an account
    GetToken {
        /// Service name
        service: String,

        /// Account identifier
        account: String,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Remove an account and its credentials
    RemoveAccount {
        /// Service name
        service: String,

        /// Account identifier
        account: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Resolve a credential reference
    Resolve {
        /// Reference to resolve (e.g., auth://spotify/personal/token)
        reference: String,
    },

    /// Manage GitHub App credentials (machine access to private repos)
    GithubApp {
        #[command(subcommand)]
        command: github_app::GithubAppCommands,
    },

    /// Set up and inspect secret storage (start here on a headless host)
    Store {
        #[command(subcommand)]
        command: store_cmd::StoreCommands,
    },

    /// Start the daemon in foreground (for debugging)
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    init_logging(cli.verbose);

    match cli.command {
        Commands::AddAccount {
            service,
            account,
            scopes,
        } => add_account(&service, &account, scopes.as_deref()).await,
        Commands::ListAccounts { service } => list_accounts(service.as_deref()).await,
        Commands::GetToken {
            service,
            account,
            format,
        } => get_token(&service, &account, &format).await,
        Commands::RemoveAccount {
            service,
            account,
            force,
        } => remove_account(&service, &account, force).await,
        Commands::Resolve { reference } => resolve_reference(&reference).await,
        Commands::GithubApp { command } => github_app::run(command).await,
        Commands::Store { command } => store_cmd::run(command),
        Commands::Daemon => run_daemon_foreground().await,
    }
}

fn init_logging(verbose: bool) {
    let default_level = if verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    fmt().with_env_filter(filter).with_target(false).init();
}

async fn add_account(service: &str, account: &str, scopes: Option<&str>) -> Result<()> {
    let mut client = client::DaemonClient::connect_default().await?;

    if client.is_connected() {
        let scope_vec = scopes
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();

        match client.add_account(service, account, scope_vec).await {
            Ok(response) => {
                println!("{}", response.message);
                Ok(())
            }
            Err(e) => {
                warn!("Daemon call failed: {}", e);
                fallback_add_account(service, account, scopes).await
            }
        }
    } else {
        warn!("Daemon not available, using fallback mode");
        fallback_add_account(service, account, scopes).await
    }
}

async fn fallback_add_account(service: &str, account: &str, scopes: Option<&str>) -> Result<()> {
    use sigilforge_core::{Account, AccountId, AccountStore, ServiceId};

    // Get provider configuration
    let registry = ProviderRegistry::with_defaults();
    let provider = registry.get(service).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown provider '{}'. Available: {:?}",
            service,
            registry.list_ids()
        )
    })?;

    // Parse scopes
    let scope_list: Vec<String> = if let Some(scopes) = scopes {
        scopes.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        provider.default_scopes.clone()
    };

    // Get OAuth client credentials from environment or config
    let client_id = std::env::var(format!("{}_CLIENT_ID", service.to_uppercase()))
        .or_else(|_| std::env::var("OAUTH_CLIENT_ID"))
        .map_err(|_| {
            anyhow::anyhow!(
                "Missing OAuth client ID. Set {}_CLIENT_ID or OAUTH_CLIENT_ID environment variable",
                service.to_uppercase()
            )
        })?;

    let client_secret = std::env::var(format!("{}_CLIENT_SECRET", service.to_uppercase()))
        .or_else(|_| std::env::var("OAUTH_CLIENT_SECRET"))
        .ok();

    // Setup OAuth callback port
    let callback_port: u16 = std::env::var("OAUTH_CALLBACK_PORT")
        .unwrap_or_else(|_| "8484".to_string())
        .parse()
        .unwrap_or(8484);
    let redirect_uri = format!("http://127.0.0.1:{}/callback", callback_port);

    println!("Starting OAuth flow for {}/{}...", service, account);
    println!("  Provider: {}", provider.name);
    println!("  Scopes: {}", scope_list.join(", "));

    // Create PKCE flow
    let flow = PkceFlow::new(provider.clone(), client_id, client_secret, redirect_uri)?;

    // Build authorization URL
    let (auth_url, csrf_state) = flow.build_authorization_url(scope_list.clone());

    println!("\nPlease visit this URL to authorize:");
    println!("\n  {}\n", auth_url);

    // Try to open browser automatically
    if let Err(e) = open_browser(&auth_url) {
        info!("Could not open browser automatically: {}", e);
        println!("(Could not open browser automatically - please copy the URL above)");
    } else {
        println!("(Browser should open automatically)");
    }

    println!("\nWaiting for authorization on port {}...", callback_port);

    // Listen for callback
    let auth_code = flow.listen_for_callback(callback_port, &csrf_state).await?;

    println!("Authorization received! Exchanging code for tokens...");

    // Exchange code for tokens
    let token_set = flow.exchange_code(auth_code).await?;

    // Store the tokens. If storage is unavailable this fails here rather than
    // "succeeding" into a store that evaporates - the user just completed a
    // browser flow, and a silent no-op would waste it without saying so.
    let store = secret_store()?;

    // Store access token
    let access_key = format!("sigilforge/{}/{}/access_token", service, account);
    let access_secret =
        sigilforge_core::store::Secret::new(token_set.access_token.access_token.expose());
    store.set(&access_key, &access_secret).await?;

    // Store refresh token if available
    if let Some(ref refresh) = token_set.refresh_token {
        let refresh_key = format!("sigilforge/{}/{}/refresh_token", service, account);
        let refresh_secret = sigilforge_core::store::Secret::new(refresh.expose());
        store.set(&refresh_key, &refresh_secret).await?;
    }

    // Store expiry if available
    if let Some(expiry) = token_set.access_token.expires_at {
        let expiry_key = format!("sigilforge/{}/{}/token_expiry", service, account);
        let expiry_secret = sigilforge_core::store::Secret::new(expiry.to_rfc3339());
        store.set(&expiry_key, &expiry_secret).await?;
    }

    // Store scopes
    let scopes_key = format!("sigilforge/{}/{}/scopes", service, account);
    let scopes_secret = sigilforge_core::store::Secret::new(scope_list.join(","));
    store.set(&scopes_key, &scopes_secret).await?;

    // Save account to account store
    let account_store = AccountStore::load()?;
    let service_id = ServiceId::new(service);
    let account_id = AccountId::new(account);
    let new_account = Account::new(service_id, account_id, scope_list);
    account_store.add_account(new_account)?;

    println!("\nSuccess! Account {}/{} configured.", service, account);
    println!("  Tokens stored in the configured secret store");
    if token_set.refresh_token.is_some() {
        println!("  Refresh token: stored (will auto-renew)");
    }
    if let Some(expiry) = token_set.access_token.expires_at {
        println!("  Token expires: {}", expiry);
    }

    Ok(())
}

/// Try to open a URL in the default browser
fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()?;
    }
    Ok(())
}

async fn list_accounts(service_filter: Option<&str>) -> Result<()> {
    let mut client = client::DaemonClient::connect_default().await?;

    if client.is_connected() {
        match client.list_accounts(service_filter).await {
            Ok(response) => {
                if response.accounts.is_empty() {
                    println!("No accounts configured");
                } else {
                    println!("Configured accounts:");
                    for account in response.accounts {
                        println!("  {}/{}", account.service, account.account);
                        println!("    Scopes: {}", account.scopes.join(", "));
                        println!("    Created: {}", account.created_at);
                        if let Some(last_used) = account.last_used {
                            println!("    Last used: {}", last_used);
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                warn!("Daemon call failed: {}", e);
                fallback_list_accounts(service_filter).await
            }
        }
    } else {
        warn!("Daemon not available, using fallback mode");
        fallback_list_accounts(service_filter).await
    }
}

async fn fallback_list_accounts(service_filter: Option<&str>) -> Result<()> {
    use sigilforge_core::{AccountStore, ServiceId};

    let store = AccountStore::load()?;

    let filter = service_filter.map(ServiceId::new);
    let accounts = store.list_accounts(filter.as_ref())?;

    // This list comes from accounts.json, not from the secret store, so it can
    // look healthy while every credential behind it is unreachable. Say so.
    warn_if_storage_unavailable();

    if accounts.is_empty() {
        println!("No accounts configured");
        if let Some(service) = service_filter {
            println!("  (filtered by service: {})", service);
        }
        return Ok(());
    }

    println!("Configured accounts:");
    for account in accounts {
        println!("  {}/{}", account.service, account.id);
        if !account.scopes.is_empty() {
            println!("    Scopes: {}", account.scopes.join(", "));
        }
        println!("    Created: {}", account.created_at);
        if let Some(last_used) = account.last_used {
            println!("    Last used: {}", last_used);
        }
    }

    Ok(())
}

async fn get_token(service: &str, account: &str, format: &str) -> Result<()> {
    let mut client = client::DaemonClient::connect_default().await?;

    if client.is_connected() {
        match client.get_token(service, account).await {
            Ok(response) => {
                match format {
                    "json" => {
                        let json_output = serde_json::json!({
                            "service": service,
                            "account": account,
                            "token": response.token,
                            "expires_at": response.expires_at,
                        });
                        println!("{}", serde_json::to_string_pretty(&json_output)?);
                    }
                    _ => {
                        println!("{}", response.token);
                    }
                }
                Ok(())
            }
            Err(e) => {
                warn!("Daemon call failed: {}", e);
                fallback_get_token(service, account, format).await
            }
        }
    } else {
        warn!("Daemon not available, using fallback mode");
        fallback_get_token(service, account, format).await
    }
}

async fn fallback_get_token(service: &str, account: &str, format: &str) -> Result<()> {
    // Opening the store proves it is readable, so a `None` below really does
    // mean "no such token" and not "could not look".
    let store = secret_store()?;

    // Try to get access token
    let access_key = format!("sigilforge/{}/{}/access_token", service, account);
    let token = match store.get(&access_key).await? {
        Some(secret) => secret.expose().to_string(),
        None => {
            return Err(anyhow::anyhow!(
                "No token found for {}/{}. Run 'sigilforge add-account {} {}' first.",
                service,
                account,
                service,
                account
            ));
        }
    };

    // Get expiry if available
    let expiry_key = format!("sigilforge/{}/{}/token_expiry", service, account);
    let expires_at: Option<chrono::DateTime<chrono::Utc>> = match store.get(&expiry_key).await? {
        Some(secret) => chrono::DateTime::parse_from_rfc3339(secret.expose())
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        None => None,
    };

    // Check if token is expired
    if let Some(expiry) = expires_at {
        let now = chrono::Utc::now();
        if expiry < now {
            // Token is expired, try to refresh
            let refresh_key = format!("sigilforge/{}/{}/refresh_token", service, account);
            if store.get(&refresh_key).await?.is_some() {
                // TODO: Implement token refresh using refresh_token
                // For now, warn user to re-authenticate
                warn!("Token expired. Refresh not yet implemented.");
                eprintln!(
                    "Warning: Token expired at {}. Run 'sigilforge add-account {} {}' to re-authenticate.",
                    expiry, service, account
                );
            } else {
                return Err(anyhow::anyhow!(
                    "Token expired at {} and no refresh token available. Run 'sigilforge add-account {} {}' to re-authenticate.",
                    expiry,
                    service,
                    account
                ));
            }
        }
    }

    match format {
        "json" => {
            let json_output = serde_json::json!({
                "service": service,
                "account": account,
                "token": token,
                "expires_at": expires_at.map(|e| e.to_rfc3339()),
            });
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        }
        _ => {
            println!("{}", token);
        }
    }

    Ok(())
}

async fn remove_account(service: &str, account: &str, force: bool) -> Result<()> {
    use std::io::{self, Write};

    let store = AccountStore::load()?;
    let service_id = ServiceId::new(service);
    let account_id = AccountId::new(account);

    // Verify account exists before prompting
    let account_entry = store.get_account(&service_id, &account_id)?;
    if account_entry.is_none() {
        eprintln!("Error: Account {}/{} not found", service, account);
        std::process::exit(1);
    }

    // Prompt for confirmation unless --force is used
    if !force {
        print!("Remove account {}/{}? [y/N] ", service, account);
        io::stdout().flush()?;

        let mut response = String::new();
        io::stdin().read_line(&mut response)?;

        let confirmed = response.trim().eq_ignore_ascii_case("y")
            || response.trim().eq_ignore_ascii_case("yes");

        if !confirmed {
            println!("Cancelled");
            return Ok(());
        }
    }

    // Remove account from store
    store.remove_account(&service_id, &account_id)?;

    delete_account_secrets(service, account).await?;

    println!("Account {}/{} removed successfully", service, account);
    println!("  Associated secrets removed from configured secret store");

    Ok(())
}

async fn delete_account_secrets(service: &str, account: &str) -> Result<()> {
    // Deleting from a MemoryStore is a no-op that prints "removed successfully"
    // while the real secrets stay where they are. Fail instead.
    let store = secret_store()?;

    // Common credential types to clean up
    let credential_types = [
        CredentialType::AccessToken,
        CredentialType::RefreshToken,
        CredentialType::TokenExpiry,
        CredentialType::ApiKey,
        CredentialType::ClientId,
        CredentialType::ClientSecret,
        CredentialType::TokenScopes,
        // GitHub App registrations live under the same key convention.
        CredentialType::AppId,
        CredentialType::InstallationId,
        CredentialType::PrivateKey,
        CredentialType::InstallationToken,
    ];

    for cred_type in &credential_types {
        let key = format!("sigilforge/{}/{}/{}", service, account, cred_type);
        // Ignore errors - the key might not exist
        let _ = store.delete(&key).await;
    }

    Ok(())
}

async fn resolve_reference(reference: &str) -> Result<()> {
    let mut client = client::DaemonClient::connect_default().await?;

    if client.is_connected() {
        match client.resolve(reference).await {
            Ok(response) => {
                println!("{}", response.value);
                Ok(())
            }
            Err(e) => {
                warn!("Daemon call failed: {}", e);
                fallback_resolve_reference(reference).await
            }
        }
    } else {
        warn!("Daemon not available, using fallback mode");
        fallback_resolve_reference(reference).await
    }
}

async fn fallback_resolve_reference(reference: &str) -> Result<()> {
    use sigilforge_core::CredentialRef;
    use sigilforge_core::resolve::{ReferenceResolver, ResolvedValue};

    let cred_ref = CredentialRef::from_auth_uri(reference)
        .map_err(|e| anyhow::anyhow!("Failed to parse reference '{}': {}", reference, e))?;

    // GitHub App references are minted, not looked up: a raw store read would
    // return the cached token even when it is about to expire.
    if cred_ref.service.as_str() == sigilforge_core::GITHUB_APP_SERVICE {
        let manager = sigilforge_core::GitHubAppTokenManager::new(secret_store()?);

        let value = manager.resolve_ref(&cred_ref).await?;
        match value {
            ResolvedValue::Multiple(_) => {
                anyhow::bail!("Reference '{}' resolved to multiple values", reference)
            }
            other => println!("{}", other.expose()),
        }

        return Ok(());
    }

    let store = secret_store()?;

    // Build the key based on credential type
    let key = format!(
        "sigilforge/{}/{}/{}",
        cred_ref.service, cred_ref.account, cred_ref.credential_type
    );

    // Retrieve the value
    let value = match store.get(&key).await? {
        Some(secret) => secret.expose().to_string(),
        None => {
            return Err(anyhow::anyhow!(
                "Credential not found: {}. Run 'sigilforge add-account {} {}' first.",
                reference,
                cred_ref.service,
                cred_ref.account
            ));
        }
    };

    println!("{}", value);
    Ok(())
}

async fn run_daemon_foreground() -> Result<()> {
    println!("[stub] Running daemon in foreground...");
    println!("Press Ctrl+C to stop");

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        info!("Daemon heartbeat");
    }
}
