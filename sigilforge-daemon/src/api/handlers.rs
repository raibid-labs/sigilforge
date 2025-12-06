//! JSON-RPC API handlers for the daemon.

use super::types::{
    AccountInfo, AddAccountRequest, AddAccountResponse, GetTokenRequest, GetTokenResponse,
    ListAccountsRequest, ListAccountsResponse, ResolveRequest, ResolveResponse,
};
use anyhow::Result;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::{ErrorCode, ErrorObject};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// State shared across RPC handlers.
#[derive(Clone)]
pub struct ApiState {
    /// In-memory storage for accounts (temporary implementation)
    pub accounts: Arc<RwLock<Vec<AccountInfo>>>,
}

impl ApiState {
    /// Create a new API state.
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for ApiState {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON-RPC API trait definition.
#[rpc(server)]
pub trait SigilforgeApi {
    /// Get a fresh access token for the specified account.
    ///
    /// # Parameters
    ///
    /// - `service`: Service identifier (e.g., "spotify")
    /// - `account`: Account identifier (e.g., "personal")
    ///
    /// # Returns
    ///
    /// A fresh access token and optional expiration timestamp.
    #[method(name = "get_token")]
    async fn get_token(&self, service: String, account: String) -> RpcResult<GetTokenResponse>;

    /// List all configured accounts, optionally filtered by service.
    ///
    /// # Parameters
    ///
    /// - `service`: Optional service filter
    ///
    /// # Returns
    ///
    /// List of configured accounts.
    #[method(name = "list_accounts")]
    async fn list_accounts(&self, service: Option<String>) -> RpcResult<ListAccountsResponse>;

    /// Add a new account with OAuth flow.
    ///
    /// # Parameters
    ///
    /// - `service`: Service identifier
    /// - `account`: Account identifier
    /// - `scopes`: OAuth scopes to request
    ///
    /// # Returns
    ///
    /// Confirmation message.
    #[method(name = "add_account")]
    async fn add_account(
        &self,
        service: String,
        account: String,
        scopes: Vec<String>,
    ) -> RpcResult<AddAccountResponse>;

    /// Resolve a credential reference to its actual value.
    ///
    /// # Parameters
    ///
    /// - `reference`: Credential reference (e.g., "auth://spotify/personal/token")
    ///
    /// # Returns
    ///
    /// The resolved credential value.
    #[method(name = "resolve")]
    async fn resolve(&self, reference: String) -> RpcResult<ResolveResponse>;
}

/// Implementation of the Sigilforge API.
pub struct SigilforgeApiImpl {
    state: ApiState,
}

impl SigilforgeApiImpl {
    /// Create a new API implementation with the given state.
    pub fn new(state: ApiState) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl SigilforgeApiServer for SigilforgeApiImpl {
    async fn get_token(&self, service: String, account: String) -> RpcResult<GetTokenResponse> {
        info!("RPC: get_token({}/{})", service, account);

        // Check if account exists
        let accounts = self.state.accounts.read().await;
        let account_exists = accounts
            .iter()
            .any(|a| a.service == service && a.account == account);

        if !account_exists {
            return Err(ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Account {}/{} not found", service, account),
                None::<()>,
            ));
        }

        // TODO: Integrate with actual token manager
        // For now, return a stub token
        Ok(GetTokenResponse {
            token: format!("stub_token_for_{}_{}", service, account),
            expires_at: Some("2025-12-07T00:00:00Z".to_string()),
        })
    }

    async fn list_accounts(&self, service: Option<String>) -> RpcResult<ListAccountsResponse> {
        debug!("RPC: list_accounts(service: {:?})", service);

        let accounts = self.state.accounts.read().await;
        let filtered: Vec<AccountInfo> = accounts
            .iter()
            .filter(|a| service.as_ref().map_or(true, |s| a.service == *s))
            .cloned()
            .collect();

        Ok(ListAccountsResponse {
            accounts: filtered,
        })
    }

    async fn add_account(
        &self,
        service: String,
        account: String,
        scopes: Vec<String>,
    ) -> RpcResult<AddAccountResponse> {
        info!("RPC: add_account({}/{}, scopes: {:?})", service, account, scopes);

        // Check for duplicates
        let mut accounts = self.state.accounts.write().await;
        if accounts
            .iter()
            .any(|a| a.service == service && a.account == account)
        {
            return Err(ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Account {}/{} already exists", service, account),
                None::<()>,
            ));
        }

        // Add the account
        let now = chrono::Utc::now();
        let new_account = AccountInfo {
            service: service.clone(),
            account: account.clone(),
            scopes,
            created_at: now.to_rfc3339(),
            last_used: None,
        };

        accounts.push(new_account);

        Ok(AddAccountResponse {
            message: format!("Account {}/{} added successfully", service, account),
        })
    }

    async fn resolve(&self, reference: String) -> RpcResult<ResolveResponse> {
        info!("RPC: resolve({})", reference);

        // Parse the reference
        use sigilforge_core::CredentialRef;
        let cred_ref = CredentialRef::from_auth_uri(&reference).map_err(|e| {
            ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Invalid reference: {}", e),
                None::<()>,
            )
        })?;

        // Check if account exists
        let accounts = self.state.accounts.read().await;
        let account_exists = accounts.iter().any(|a| {
            a.service == cred_ref.service.as_str() && a.account == cred_ref.account.as_str()
        });

        if !account_exists {
            return Err(ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Account {}/{} not found", cred_ref.service, cred_ref.account),
                None::<()>,
            ));
        }

        // TODO: Integrate with actual resolver
        // For now, return a stub value
        Ok(ResolveResponse {
            value: format!(
                "resolved_{}_{}_{}",
                cred_ref.service, cred_ref.account, cred_ref.credential_type
            ),
        })
    }
}
