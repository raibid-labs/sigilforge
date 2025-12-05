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
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::FmtSubscriber;

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

    /// Start the daemon in foreground (for debugging)
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        FmtSubscriber::builder()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    match cli.command {
        Commands::AddAccount { service, account, scopes } => {
            add_account(&service, &account, scopes.as_deref()).await
        }
        Commands::ListAccounts { service } => {
            list_accounts(service.as_deref()).await
        }
        Commands::GetToken { service, account, format } => {
            get_token(&service, &account, &format).await
        }
        Commands::RemoveAccount { service, account, force } => {
            remove_account(&service, &account, force).await
        }
        Commands::Resolve { reference } => {
            resolve_reference(&reference).await
        }
        Commands::Daemon => {
            run_daemon_foreground().await
        }
    }
}

async fn add_account(service: &str, account: &str, scopes: Option<&str>) -> Result<()> {
    println!("Adding account {}/{}", service, account);
    if let Some(scopes) = scopes {
        println!("  Scopes: {}", scopes);
    }
    println!("  [stub] Would start OAuth flow here");
    Ok(())
}

async fn list_accounts(service_filter: Option<&str>) -> Result<()> {
    println!("Configured accounts:");
    if let Some(service) = service_filter {
        println!("  (filtered by service: {})", service);
    }
    println!("  [stub] No accounts configured yet");
    Ok(())
}

async fn get_token(service: &str, account: &str, format: &str) -> Result<()> {
    println!("[stub] Getting token for {}/{}", service, account);

    match format {
        "json" => {
            println!(r#"{{"service": "{}", "account": "{}", "token": "[stub]"}}"#, service, account);
        }
        _ => {
            println!("[stub] token-would-appear-here");
        }
    }
    Ok(())
}

async fn remove_account(service: &str, account: &str, force: bool) -> Result<()> {
    if !force {
        println!("Remove account {}/{}? [y/N]", service, account);
        println!("[stub] Would prompt for confirmation");
    }
    println!("[stub] Account removed");
    Ok(())
}

async fn resolve_reference(reference: &str) -> Result<()> {
    use sigilforge_core::CredentialRef;

    match CredentialRef::from_auth_uri(reference) {
        Ok(cred_ref) => {
            println!("Parsed reference:");
            println!("  Service: {}", cred_ref.service);
            println!("  Account: {}", cred_ref.account);
            println!("  Type: {}", cred_ref.credential_type);
            println!("[stub] Would resolve to actual value");
        }
        Err(e) => {
            eprintln!("Failed to parse reference: {}", e);
        }
    }
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
