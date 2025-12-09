//! JSON-RPC API for daemon IPC.
//!
//! This module provides a JSON-RPC interface for communication between
//! the sigilforge-cli client and the sigilforged daemon.

pub mod handlers;
pub mod server;

#[allow(unused_imports)]
pub use handlers::{ApiState, AccountInfo, AddAccountResponse, GetTokenResponse, ListAccountsResponse, ResolveResponse};
#[allow(unused_imports)]
pub use server::{start_server, ServerHandle};
