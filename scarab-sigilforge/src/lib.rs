//! Scarab Plugin for Sigilforge OAuth Credential Management
//!
//! This plugin integrates Sigilforge with Scarab's status bar and menu system,
//! providing quick access to OAuth account management and displaying credential
//! status in the terminal's status bar.
//!
//! ## Features
//!
//! - Display account count and status in the status bar
//! - Color-coded status indicators (green for valid, red for issues)
//! - Warning icons for expiring tokens
//! - Menu integration for adding and managing accounts
//! - Support for Google, GitHub, and Spotify OAuth providers

mod status;

use async_trait::async_trait;
use scarab_plugin_api::{
    menu::{MenuAction, MenuItem},
    Plugin, PluginContext, PluginMetadata, Result as PluginResult,
};
use sigilforge_core::{AccountStore, AccountStoreError};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Account status information for display in the status bar
#[derive(Debug, Clone)]
struct AccountStatus {
    /// Service name (e.g., "google", "github", "spotify")
    service: String,
    /// Account identifier (e.g., "personal", "work")
    account: String,
    /// Whether the token is currently valid
    token_valid: bool,
    /// Whether the token expires soon (within 24 hours)
    expires_soon: bool,
}

/// Sigilforge plugin for OAuth credential management
pub struct SigilforgePlugin {
    /// Plugin metadata
    metadata: PluginMetadata,
    /// Account store for managing credentials
    account_store: Arc<RwLock<Option<AccountStore>>>,
    /// Cached account status for status bar rendering
    accounts: Arc<RwLock<Vec<AccountStatus>>>,
}

impl SigilforgePlugin {
    /// Create a new Sigilforge plugin instance
    pub fn new() -> Self {
        let metadata = PluginMetadata::new(
            "sigilforge",
            env!("CARGO_PKG_VERSION"),
            "OAuth credential management for terminal workflows",
            "raibid-labs",
        )
        .with_emoji("🔐")
        .with_color("#a6e3a1")
        .with_catchphrase("Secure credentials, seamless authentication");

        Self {
            metadata,
            account_store: Arc::new(RwLock::new(None)),
            accounts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load account status from the account store
    async fn refresh_account_status(&self) -> Result<(), AccountStoreError> {
        let store = self.account_store.read().await;

        if let Some(store) = store.as_ref() {
            let all_accounts = store.list_accounts(None)?;
            let mut status_list = Vec::new();

            for account in all_accounts {
                // For now, we'll mark all accounts as valid
                // In a full implementation, we would check token expiry
                // by querying the keyring store
                status_list.push(AccountStatus {
                    service: account.service.as_str().to_string(),
                    account: account.id.as_str().to_string(),
                    token_valid: true,
                    expires_soon: false,
                });
            }

            *self.accounts.write().await = status_list;
            info!("Refreshed account status: {} accounts loaded", self.accounts.read().await.len());
        }

        Ok(())
    }

    /// Handle adding a new account for a specific service
    async fn handle_add_account(&self, service: &str, _ctx: &PluginContext) -> PluginResult<()> {
        info!("Adding new {} account via Sigilforge", service);

        // In a full implementation, this would:
        // 1. Launch the OAuth flow via sigilforge-daemon
        // 2. Wait for the flow to complete
        // 3. Refresh the account status

        warn!("Add account functionality requires sigilforge-daemon integration");
        Ok(())
    }

    /// Handle listing all accounts
    async fn handle_list_accounts(&self, _ctx: &PluginContext) -> PluginResult<()> {
        let accounts = self.accounts.read().await;

        if accounts.is_empty() {
            info!("No accounts configured");
        } else {
            info!("Configured accounts:");
            for account in accounts.iter() {
                let status = if account.token_valid {
                    if account.expires_soon {
                        "⚠️  expiring soon"
                    } else {
                        "✓ valid"
                    }
                } else {
                    "✗ invalid"
                };
                info!("  {}/{} - {}", account.service, account.account, status);
            }
        }

        Ok(())
    }

    /// Handle removing an account
    async fn handle_remove_account(&self, _ctx: &PluginContext) -> PluginResult<()> {
        // In a full implementation, this would:
        // 1. Present a list of accounts to remove
        // 2. Call sigilforge-daemon to revoke tokens
        // 3. Remove from account store
        // 4. Refresh account status

        warn!("Remove account functionality requires interactive menu support");
        Ok(())
    }
}

impl Default for SigilforgePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SigilforgePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    async fn on_load(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
        info!("Loading Sigilforge plugin");

        // Load the account store
        match AccountStore::load() {
            Ok(store) => {
                *self.account_store.write().await = Some(store);
                info!("Account store loaded successfully");

                // Refresh account status
                if let Err(e) = self.refresh_account_status().await {
                    error!("Failed to refresh account status: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to load account store: {}", e);
                // Continue loading even if account store fails
                // This allows the plugin to function in a degraded state
            }
        }

        info!("Sigilforge plugin loaded");
        Ok(())
    }

    async fn on_unload(&mut self) -> PluginResult<()> {
        info!("Unloading Sigilforge plugin");
        Ok(())
    }

    fn get_menu(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::new(
                "Add Account",
                MenuAction::SubMenu(vec![
                    MenuItem::new(
                        "Google",
                        MenuAction::Remote("add_google".to_string()),
                    )
                    .with_icon("🔍"),
                    MenuItem::new(
                        "GitHub",
                        MenuAction::Remote("add_github".to_string()),
                    )
                    .with_icon("🐙"),
                    MenuItem::new(
                        "Spotify",
                        MenuAction::Remote("add_spotify".to_string()),
                    )
                    .with_icon("🎵"),
                ]),
            )
            .with_icon("➕"),
            MenuItem::new(
                "List Accounts",
                MenuAction::Remote("list_accounts".to_string()),
            )
            .with_icon("📋"),
            MenuItem::new(
                "Remove Account",
                MenuAction::Remote("remove_account".to_string()),
            )
            .with_icon("🗑️"),
        ]
    }

    async fn on_remote_command(&mut self, id: &str, ctx: &PluginContext) -> PluginResult<()> {
        match id {
            "add_google" => self.handle_add_account("google", ctx).await,
            "add_github" => self.handle_add_account("github", ctx).await,
            "add_spotify" => self.handle_add_account("spotify", ctx).await,
            "list_accounts" => self.handle_list_accounts(ctx).await,
            "remove_account" => self.handle_remove_account(ctx).await,
            _ => {
                warn!("Unknown remote command: {}", id);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_metadata() {
        let plugin = SigilforgePlugin::new();
        let metadata = plugin.metadata();

        assert_eq!(metadata.name, "sigilforge");
        assert_eq!(metadata.emoji, Some("🔐".to_string()));
        assert_eq!(metadata.color, Some("#a6e3a1".to_string()));
    }

    #[test]
    fn test_plugin_menu() {
        let plugin = SigilforgePlugin::new();
        let menu = plugin.get_menu();

        assert_eq!(menu.len(), 3);
        assert_eq!(menu[0].label, "Add Account");
        assert_eq!(menu[1].label, "List Accounts");
        assert_eq!(menu[2].label, "Remove Account");

        // Verify the "Add Account" submenu
        if let MenuAction::SubMenu(ref items) = menu[0].action {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].label, "Google");
            assert_eq!(items[1].label, "GitHub");
            assert_eq!(items[2].label, "Spotify");
        } else {
            panic!("Expected SubMenu action for Add Account");
        }
    }
}
