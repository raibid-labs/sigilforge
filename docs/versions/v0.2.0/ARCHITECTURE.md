# Sigilforge Architecture

This document describes the internal architecture of Sigilforge, including workspace layout, component responsibilities, and key design decisions.

## Workspace Layout

```
sigilforge/
├── Cargo.toml                  # Workspace manifest
├── sigilforge-core/            # Core library crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Public API surface
│       ├── model/              # Domain types
│       │   ├── mod.rs
│       │   ├── service.rs      # ServiceId, ServiceConfig
│       │   ├── account.rs      # AccountId, Account
│       │   └── credential.rs   # CredentialRef, Token, etc.
│       ├── store/              # Secret storage abstraction
│       │   ├── mod.rs
│       │   ├── traits.rs       # SecretStore trait
│       │   ├── memory.rs       # In-memory implementation
│       │   ├── keyring.rs      # OS keyring implementation
│       │   └── encrypted.rs    # ROPS/SOPS file backend
│       ├── auth/               # OAuth and token management
│       │   ├── mod.rs
│       │   ├── traits.rs       # TokenManager trait
│       │   ├── oauth.rs        # OAuth2 flow implementations
│       │   └── providers/      # Provider-specific configs
│       └── resolve/            # Reference resolution
│           ├── mod.rs
│           ├── traits.rs       # ReferenceResolver trait
│           └── auth_uri.rs     # auth:// URI parser
│
├── sigilforge-daemon/          # Background service
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Entry point
│       ├── config.rs           # Daemon configuration
│       ├── server.rs           # Socket server
│       ├── api/                # API handlers
│       │   ├── mod.rs
│       │   └── handlers.rs     # Request handlers
│       └── state.rs            # Runtime state management
│
├── sigilforge-cli/             # Command-line interface
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Entry point + CLI definition
│       └── commands/           # Subcommand implementations
│           ├── mod.rs
│           ├── add_account.rs
│           ├── list_accounts.rs
│           └── get_token.rs
│
└── docs/
    ├── ARCHITECTURE.md         # This file
    ├── ROADMAP.md              # Development phases
    └── INTERFACES.md           # Trait and API definitions
```

## Crate Responsibilities

### sigilforge-core

The core library contains all domain logic and is designed to be embeddable in other applications:

| Module | Responsibility |
|--------|----------------|
| `model` | Domain types: ServiceId, AccountId, Account, CredentialRef, Token |
| `store` | Secret storage abstraction with multiple backends |
| `auth` | OAuth2 flow execution and token lifecycle management |
| `resolve` | Reference resolution for `auth://` URIs and vals-style refs |

**Key design principle**: Core is async-runtime-agnostic where possible. Traits use `async_trait` for async methods but don't mandate a specific executor.

### sigilforge-daemon

The daemon provides a long-running service that:

1. Holds runtime state (loaded accounts, cached tokens)
2. Exposes a local API over Unix socket or named pipe
3. Runs OAuth flows requiring user interaction
4. Persists state changes to storage backends

**Communication**: JSON-RPC 2.0 over Unix socket (Linux/macOS) or named pipe (Windows).

### sigilforge-cli

The CLI provides human-friendly commands:

- `add-account <service> <account>` - Start OAuth flow or prompt for API key
- `list-accounts` - Show configured accounts
- `get-token <service> <account>` - Print a fresh access token
- `remove-account <service> <account>` - Remove stored credentials
- `status` - Show daemon status and account health

**Modes**: CLI can operate in two modes:
1. **Daemon mode** (default): Connects to running daemon via socket
2. **Direct mode** (`--direct`): Uses sigilforge-core directly (useful when daemon isn't running)

## Secret Storage Abstraction

Sigilforge supports multiple storage backends through the `SecretStore` trait:

```
┌──────────────────────────────────────────────────────────┐
│                     SecretStore Trait                     │
├──────────────────────────────────────────────────────────┤
│  get(key) -> Option<Secret>                              │
│  set(key, secret)                                        │
│  delete(key)                                             │
│  list_keys(prefix) -> Vec<String>                        │
└──────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          │                   │                   │
          ▼                   ▼                   ▼
    ┌───────────┐      ┌───────────┐      ┌───────────┐
    │  Memory   │      │  Keyring  │      │ Encrypted │
    │  Store    │      │   Store   │      │   File    │
    │           │      │           │      │  Store    │
    │ (testing) │      │ (runtime) │      │ (config)  │
    └───────────┘      └───────────┘      └───────────┘
```

### Backend Details

#### MemoryStore
- In-memory HashMap for testing and development
- Not persistent; data lost on restart

#### KeyringStore
- Uses OS keyring via the `keyring` crate
- Linux: libsecret/Secret Service
- macOS: Keychain
- Windows: Credential Manager
- Best for runtime secrets (refresh tokens, API keys)

#### EncryptedFileStore
- ROPS (Rust-native) or SOPS (via CLI) encrypted YAML/JSON files
- Git-friendly: encrypted files can be committed
- Useful for service configurations and non-secret metadata
- Encryption keys from age, GPG, or cloud KMS

### Storage Key Convention

Keys follow a hierarchical pattern:
```
sigilforge/{service}/{account}/{credential_type}
```

Examples:
```
sigilforge/spotify/personal/refresh_token
sigilforge/spotify/personal/access_token
sigilforge/github/work/api_key
sigilforge/gmail/lab/client_secret
```

## OAuth Flows

Sigilforge implements two OAuth2 flows:

### Authorization Code + PKCE

Used for services with browser-based auth (most web services):

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│   User   │     │ Sigil-   │     │ Local    │     │ Provider │
│          │     │ forge    │     │ Browser  │     │ (Google) │
└────┬─────┘     └────┬─────┘     └────┬─────┘     └────┬─────┘
     │                │                │                │
     │ add-account    │                │                │
     │───────────────>│                │                │
     │                │                │                │
     │                │ Open auth URL  │                │
     │                │───────────────>│                │
     │                │                │                │
     │                │                │ Auth request   │
     │                │                │───────────────>│
     │                │                │                │
     │                │                │<───────────────│
     │                │                │ Redirect +code │
     │                │                │                │
     │                │<───────────────│                │
     │                │ Callback       │                │
     │                │                │                │
     │                │ Exchange code  │                │
     │                │───────────────────────────────>│
     │                │                │                │
     │                │<──────────────────────────────│
     │                │ Access + Refresh tokens        │
     │                │                │                │
     │<───────────────│                │                │
     │ Success        │                │                │
```

The daemon starts a temporary local HTTP server to receive the OAuth callback.

### Device Code Flow

Used for CLI-only environments or services that support it:

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│   User   │     │ Sigil-   │     │ External │     │ Provider │
│          │     │ forge    │     │ Browser  │     │ (GitHub) │
└────┬─────┘     └────┬─────┘     └────┬─────┘     └────┬─────┘
     │                │                │                │
     │ add-account    │                │                │
     │───────────────>│                │                │
     │                │                │                │
     │                │ Request device code             │
     │                │───────────────────────────────>│
     │                │                │                │
     │                │<──────────────────────────────│
     │                │ device_code + user_code        │
     │                │                │                │
     │<───────────────│                │                │
     │ "Visit URL,    │                │                │
     │  enter code"   │                │                │
     │                │                │                │
     │                │                │ User visits    │
     │───────────────────────────────>│───────────────>│
     │                │                │ Enters code    │
     │                │                │                │
     │                │ Poll for token │                │
     │                │───────────────────────────────>│
     │                │                │                │
     │                │<──────────────────────────────│
     │                │ Access + Refresh tokens        │
     │                │                │                │
     │<───────────────│                │                │
     │ Success        │                │                │
```

## Token Lifecycle

The `TokenManager` handles token refresh and caching:

```
┌─────────────────────────────────────────────────────────────────┐
│                     ensure_access_token()                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ Check memory    │
                    │ cache           │
                    └────────┬────────┘
                              │
              ┌───────────────┴───────────────┐
              │                               │
              ▼                               ▼
        [Cache hit]                     [Cache miss]
              │                               │
              ▼                               ▼
    ┌─────────────────┐             ┌─────────────────┐
    │ Token expired?  │             │ Load from       │
    │                 │             │ SecretStore     │
    └────────┬────────┘             └────────┬────────┘
              │                               │
      ┌───────┴───────┐                       │
      │               │                       │
      ▼               ▼                       ▼
  [Valid]        [Expired]           ┌─────────────────┐
      │               │              │ Has refresh     │
      │               │              │ token?          │
      │               ▼              └────────┬────────┘
      │     ┌─────────────────┐               │
      │     │ Refresh using   │       ┌───────┴───────┐
      │     │ refresh_token   │       │               │
      │     └────────┬────────┘       ▼               ▼
      │               │           [Yes]           [No]
      │               │              │               │
      │               │              │               ▼
      │               │              │      ┌─────────────────┐
      │               │              │      │ Error: Re-auth  │
      │               │              │      │ required        │
      │               │              │      └─────────────────┘
      │               │              │
      │               ▼              ▼
      │     ┌─────────────────┐     ┌─────────────────┐
      │     │ Store new       │     │ Refresh using   │
      │     │ access_token    │     │ refresh_token   │
      │     └────────┬────────┘     └────────┬────────┘
      │               │                       │
      │               │                       ▼
      │               │              ┌─────────────────┐
      │               │              │ Store tokens,   │
      │               │              │ update cache    │
      │               │              └────────┬────────┘
      │               │                       │
      └───────────────┴───────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ Return valid    │
                    │ access_token    │
                    └─────────────────┘
```

### Token Expiry Buffer

Access tokens are considered expired 5 minutes before their actual expiry to prevent edge cases where a token expires mid-request.

## Local API Surface

The daemon exposes a JSON-RPC 2.0 API over Unix socket:

### Socket Location

- Linux: `$XDG_RUNTIME_DIR/sigilforge.sock` or `/tmp/sigilforge-$UID.sock`
- macOS: `~/Library/Application Support/sigilforge/daemon.sock`
- Windows: Named pipe `\\.\pipe\sigilforge`

### Methods

| Method | Description |
|--------|-------------|
| `get_token` | Get a valid access token for service/account |
| `list_accounts` | List all configured accounts |
| `get_account` | Get details for a specific account |
| `add_account` | Start account setup (OAuth or manual) |
| `remove_account` | Remove an account and its credentials |
| `refresh_token` | Force token refresh |
| `resolve` | Resolve an `auth://` reference |
| `status` | Get daemon status |

### Example Request/Response

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "get_token",
  "params": {
    "service": "spotify",
    "account": "personal"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "access_token": "BQC...",
    "token_type": "Bearer",
    "expires_at": "2024-01-15T10:30:00Z"
  }
}
```

## Configuration

### Daemon Config

Location: `~/.config/sigilforge/config.toml` (Linux)

```toml
[daemon]
socket_path = "/run/user/1000/sigilforge.sock"  # Optional override

[storage]
# Primary backend for runtime secrets
primary = "keyring"

# Optional secondary backend for encrypted configs
secondary = { type = "rops", path = "~/.config/sigilforge/secrets.yaml" }

[oauth]
# Local callback server for auth code flow
callback_port = 8484
callback_host = "127.0.0.1"

[providers.spotify]
client_id = "your-client-id"
# client_secret stored in keyring: sigilforge/spotify/_provider/client_secret
scopes = ["user-read-private", "playlist-read-private"]

[providers.gmail]
client_id = "your-client-id"
scopes = ["https://www.googleapis.com/auth/gmail.readonly"]
```

### Account Storage

Account metadata is stored in `~/.config/sigilforge/accounts.toml`:

```toml
[[accounts]]
service = "spotify"
id = "personal"
scopes = ["user-read-private", "playlist-read-private"]
created_at = "2024-01-10T15:30:00Z"

[[accounts]]
service = "gmail"
id = "work"
scopes = ["https://www.googleapis.com/auth/gmail.readonly"]
created_at = "2024-01-12T09:00:00Z"
```

Actual credentials (tokens, API keys) are stored separately in the configured `SecretStore`.

## Security Considerations

1. **No plaintext secrets in config files**: Client secrets and tokens go in keyring or encrypted files only.

2. **Minimal daemon permissions**: Daemon runs as user, not root. Uses user's keyring.

3. **Socket permissions**: Unix socket is created with user-only permissions (0600).

4. **Token exposure**: Access tokens are short-lived and never logged. Debug logging redacts sensitive values.

5. **PKCE for OAuth**: All auth code flows use PKCE to prevent authorization code interception.

6. **Localhost only**: OAuth callback server binds to localhost only; daemon socket is local.
