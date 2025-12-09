//! JSON-RPC API handlers for the daemon.

use sigilforge_core::{
    account_store::AccountStore,
    model::{Account, AccountId, ServiceId},
    store::{create_store, SecretStore},
    token_manager::DefaultTokenManager,
    provider::ProviderRegistry,
    TokenManager,
    DefaultReferenceResolver,
    ReferenceResolver,
};
use std::sync::Arc;

/// Information about a configured account (RPC response)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountInfo {
    pub service: String,
    pub account: String,
    pub scopes: Vec<String>,
    pub created_at: String,
    pub last_used: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddAccountResponse {
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GetTokenResponse {
    pub token: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListAccountsResponse {
    pub accounts: Vec<AccountInfo>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolveResponse {
    pub value: String,
}
use anyhow::Result;
use jsonrpsee::core::RpcResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::{ErrorCode, ErrorObject};
use tracing::{debug, info};

/// Type alias for the token manager used by the daemon.
pub type DaemonTokenManager = DefaultTokenManager<Box<dyn SecretStore>>;

/// Type alias for the reference resolver used by the daemon.
pub type DaemonResolver = DefaultReferenceResolver<Box<dyn SecretStore>, DaemonTokenManager>;

/// State shared across RPC handlers.
pub struct ApiState {
    /// Persistent account store
    pub accounts: Arc<AccountStore>,
    /// Token manager for token operations
    pub token_manager: Arc<DaemonTokenManager>,
    /// Reference resolver for auth:// URIs
    pub resolver: Arc<DaemonResolver>,
}

impl ApiState {
    /// Create a new API state.
    pub fn new() -> Result<Self> {
        let accounts = AccountStore::load()?;

        // Create secret store (prefer keyring)
        let store = create_store(true);

        // Create provider registry with defaults
        let providers = ProviderRegistry::with_defaults();

        // Create token manager
        let token_manager = DefaultTokenManager::new(store, providers);

        // Clone references for resolver (store is moved, so we need to create another)
        let resolver_store = create_store(true);
        let resolver_token_manager = DefaultTokenManager::new(
            create_store(true),
            ProviderRegistry::with_defaults(),
        );
        let resolver = DefaultReferenceResolver::new(resolver_store, resolver_token_manager);

        Ok(Self {
            accounts: Arc::new(accounts),
            token_manager: Arc::new(token_manager),
            resolver: Arc::new(resolver),
        })
    }

    /// Create API state with a provided account store (useful for tests).
    #[allow(dead_code)]
    pub fn with_store(accounts: AccountStore) -> Self {
        let store = create_store(false); // Use memory store for tests
        let providers = ProviderRegistry::new();
        let token_manager = DefaultTokenManager::new(store, providers);

        let resolver_store = create_store(false);
        let resolver_token_manager = DefaultTokenManager::new(
            create_store(false),
            ProviderRegistry::new(),
        );
        let resolver = DefaultReferenceResolver::new(resolver_store, resolver_token_manager);

        Self {
            accounts: Arc::new(accounts),
            token_manager: Arc::new(token_manager),
            resolver: Arc::new(resolver),
        }
    }
}

impl Default for ApiState {
    fn default() -> Self {
        Self::new().expect("failed to load AccountStore")
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
        let service_id = ServiceId::new(&service);
        let account_id = AccountId::new(&account);
        if self
            .state
            .accounts
            .get_account(&service_id, &account_id)
            .map_err(internal_error)?
            .is_none()
        {
            return Err(ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Account {}/{} not found", service, account),
                None::<()>,
            ));
        }

        // Update last_used timestamp
        let _ = self
            .state
            .accounts
            .update_last_used(&service_id, &account_id)
            .map_err(internal_error);

        // Use the token manager to get a valid token (handles refresh)
        match self
            .state
            .token_manager
            .ensure_access_token(&service_id, &account_id)
            .await
        {
            Ok(token) => {
                let expires_at = token.expires_at.map(|dt| dt.to_rfc3339());
                Ok(GetTokenResponse {
                    token: token.access_token.expose().to_string(),
                    expires_at,
                })
            }
            Err(e) => {
                // If no token found, return a more helpful error
                Err(ErrorObject::owned(
                    ErrorCode::InternalError.code(),
                    format!("Failed to get token: {}. You may need to re-authenticate.", e),
                    None::<()>,
                ))
            }
        }
    }

    async fn list_accounts(&self, service: Option<String>) -> RpcResult<ListAccountsResponse> {
        debug!("RPC: list_accounts(service: {:?})", service);

        let service_filter = service.as_ref().map(ServiceId::new);
        let accounts = self
            .state
            .accounts
            .list_accounts(service_filter.as_ref())
            .map_err(internal_error)?;

        let filtered: Vec<AccountInfo> = accounts
            .into_iter()
            .map(|a| AccountInfo {
                service: a.service.to_string(),
                account: a.id.to_string(),
                scopes: a.scopes,
                created_at: a.created_at.to_rfc3339(),
                last_used: a.last_used.map(|dt| dt.to_rfc3339()),
            })
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

        let new_account = Account::new(
            ServiceId::new(&service),
            AccountId::new(&account),
            scopes,
        );

        if let Err(e) = self.state.accounts.add_account(new_account) {
            return Err(match e {
                sigilforge_core::account_store::AccountStoreError::AlreadyExists { .. } => {
                    ErrorObject::owned(
                        ErrorCode::InvalidParams.code(),
                        format!("Account {}/{} already exists", service, account),
                        None::<()>,
                    )
                }
                other => internal_error(other),
            });
        }

        Ok(AddAccountResponse {
            message: format!("Account {}/{} added successfully", service, account),
        })
    }

    async fn resolve(&self, reference: String) -> RpcResult<ResolveResponse> {
        info!("RPC: resolve({})", reference);

        // Parse and validate the reference first
        use sigilforge_core::CredentialRef;
        let cred_ref = CredentialRef::from_auth_uri(&reference).map_err(|e| {
            ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Invalid reference: {}", e),
                None::<()>,
            )
        })?;

        // Check if account exists
        let account_exists = self
            .state
            .accounts
            .get_account(&cred_ref.service, &cred_ref.account)
            .map_err(internal_error)?
            .is_some();

        if !account_exists {
            return Err(ErrorObject::owned(
                ErrorCode::InvalidParams.code(),
                format!("Account {}/{} not found", cred_ref.service, cred_ref.account),
                None::<()>,
            ));
        }

        // Use the resolver to get the actual value
        match self.state.resolver.resolve(&reference).await {
            Ok(resolved) => Ok(ResolveResponse {
                value: resolved.expose(),
            }),
            Err(e) => Err(ErrorObject::owned(
                ErrorCode::InternalError.code(),
                format!("Failed to resolve reference: {}", e),
                None::<()>,
            )),
        }
    }
}

fn internal_error<E: std::fmt::Display>(err: E) -> ErrorObject<'static> {
    ErrorObject::owned(
        ErrorCode::InternalError.code(),
        format!("{}", err),
        None::<()>,
    )
}
