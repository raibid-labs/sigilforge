//! Application state management for Sigilforge TUI.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sigilforge_client::{SigilforgeClient, TokenProvider};
use std::time::Instant;
use tracing::{debug, warn};

/// Status of an OAuth account token
#[derive(Debug, Clone, PartialEq)]
pub enum TokenStatus {
    /// Token is valid and not expiring soon
    Valid,
    /// Token is valid but expires within 7 days
    ExpiringSoon,
    /// Token has expired
    Expired,
    /// Unable to determine status (daemon unavailable or error)
    Unknown,
}

/// Information about a configured OAuth account
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub service: String,
    pub account: String,
    pub scopes: Vec<String>,
    pub status: TokenStatus,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: String,
    pub last_used: Option<String>,
}

impl AccountInfo {
    /// Get a display string for expiration
    pub fn expiry_display(&self) -> String {
        match &self.expires_at {
            Some(expiry) => {
                let now = Utc::now();
                let duration = expiry.signed_duration_since(now);

                if duration < Duration::zero() {
                    format!("Expired: {} ago", format_duration(-duration))
                } else {
                    format!("Expires: {}", format_duration(duration))
                }
            }
            None => "No expiry".to_string(),
        }
    }

    /// Get status text
    pub fn status_text(&self) -> &'static str {
        match self.status {
            TokenStatus::Valid => "ACTIVE",
            TokenStatus::ExpiringSoon => "EXPIRING SOON",
            TokenStatus::Expired => "EXPIRED",
            TokenStatus::Unknown => "UNKNOWN",
        }
    }
}

/// Format a duration as a human-readable string
fn format_duration(duration: Duration) -> String {
    let days = duration.num_days();
    if days > 0 {
        format!("{} days", days)
    } else {
        let hours = duration.num_hours();
        if hours > 0 {
            format!("{} hours", hours)
        } else {
            let minutes = duration.num_minutes();
            format!("{} minutes", minutes)
        }
    }
}

/// Application state
pub struct App {
    /// Sigilforge client for daemon communication
    client: SigilforgeClient,
    /// List of accounts
    pub accounts: Vec<AccountInfo>,
    /// Currently selected account index
    pub selected: usize,
    /// Whether the daemon is available
    pub daemon_available: bool,
    /// Status message to display
    pub status_message: String,
    /// Last refresh time
    last_refresh: Instant,
    /// Auto-refresh interval (30 seconds)
    refresh_interval: std::time::Duration,
}

impl App {
    /// Create a new application instance
    pub async fn new() -> Result<Self> {
        let client = SigilforgeClient::new();

        // Check daemon availability
        let daemon_available = client.is_daemon_available().await;

        let mut app = Self {
            client,
            accounts: Vec::new(),
            selected: 0,
            daemon_available,
            status_message: if daemon_available {
                "Connected to Sigilforge daemon".to_string()
            } else {
                "WARNING: Sigilforge daemon is not available".to_string()
            },
            last_refresh: Instant::now(),
            refresh_interval: std::time::Duration::from_secs(30),
        };

        // Load initial account list
        app.load_accounts().await?;

        Ok(app)
    }

    /// Load accounts from the daemon
    async fn load_accounts(&mut self) -> Result<()> {
        if !self.daemon_available {
            self.status_message = "Cannot load accounts: daemon unavailable".to_string();
            return Ok(());
        }

        // Get list of accounts via daemon
        // Note: We need to add list_accounts support to sigilforge-client
        // For now, we'll make a direct RPC call
        match self.fetch_accounts_status().await {
            Ok(accounts) => {
                self.accounts = accounts;
                self.status_message = format!("Loaded {} accounts", self.accounts.len());

                // Ensure selection is valid
                if self.accounts.is_empty() {
                    self.selected = 0;
                } else if self.selected >= self.accounts.len() {
                    self.selected = self.accounts.len() - 1;
                }
            }
            Err(e) => {
                warn!("Failed to load accounts: {}", e);
                self.status_message = format!("Error loading accounts: {}", e);
            }
        }

        Ok(())
    }

    /// Fetch account status from daemon
    async fn fetch_accounts_status(&self) -> Result<Vec<AccountInfo>> {
        // This is a workaround until we add proper list_accounts to the client
        // For now, we'll use a hardcoded example for demonstration
        // In production, this would make a JSON-RPC call to list_accounts
        // and accounts_status endpoints

        // TODO: Implement proper daemon RPC call
        // For now, return empty list
        debug!("Fetching account status from daemon");
        Ok(vec![])
    }

    /// Refresh the selected account's token
    pub async fn refresh_selected(&mut self) -> Result<()> {
        if self.accounts.is_empty() {
            self.status_message = "No accounts to refresh".to_string();
            return Ok(());
        }

        let account = &self.accounts[self.selected];
        self.status_message = format!(
            "Refreshing {}/{}...",
            account.service, account.account
        );

        match self
            .client
            .ensure_token(&account.service, &account.account)
            .await
        {
            Ok(_) => {
                self.status_message = format!(
                    "Refreshed {}/{} successfully",
                    account.service, account.account
                );
                self.load_accounts().await?;
            }
            Err(e) => {
                self.status_message = format!("Failed to refresh token: {}", e);
            }
        }

        Ok(())
    }

    /// Refresh all accounts
    pub async fn refresh_all(&mut self) -> Result<()> {
        if self.accounts.is_empty() {
            self.status_message = "No accounts to refresh".to_string();
            return Ok(());
        }

        self.status_message = "Refreshing all accounts...".to_string();

        let mut success_count = 0;
        let mut error_count = 0;

        for account in &self.accounts {
            match self
                .client
                .ensure_token(&account.service, &account.account)
                .await
            {
                Ok(_) => success_count += 1,
                Err(_) => error_count += 1,
            }
        }

        self.status_message = format!(
            "Refreshed {} accounts, {} errors",
            success_count, error_count
        );
        self.load_accounts().await?;

        Ok(())
    }

    /// Select the next account
    pub fn select_next(&mut self) {
        if !self.accounts.is_empty() {
            self.selected = (self.selected + 1) % self.accounts.len();
        }
    }

    /// Select the previous account
    pub fn select_previous(&mut self) {
        if !self.accounts.is_empty() {
            self.selected = if self.selected == 0 {
                self.accounts.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Select the first account
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Select the last account
    pub fn select_last(&mut self) {
        if !self.accounts.is_empty() {
            self.selected = self.accounts.len() - 1;
        }
    }

    /// Get the currently selected account
    pub fn selected_account(&self) -> Option<&AccountInfo> {
        self.accounts.get(self.selected)
    }

    /// Periodic tick for background tasks
    pub async fn tick(&mut self) -> Result<()> {
        // Auto-refresh account list periodically
        if self.last_refresh.elapsed() >= self.refresh_interval {
            debug!("Auto-refreshing account list");
            self.load_accounts().await?;
            self.last_refresh = Instant::now();
        }

        Ok(())
    }
}
