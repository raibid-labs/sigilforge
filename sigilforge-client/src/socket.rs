use crate::types::{AccessToken, DaemonHealth, Result, SecretValue, SigilforgeError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, trace};

#[cfg(windows)]
use tracing::warn;

#[cfg(unix)]
use tokio::net::UnixStream;

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

/// Response for get_token method.
#[derive(Debug, Deserialize)]
struct GetTokenResponse {
    access_token: String,
    token_type: String,
    expires_at: Option<DateTime<Utc>>,
}

/// Response for resolve method.
#[derive(Debug, Deserialize)]
struct ResolveResponse {
    value: String,
    metadata: Option<serde_json::Value>,
}

/// Response for status method.
#[derive(Debug, Deserialize)]
struct StatusResponse {
    version: Option<String>,
    account_count: Option<u32>,
}

/// Counter for generating unique request IDs.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Get the default socket path for the current platform.
pub fn default_socket_path() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        // Try XDG_RUNTIME_DIR first
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            return Some(PathBuf::from(runtime_dir).join("sigilforge.sock"));
        }
        // Fall back to /tmp with UID
        let uid = unsafe { libc::getuid() };
        Some(PathBuf::from(format!("/tmp/sigilforge-{}.sock", uid)))
    }

    #[cfg(target_os = "macos")]
    {
        directories::ProjectDirs::from("", "", "sigilforge")
            .map(|dirs| dirs.data_dir().join("daemon.sock"))
    }

    #[cfg(target_os = "windows")]
    {
        // Windows uses named pipes, not file paths
        Some(PathBuf::from(r"\\.\pipe\sigilforge"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Client for communicating with the Sigilforge daemon over Unix socket.
pub struct DaemonConnection {
    socket_path: PathBuf,
    timeout: Duration,
}

impl DaemonConnection {
    /// Create a new daemon connection.
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            timeout: Duration::from_secs(5),
        }
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Check if the daemon is available.
    pub async fn is_available(&self) -> bool {
        self.health_check().await.is_ok()
    }

    /// Health check - verify daemon is running.
    pub async fn health_check(&self) -> Result<DaemonHealth> {
        let response = self.send_request("status", None).await?;

        let status: StatusResponse = serde_json::from_value(response)?;

        Ok(DaemonHealth {
            running: true,
            version: status.version,
            account_count: status.account_count,
        })
    }

    /// Get an access token from the daemon.
    pub async fn get_token(&self, service: &str, account: &str) -> Result<AccessToken> {
        let params = serde_json::json!({
            "service": service,
            "account": account
        });

        let response = self.send_request("get_token", Some(params)).await?;
        let token_resp: GetTokenResponse = serde_json::from_value(response)?;

        Ok(AccessToken {
            token: token_resp.access_token,
            token_type: token_resp.token_type,
            expires_at: token_resp.expires_at,
        })
    }

    /// Ensure a valid token (refresh if needed).
    pub async fn ensure_token(&self, service: &str, account: &str) -> Result<AccessToken> {
        let params = serde_json::json!({
            "service": service,
            "account": account,
            "refresh_if_needed": true
        });

        let response = self.send_request("get_token", Some(params)).await?;
        let token_resp: GetTokenResponse = serde_json::from_value(response)?;

        Ok(AccessToken {
            token: token_resp.access_token,
            token_type: token_resp.token_type,
            expires_at: token_resp.expires_at,
        })
    }

    /// Resolve an auth:// reference.
    pub async fn resolve(&self, reference: &str) -> Result<SecretValue> {
        let params = serde_json::json!({
            "reference": reference
        });

        let response = self.send_request("resolve", Some(params)).await?;
        let resolve_resp: ResolveResponse = serde_json::from_value(response)?;

        Ok(SecretValue {
            value: resolve_resp.value,
            metadata: resolve_resp.metadata,
        })
    }

    /// Send a JSON-RPC request to the daemon.
    #[cfg(unix)]
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        debug!("connecting to daemon at {:?}", self.socket_path);

        // Connect to socket
        let stream = tokio::time::timeout(
            self.timeout,
            UnixStream::connect(&self.socket_path),
        )
        .await
        .map_err(|_| SigilforgeError::Timeout)?
        .map_err(|e| {
            SigilforgeError::DaemonUnavailable(format!(
                "failed to connect to {}: {}",
                self.socket_path.display(),
                e
            ))
        })?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Build request
        let request_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: request_id,
            method,
            params,
        };

        let request_json = serde_json::to_string(&request)?;
        trace!("sending request: {}", request_json);

        // Send request (newline-delimited)
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut response_line = String::new();
        tokio::time::timeout(self.timeout, reader.read_line(&mut response_line))
            .await
            .map_err(|_| SigilforgeError::Timeout)?
            .map_err(|e| SigilforgeError::NetworkError(format!("failed to read response: {}", e)))?;

        trace!("received response: {}", response_line.trim());

        // Parse response
        let response: JsonRpcResponse = serde_json::from_str(&response_line)?;

        // Check for error
        if let Some(error) = response.error {
            return Err(SigilforgeError::DaemonError {
                code: error.code,
                message: error.message,
            });
        }

        response.result.ok_or_else(|| {
            SigilforgeError::DaemonError {
                code: -32600,
                message: "missing result in response".to_string(),
            }
        })
    }

    /// Send a JSON-RPC request (Windows named pipe).
    #[cfg(windows)]
    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        // Windows named pipe support would go here
        // For now, return an error indicating it's not yet implemented
        warn!("Windows named pipe support not yet implemented");
        Err(SigilforgeError::DaemonUnavailable(
            "Windows named pipe support not yet implemented".to_string(),
        ))
    }

    /// Stub for non-Unix/Windows platforms.
    #[cfg(not(any(unix, windows)))]
    async fn send_request(
        &self,
        _method: &str,
        _params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        Err(SigilforgeError::DaemonUnavailable(
            "daemon not supported on this platform".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_socket_path() {
        let path = default_socket_path();
        // Should return Some on supported platforms
        #[cfg(any(unix, windows))]
        assert!(path.is_some());
    }

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "get_token",
            params: Some(serde_json::json!({"service": "spotify", "account": "personal"})),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"get_token\""));
    }

    #[test]
    fn test_json_rpc_response_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"access_token":"test","token_type":"Bearer"}}"#;
        let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_error_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid request"}}"#;
        let response: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32600);
    }
}
