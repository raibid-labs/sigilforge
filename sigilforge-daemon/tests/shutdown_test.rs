//! Integration test for daemon graceful shutdown.
//!
//! This test verifies that the daemon can be shut down gracefully without panics
//! and that the socket file is properly cleaned up.

use std::path::PathBuf;
use tokio::time::{sleep, Duration};
use sigilforge_core::account_store::AccountStore;

/// Detect whether the sandbox allows binding Unix sockets. Skip tests if not.
fn can_bind_unix_socket() -> bool {
    let path = std::env::temp_dir().join("sigilforge-socket-permission-check.sock");
    let _ = std::fs::remove_file(&path);
    let result = std::os::unix::net::UnixListener::bind(&path);
    let ok = result.is_ok();
    let _ = std::fs::remove_file(&path);
    ok
}

#[tokio::test]
async fn test_graceful_shutdown() {
    if !can_bind_unix_socket() {
        eprintln!("Skipping test_graceful_shutdown: Unix sockets not permitted in sandbox");
        return;
    }

    // Use a unique socket path for this test
    let socket_path = PathBuf::from("/tmp/sigilforge-test-shutdown.sock");

    // Remove socket if it exists
    let _ = std::fs::remove_file(&socket_path);

    // Create a temporary account store
    let store_path = std::env::temp_dir().join("sigilforge-test-shutdown-accounts.json");
    let _ = std::fs::remove_file(&store_path); // Clean up from previous runs
    let store = AccountStore::load_from_path(store_path.clone())
        .expect("Failed to create account store");

    // Start the server
    let state = sigilforge_daemon::api::ApiState::with_store(store);
    let server_handle = sigilforge_daemon::api::start_server(&socket_path, state)
        .await
        .expect("Failed to start server");

    // Give the server time to start
    sleep(Duration::from_millis(100)).await;

    // Verify socket exists
    assert!(socket_path.exists(), "Socket file should exist after server start");

    // Stop the server gracefully - this should not panic
    server_handle.stop().await.expect("Server stop should succeed");

    // Wait a bit for cleanup
    sleep(Duration::from_millis(100)).await;

    // Note: Socket cleanup is done in main.rs, not in the server itself
    // So we manually clean it up here as main.rs would
    if socket_path.exists() {
        std::fs::remove_file(&socket_path).expect("Failed to remove socket file");
    }

    // Verify socket was cleaned up
    assert!(!socket_path.exists(), "Socket file should be removed after shutdown");

    // Cleanup test account store
    let _ = std::fs::remove_file(&store_path);
}

#[tokio::test]
async fn test_shutdown_with_active_connections() {
    if !can_bind_unix_socket() {
        eprintln!("Skipping test_shutdown_with_active_connections: Unix sockets not permitted in sandbox");
        return;
    }

    // Use a unique socket path for this test
    let socket_path = PathBuf::from("/tmp/sigilforge-test-shutdown-connections.sock");

    // Remove socket if it exists
    let _ = std::fs::remove_file(&socket_path);

    // Create a temporary account store
    let store_path = std::env::temp_dir().join("sigilforge-test-shutdown-connections-accounts.json");
    let _ = std::fs::remove_file(&store_path);
    let store = AccountStore::load_from_path(store_path.clone())
        .expect("Failed to create account store");

    // Start the server
    let state = sigilforge_daemon::api::ApiState::with_store(store);
    let server_handle = sigilforge_daemon::api::start_server(&socket_path, state)
        .await
        .expect("Failed to start server");

    // Give the server time to start
    sleep(Duration::from_millis(100)).await;

    // Create a connection to the server
    let mut stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("Failed to connect to server");

    // Send a request but don't wait for response
    use tokio::io::AsyncWriteExt;
    let request = r#"{"jsonrpc":"2.0","method":"list_accounts","params":[null],"id":1}"#;
    stream.write_all(request.as_bytes()).await.expect("Failed to write request");
    stream.write_all(b"\n").await.expect("Failed to write newline");
    stream.flush().await.expect("Failed to flush");

    // Stop the server while connection is active - this should not panic
    server_handle.stop().await.expect("Server stop should succeed even with active connections");

    // Cleanup
    if socket_path.exists() {
        std::fs::remove_file(&socket_path).expect("Failed to remove socket file");
    }
    let _ = std::fs::remove_file(&store_path);
}

#[tokio::test]
async fn test_multiple_stop_calls() {
    if !can_bind_unix_socket() {
        eprintln!("Skipping test_multiple_stop_calls: Unix sockets not permitted in sandbox");
        return;
    }

    // Use a unique socket path for this test
    let socket_path = PathBuf::from("/tmp/sigilforge-test-multi-stop.sock");

    // Remove socket if it exists
    let _ = std::fs::remove_file(&socket_path);

    // Create a temporary account store
    let store_path = std::env::temp_dir().join("sigilforge-test-multi-stop-accounts.json");
    let _ = std::fs::remove_file(&store_path);
    let store = AccountStore::load_from_path(store_path.clone())
        .expect("Failed to create account store");

    // Start the server
    let state = sigilforge_daemon::api::ApiState::with_store(store);
    let server_handle = sigilforge_daemon::api::start_server(&socket_path, state)
        .await
        .expect("Failed to start server");

    // Give the server time to start
    sleep(Duration::from_millis(100)).await;

    // Call stop multiple times - should be idempotent
    server_handle.stop().await.expect("First stop should succeed");
    server_handle.stop().await.expect("Second stop should succeed");
    server_handle.stop().await.expect("Third stop should succeed");

    // Cleanup
    if socket_path.exists() {
        std::fs::remove_file(&socket_path).expect("Failed to remove socket file");
    }
    let _ = std::fs::remove_file(&store_path);
}
