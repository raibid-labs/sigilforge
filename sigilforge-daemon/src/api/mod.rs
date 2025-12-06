//! JSON-RPC API for daemon IPC.
//!
//! This module provides a JSON-RPC interface for communication between
//! the sigilforge-cli client and the sigilforged daemon.

pub mod handlers;
pub mod server;
pub mod types;

pub use handlers::{ApiState, SigilforgeApiImpl, SigilforgeApiServer};
pub use server::{start_server, ServerConfig};
pub use types::*;
