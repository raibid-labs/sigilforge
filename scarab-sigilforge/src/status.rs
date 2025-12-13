//! Status bar rendering for Sigilforge plugin
//!
//! This module provides status bar rendering logic for displaying account
//! status, including:
//! - Account count
//! - Color-coded status (green for valid, red for issues)
//! - Warning icons for expiring tokens

use scarab_plugin_api::status_bar::{Color, RenderItem};

use crate::AccountStatus;

/// Colors for status bar rendering (Catppuccin Mocha palette)
pub mod colors {
    /// Green color for valid tokens (#a6e3a1)
    pub const VALID: &str = "#a6e3a1";
    /// Red color for invalid tokens (#f38ba8)
    pub const INVALID: &str = "#f38ba8";
    /// Yellow color for expiring tokens (#f9e2af)
    pub const EXPIRING: &str = "#f9e2af";
}

/// Render status bar items for the Sigilforge plugin
///
/// Returns a vector of render items that display:
/// - Lock emoji (🔐)
/// - Account count
/// - Color-coded status
/// - Warning icon if any tokens are expiring soon
pub fn render_status_bar(accounts: &[AccountStatus]) -> Vec<RenderItem> {
    let mut items = Vec::new();

    // Determine overall status
    let has_invalid = accounts.iter().any(|a| !a.token_valid);
    let has_expiring = accounts.iter().any(|a| a.expires_soon);

    // Choose color based on status
    let color = if has_invalid {
        colors::INVALID
    } else if has_expiring {
        colors::EXPIRING
    } else {
        colors::VALID
    };

    // Add lock emoji
    items.push(RenderItem::Foreground(Color::Hex(color.to_string())));
    items.push(RenderItem::Text("🔐".to_string()));
    items.push(RenderItem::ResetForeground);

    // Add spacing
    items.push(RenderItem::Padding(1));

    // Add account count
    let count_text = if accounts.is_empty() {
        "no accounts".to_string()
    } else if accounts.len() == 1 {
        "1 account".to_string()
    } else {
        format!("{} accounts", accounts.len())
    };

    items.push(RenderItem::Foreground(Color::Hex(color.to_string())));
    items.push(RenderItem::Text(count_text));
    items.push(RenderItem::ResetForeground);

    // Add warning icon if tokens are expiring
    if has_expiring {
        items.push(RenderItem::Padding(1));
        items.push(RenderItem::Foreground(Color::Hex(
            colors::EXPIRING.to_string(),
        )));
        items.push(RenderItem::Text("⚠️".to_string()));
        items.push(RenderItem::ResetForeground);
    }

    items
}

/// Render a detailed status bar for debugging or verbose mode
///
/// Shows individual account statuses with service and account names
pub fn render_detailed_status(accounts: &[AccountStatus]) -> Vec<RenderItem> {
    let mut items = Vec::new();

    if accounts.is_empty() {
        items.push(RenderItem::Text("🔐 No accounts configured".to_string()));
        return items;
    }

    items.push(RenderItem::Text("🔐 ".to_string()));

    for (i, account) in accounts.iter().enumerate() {
        if i > 0 {
            items.push(RenderItem::Separator(" | ".to_string()));
        }

        // Service/account name
        items.push(RenderItem::Text(format!(
            "{}/{}",
            account.service, account.account
        )));

        // Status indicator
        let (status_text, color) = if !account.token_valid {
            (" ✗", colors::INVALID)
        } else if account.expires_soon {
            (" ⚠️", colors::EXPIRING)
        } else {
            (" ✓", colors::VALID)
        };

        items.push(RenderItem::Foreground(Color::Hex(color.to_string())));
        items.push(RenderItem::Text(status_text.to_string()));
        items.push(RenderItem::ResetForeground);
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(service: &str, account: &str, valid: bool, expiring: bool) -> AccountStatus {
        AccountStatus {
            service: service.to_string(),
            account: account.to_string(),
            token_valid: valid,
            expires_soon: expiring,
        }
    }

    #[test]
    fn test_render_empty_status() {
        let items = render_status_bar(&[]);

        // Should contain lock emoji, spacing, and "no accounts" text
        assert!(!items.is_empty());

        // Check that we have the expected text
        let has_no_accounts = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("no accounts")
            } else {
                false
            }
        });
        assert!(has_no_accounts);
    }

    #[test]
    fn test_render_single_valid_account() {
        let accounts = vec![make_account("google", "personal", true, false)];
        let items = render_status_bar(&accounts);

        // Should contain "1 account"
        let has_single = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("1 account")
            } else {
                false
            }
        });
        assert!(has_single);

        // Should use valid (green) color
        let has_valid_color = items.iter().any(|item| {
            if let RenderItem::Foreground(Color::Hex(hex)) = item {
                hex == colors::VALID
            } else {
                false
            }
        });
        assert!(has_valid_color);
    }

    #[test]
    fn test_render_multiple_accounts() {
        let accounts = vec![
            make_account("google", "personal", true, false),
            make_account("github", "work", true, false),
        ];
        let items = render_status_bar(&accounts);

        // Should contain "2 accounts"
        let has_multiple = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("2 accounts")
            } else {
                false
            }
        });
        assert!(has_multiple);
    }

    #[test]
    fn test_render_expiring_account() {
        let accounts = vec![make_account("spotify", "personal", true, true)];
        let items = render_status_bar(&accounts);

        // Should use expiring (yellow) color
        let has_expiring_color = items.iter().any(|item| {
            if let RenderItem::Foreground(Color::Hex(hex)) = item {
                hex == colors::EXPIRING
            } else {
                false
            }
        });
        assert!(has_expiring_color);

        // Should have warning icon
        let has_warning = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("⚠️")
            } else {
                false
            }
        });
        assert!(has_warning);
    }

    #[test]
    fn test_render_invalid_account() {
        let accounts = vec![make_account("github", "work", false, false)];
        let items = render_status_bar(&accounts);

        // Should use invalid (red) color
        let has_invalid_color = items.iter().any(|item| {
            if let RenderItem::Foreground(Color::Hex(hex)) = item {
                hex == colors::INVALID
            } else {
                false
            }
        });
        assert!(has_invalid_color);
    }

    #[test]
    fn test_render_detailed_status() {
        let accounts = vec![
            make_account("google", "personal", true, false),
            make_account("github", "work", false, false),
        ];
        let items = render_detailed_status(&accounts);

        // Should contain service/account names
        let has_google = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("google/personal")
            } else {
                false
            }
        });
        assert!(has_google);

        let has_github = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("github/work")
            } else {
                false
            }
        });
        assert!(has_github);
    }

    #[test]
    fn test_render_detailed_empty() {
        let items = render_detailed_status(&[]);

        // Should show "No accounts configured"
        let has_no_accounts = items.iter().any(|item| {
            if let RenderItem::Text(text) = item {
                text.contains("No accounts configured")
            } else {
                false
            }
        });
        assert!(has_no_accounts);
    }
}
