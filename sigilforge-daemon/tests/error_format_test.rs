//! Test to verify JSON-RPC 2.0 error response format compliance.
//!
//! This test checks that error responses follow the JSON-RPC 2.0 specification:
//! - Must have `"jsonrpc": "2.0"` (not "jsonrpsee")
//! - Must have `error` object with `code` (integer) and `message` (string)
//! - Must echo the `id` from the request
//! - Optionally may have `data` field

use serde_json::json;
use std::path::PathBuf;
use std::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::{sleep, Duration};

use sigilforge_core::account_store::AccountStore;
use sigilforge_daemon::api::{start_server, ApiState};

/// Helper to start a test server that stays alive for the duration of the test
async fn start_test_server(socket_path: &std::path::Path, store: AccountStore) {
    let socket_path = socket_path.to_path_buf();
    tokio::spawn(async move {
        let state = ApiState::with_store(store);
        match start_server(&socket_path, state).await {
            Ok(handle) => {
                // Keep the handle alive to prevent server shutdown
                std::mem::forget(handle);
            }
            Err(e) => {
                eprintln!("Server start failed: {}", e);
            }
        }
    });
}

/// Detect whether the sandbox allows binding Unix sockets. Skip tests if not.
fn can_bind_unix_socket() -> bool {
    let path = std::env::temp_dir().join("sigilforge-socket-permission-check.sock");
    let _ = fs::remove_file(&path);
    let result = std::os::unix::net::UnixListener::bind(&path);
    let ok = result.is_ok();
    let _ = fs::remove_file(&path);
    ok
}

/// Helper to send raw JSON-RPC request and get raw response
/// Creates a fresh connection for each request to avoid stream state issues.
async fn send_raw_request(
    socket_path: &std::path::Path,
    request: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let (reader, _writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut response_str = String::new();
    reader.read_line(&mut response_str).await?;

    Ok(serde_json::from_str(&response_str)?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_jsonrpc_error_format_compliance() {
    if !can_bind_unix_socket() {
        eprintln!("Skipping test_jsonrpc_error_format_compliance: Unix sockets not permitted");
        return;
    }

    let socket_path = PathBuf::from("/tmp/sigilforge-test-error-format.sock");
    let _ = std::fs::remove_file(&socket_path);

    let store_path = std::env::temp_dir().join("sigilforge-test-error-format.json");
    let _ = std::fs::remove_file(&store_path); // Clean up any existing store
    let store = AccountStore::load_from_path(store_path).unwrap();
    start_test_server(&socket_path, store).await;

    // Wait for socket file to appear (up to 2 seconds)
    let mut attempts = 0;
    while !socket_path.exists() && attempts < 20 {
        sleep(Duration::from_millis(100)).await;
        attempts += 1;
    }

    if !socket_path.exists() {
        panic!("Socket file was not created at {:?} after 2 seconds", socket_path);
    }

    // Test 1: Parse error (invalid JSON)
    println!("\n=== Test 1: Parse Error ===");
    let response = send_raw_request(&socket_path, "{invalid json}")
        .await
        .expect("Failed to get response");

    println!("Response: {}", serde_json::to_string_pretty(&response).unwrap());

    // Check JSON-RPC 2.0 compliance
    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")),
        "Must have 'jsonrpc': '2.0' field");
    assert!(response.get("error").is_some(), "Must have 'error' field");
    assert_eq!(response.get("id"), Some(&json!(null)),
        "Must have 'id' field (null for parse errors)");

    let error = response.get("error").unwrap();
    assert!(error.get("code").is_some(), "Error must have 'code' field");
    assert!(error.get("message").is_some(), "Error must have 'message' field");
    assert_eq!(error.get("code").unwrap().as_i64(), Some(-32700),
        "Parse error should have code -32700");

    // Test 2: Invalid request (missing method)
    println!("\n=== Test 2: Invalid Request (Missing Method) ===");
    let request = json!({
        "jsonrpc": "2.0",
        "params": [],
        "id": 42
    });

    let response = send_raw_request(&socket_path, &request.to_string())
        .await
        .expect("Failed to get response");

    println!("Response: {}", serde_json::to_string_pretty(&response).unwrap());

    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")),
        "Must have 'jsonrpc': '2.0' field");
    assert!(response.get("error").is_some(), "Must have 'error' field");
    assert_eq!(response.get("id"), Some(&json!(42)),
        "Must echo the request id");

    let error = response.get("error").unwrap();
    assert!(error.get("code").is_some(), "Error must have 'code' field");
    assert!(error.get("message").is_some(), "Error must have 'message' field");
    assert_eq!(error.get("code").unwrap().as_i64(), Some(-32600),
        "Invalid request should have code -32600");

    // Test 3: Method not found
    println!("\n=== Test 3: Method Not Found ===");
    let request = json!({
        "jsonrpc": "2.0",
        "method": "nonexistent_method",
        "params": [],
        "id": 100
    });

    let response = send_raw_request(&socket_path, &request.to_string())
        .await
        .expect("Failed to get response");

    println!("Response: {}", serde_json::to_string_pretty(&response).unwrap());

    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")),
        "Must have 'jsonrpc': '2.0' field");
    assert!(response.get("error").is_some(), "Must have 'error' field");
    assert_eq!(response.get("id"), Some(&json!(100)),
        "Must echo the request id");

    let error = response.get("error").unwrap();
    assert_eq!(error.get("code").unwrap().as_i64(), Some(-32601),
        "Method not found should have code -32601");

    // Test 4: Invalid params
    println!("\n=== Test 4: Invalid Params ===");
    let request = json!({
        "jsonrpc": "2.0",
        "method": "get_token",
        "params": ["only_one_param"],
        "id": 200
    });

    let response = send_raw_request(&socket_path, &request.to_string())
        .await
        .expect("Failed to get response");

    println!("Response: {}", serde_json::to_string_pretty(&response).unwrap());

    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")),
        "Must have 'jsonrpc': '2.0' field");
    assert!(response.get("error").is_some(), "Must have 'error' field");
    assert_eq!(response.get("id"), Some(&json!(200)),
        "Must echo the request id");

    let error = response.get("error").unwrap();
    assert_eq!(error.get("code").unwrap().as_i64(), Some(-32602),
        "Invalid params should have code -32602");

    // Test 5: Application error (account not found)
    println!("\n=== Test 5: Application Error (Account Not Found) ===");
    let request = json!({
        "jsonrpc": "2.0",
        "method": "get_token",
        "params": ["nonexistent_service", "nonexistent_account"],
        "id": 300
    });

    let response = send_raw_request(&socket_path, &request.to_string())
        .await
        .expect("Failed to get response");

    println!("Response: {}", serde_json::to_string_pretty(&response).unwrap());

    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")),
        "Must have 'jsonrpc': '2.0' field");
    assert!(response.get("error").is_some(), "Must have 'error' field");
    assert_eq!(response.get("id"), Some(&json!(300)),
        "Must echo the request id");

    let error = response.get("error").unwrap();
    assert!(error.get("code").is_some(), "Error must have 'code' field");
    assert!(error.get("message").is_some(), "Error must have 'message' field");

    // Cleanup
    let _ = std::fs::remove_file(&socket_path);
    let store_path = std::env::temp_dir().join("sigilforge-test-error-format.json");
    let _ = std::fs::remove_file(&store_path);
}
