# sigilforge-client

A lightweight Rust client library for the [Sigilforge](https://github.com/raibid-labs/sigilforge) authentication daemon.

Sigilforge provides centralized credential management for OAuth tokens, API keys, and secrets. This client provides a simple async interface for applications to obtain credentials without managing token lifecycle complexity.

## Features

- **Daemon Connection**: Connect to the Sigilforge daemon via Unix socket (Linux/macOS) or named pipe (Windows)
- **Fallback Support**: Automatically fall back to environment variables or config files when daemon unavailable
- **auth:// URI Resolution**: Parse and resolve `auth://service/account/type` credential references
- **Token Lifecycle**: Automatic token refresh through the daemon
- **Fusabi Integration**: Optional host function bindings for Fusabi scripts

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
sigilforge-client = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Quick Start

```rust
use sigilforge_client::{SigilforgeClient, TokenProvider};

#[tokio::main]
async fn main() -> sigilforge_client::Result<()> {
    // Create client with default configuration
    let client = SigilforgeClient::new();

    // Get a token (tries daemon first, then fallbacks)
    let token = client.get_token("spotify", "personal").await?;
    println!("Authorization: {}", token.authorization_header());

    // Resolve an auth:// URI
    let api_key = client.resolve("auth://openai/default/api_key").await?;
    println!("API Key: {}", api_key.value);

    Ok(())
}
```

## Fallback Configuration

When the Sigilforge daemon isn't running (development, CI, etc.), the client falls back to other credential sources:

### Environment Variables

```rust
use sigilforge_client::{SigilforgeClient, FallbackConfig};

// Use only environment variables (default prefix: SIGILFORGE)
let client = SigilforgeClient::fallback_only(FallbackConfig::env_vars());

// Custom prefix
let client = SigilforgeClient::fallback_only(
    FallbackConfig::env_vars_with_prefix("MYAPP")
);
```

Environment variable format: `{PREFIX}_{SERVICE}_{ACCOUNT}_{TYPE}`

Examples:
- `SIGILFORGE_SPOTIFY_PERSONAL_TOKEN`
- `SIGILFORGE_GITHUB_OSS_API_KEY`
- `SIGILFORGE_OPENAI_DEFAULT_API_KEY`

### Config File

```rust
use sigilforge_client::{SigilforgeClient, FallbackConfig};

let client = SigilforgeClient::fallback_only(
    FallbackConfig::config_file("/path/to/credentials.toml")
);
```

Config file format:

```toml
[credentials.spotify.personal]
token = "your-spotify-token"

[credentials.github.oss]
api_key = "ghp_xxxxxxxxxxxx"

[credentials.openai.default]
api_key = "sk-xxxxxxxxxxxx"
```

### Chained Fallbacks

```rust
use sigilforge_client::{SigilforgeClient, FallbackConfig};

// Try env vars first, then config file
let client = SigilforgeClient::new()
    .with_fallback(FallbackConfig::chain(vec![
        FallbackConfig::env_vars(),
        FallbackConfig::config_file("/etc/myapp/credentials.toml"),
    ]));
```

## auth:// URI Format

The `auth://` URI scheme provides a standard way to reference credentials:

```
auth://{service}/{account}/{credential_type}
```

Supported credential types:
- `token` - OAuth access token
- `refresh_token` - OAuth refresh token
- `api_key` - Static API key
- `client_id` - OAuth client ID
- `client_secret` - OAuth client secret

Examples:
- `auth://spotify/personal/token`
- `auth://github/oss/api_key`
- `auth://gmail/work/refresh_token`

### Parsing URIs

```rust
use sigilforge_client::AuthRef;

let auth_ref = AuthRef::parse("auth://spotify/personal/token")?;
println!("Service: {}", auth_ref.service);      // "spotify"
println!("Account: {}", auth_ref.account);      // "personal"
println!("Type: {}", auth_ref.credential_type); // Token

// Convert to env var name
println!("Env var: {}", auth_ref.to_env_var()); // SIGILFORGE_SPOTIFY_PERSONAL_TOKEN
```

## Builder Pattern

For more control over client configuration:

```rust
use sigilforge_client::{SigilforgeClientBuilder, FallbackConfig};
use std::time::Duration;

let client = SigilforgeClientBuilder::new()
    .socket_path("/custom/path/sigilforge.sock")
    .fallback(FallbackConfig::env_vars())
    .timeout(Duration::from_secs(10))
    .build();
```

## Daemon Health Check

```rust
use sigilforge_client::SigilforgeClient;

let client = SigilforgeClient::new();

// Check if daemon is available
if client.is_daemon_available().await {
    let health = client.health_check().await?;
    println!("Daemon version: {:?}", health.version);
    println!("Configured accounts: {:?}", health.account_count);
}
```

## Fusabi Integration

Use Sigilforge from Fusabi scripts via `fusabi-stdlib-ext`:

```toml
[dependencies]
fusabi-stdlib-ext = { version = "0.1", features = ["sigilforge"] }
```

Register the sigilforge module:

```rust
use fusabi_stdlib_ext::StdlibRegistry;

let registry = StdlibRegistry::default_config()?;
registry.register_sigilforge(&mut host_registry)?;
```

Then in Fusabi scripts:

```fsharp
// Get a token
let! token = Sigilforge.getToken "spotify" "personal"

// Resolve an auth:// URI
let! apiKey = Sigilforge.resolve "auth://openai/default/api_key"

// Check daemon availability
let! available = Sigilforge.isAvailable ()
```

## Feature Flags

- `fallback-env` (default): Enable environment variable fallback
- `fallback-config` (default): Enable TOML config file fallback

## Socket Paths

Default daemon socket locations:

| Platform | Path |
|----------|------|
| Linux | `$XDG_RUNTIME_DIR/sigilforge.sock` or `/tmp/sigilforge-$UID.sock` |
| macOS | `~/Library/Application Support/sigilforge/daemon.sock` |
| Windows | `\\.\pipe\sigilforge` |

## Error Handling

```rust
use sigilforge_client::{SigilforgeClient, SigilforgeError, TokenProvider};

let client = SigilforgeClient::new();

match client.get_token("spotify", "personal").await {
    Ok(token) => println!("Got token: {}", token.token),
    Err(SigilforgeError::DaemonUnavailable(msg)) => {
        println!("Daemon not running: {}", msg);
    }
    Err(SigilforgeError::AccountNotFound { service, account }) => {
        println!("Account {}/{} not configured", service, account);
    }
    Err(SigilforgeError::NoFallback { service, account }) => {
        println!("No fallback for {}/{}", service, account);
    }
    Err(e) => println!("Error: {}", e),
}
```

## License

MIT OR Apache-2.0
