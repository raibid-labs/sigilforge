//! # Sigilforge Core
//!
//! Core library for Sigilforge credential management.
//!
//! This crate provides:
//! - Domain types for services, accounts, and credentials
//! - Traits for secret storage, token management, and reference resolution
//! - In-memory and (optionally) keyring-based storage implementations
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use sigilforge_core::{ServiceId, AccountId, TokenManager};
//!
//! async fn get_token(manager: &impl TokenManager) -> Result<String, sigilforge_core::TokenError> {
//!     let service = ServiceId::new("spotify");
//!     let account = AccountId::new("personal");
//!     let token = manager.ensure_access_token(&service, &account).await?;
//!     Ok(token.access_token.expose().to_string())
//! }
//! ```

pub mod account_store;
pub mod error;
pub mod model;
pub mod resolve;
pub mod store;
pub mod token;

#[cfg(feature = "oauth")]
pub mod provider;

#[cfg(feature = "oauth")]
pub mod token_manager;

#[cfg(feature = "oauth")]
pub mod oauth;

#[cfg(feature = "github-app")]
pub mod github_app;

// Re-export commonly used types at crate root
pub use model::{Account, AccountId, CredentialRef, CredentialType, ServiceId};

pub use store::{MemoryStore, Secret, SecretStore, StoreError, create_store};

#[cfg(feature = "keyring-store")]
pub use store::KeyringStore;

pub use token::{Token, TokenError, TokenInfo, TokenManager, TokenSet};

pub use resolve::{ReferenceResolver, ResolveError, ResolvedValue, ResolverConfig};

#[cfg(feature = "oauth")]
pub use resolve::DefaultReferenceResolver;

pub use error::SigilforgeError;

pub use account_store::{AccountStore, AccountStoreError};

#[cfg(feature = "oauth")]
pub use provider::{ProviderConfig, ProviderRegistry};

#[cfg(feature = "oauth")]
pub use token_manager::DefaultTokenManager;

#[cfg(feature = "github-app")]
pub use github_app::{
    GITHUB_APP_SERVICE, GitHubAppCredential, GitHubAppError, GitHubAppTokenManager,
};
