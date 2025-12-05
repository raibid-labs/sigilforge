# Next Steps for Sigilforge Development

This document outlines concrete next tasks for continuing Sigilforge development. It is intended to be self-contained so future Claude sessions can pick up work without seeing the original initialization prompt.

## Current State

The scaffolding is complete:

- **sigilforge-core**: Core types (`ServiceId`, `AccountId`, `Account`, `CredentialRef`), traits (`SecretStore`, `TokenManager`, `ReferenceResolver`), and an in-memory `MemoryStore` implementation.
- **sigilforge-daemon**: Placeholder daemon with configuration loading.
- **sigilforge-cli**: CLI with subcommands defined (`add-account`, `list-accounts`, `get-token`, `remove-account`, `resolve`, `daemon`), but all handlers are stubs.
- **docs/**: Architecture, roadmap, and interface documentation.

## Phase 1: Basic Storage and Account Model

### 1.1 Implement Account Persistence

Create an `AccountStore` that persists account metadata to disk:

```
sigilforge-core/src/account_store.rs
```

Requirements:
- Load/save accounts from `~/.config/sigilforge/accounts.json` (or platform equivalent)
- CRUD operations: `add_account`, `get_account`, `list_accounts`, `remove_account`
- Use `directories` crate for platform paths

### 1.2 Wire CLI to Account Store

Update CLI handlers in `sigilforge-cli/src/main.rs`:
- `add-account`: Create account metadata (without OAuth yet)
- `list-accounts`: Read from account store
- `remove-account`: Delete account and associated secrets

### 1.3 OS Keyring Integration

Implement `KeyringStore` in sigilforge-core:

```rust
// sigilforge-core/src/store/keyring.rs
pub struct KeyringStore { ... }

impl SecretStore for KeyringStore { ... }
```

Use the `keyring` crate. Handle platform differences gracefully.

## Phase 2: OAuth Flows

### 2.1 Provider Configuration

Create a provider registry:

```
sigilforge-core/src/provider.rs
```

Define `ProviderConfig`:
- OAuth endpoints (authorize, token, revoke)
- Default scopes
- Client ID/secret storage strategy

Start with 2-3 providers:
- **GitHub**: Simple OAuth2 with PKCE
- **Spotify**: Standard OAuth2
- **Google**: OAuth2 + refresh tokens

### 2.2 OAuth Flow Implementation

Use `oauth2` crate to implement:

1. **Authorization Code + PKCE** (primary):
   - Generate PKCE verifier
   - Open browser for authorization
   - Listen on localhost for callback
   - Exchange code for tokens

2. **Device Code** (for headless):
   - Request device code
   - Display user code
   - Poll for completion

```
sigilforge-core/src/oauth/
├── mod.rs
├── pkce.rs
└── device_code.rs
```

### 2.3 Token Manager Implementation

Implement `TokenManager` trait:

```rust
pub struct DefaultTokenManager<S: SecretStore> {
    store: S,
    providers: ProviderRegistry,
}
```

Logic for `ensure_access_token`:
1. Check stored token expiry
2. If expired, use refresh token
3. If no refresh token or refresh fails, return error indicating re-auth needed

## Phase 3: Daemon and IPC

### 3.1 Socket API

Implement JSON-RPC over Unix socket:

```
sigilforge-daemon/src/api/
├── mod.rs
├── server.rs
└── handlers.rs
```

Methods:
- `get_token(service, account)` → token string
- `list_accounts()` → account list
- `add_account(service, account, scopes)` → starts OAuth flow
- `resolve(reference)` → resolved value

### 3.2 CLI as Daemon Client

Update CLI to communicate with daemon:
- Check if daemon is running
- Send requests over socket
- Fall back to direct library calls if daemon unavailable

## Phase 4: Reference Resolution

### 4.1 Implement `ReferenceResolver`

```rust
pub struct DefaultResolver<T: TokenManager> {
    token_manager: T,
    config: ResolverConfig,
}
```

Handle `auth://` URIs:
- Parse with `CredentialRef::from_auth_uri`
- Route to `TokenManager` for tokens
- Route to `SecretStore` for API keys

### 4.2 Optional vals Integration

If `enable_vals` is true:
- Shell out to `vals` binary for `vals:ref+...` references
- Cache results based on `cache_ttl_secs`

## Integration with Scryforge

Scryforge will integrate with Sigilforge in two modes:

### Library Mode (Embedded)

```rust
use sigilforge_core::{TokenManager, DefaultTokenManager, KeyringStore};

let store = KeyringStore::new();
let manager = DefaultTokenManager::new(store, providers);
let token = manager.ensure_access_token(&service, &account).await?;
```

### Daemon Mode (IPC)

```rust
use sigilforge_client::DaemonClient;

let client = DaemonClient::connect().await?;
let token = client.get_token("spotify", "personal").await?;
```

The daemon mode is preferred for:
- Sharing tokens across multiple applications
- Centralized token refresh
- Avoiding keyring access from multiple processes

## Testing Strategy

### Unit Tests

Each module should have tests:
- `model.rs`: Parsing, serialization
- `store.rs`: MemoryStore operations
- `token.rs`: Expiry logic
- `resolve.rs`: URI parsing

### Integration Tests

```
sigilforge-core/tests/
├── account_lifecycle.rs
├── token_refresh.rs
└── reference_resolution.rs
```

Use mock OAuth servers for OAuth flow testing.

## Build and Verification

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run clippy
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --check
```

## File Structure (Target State)

```
sigilforge/
├── Cargo.toml
├── sigilforge-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── model.rs
│       ├── error.rs
│       ├── store/
│       │   ├── mod.rs
│       │   ├── memory.rs
│       │   └── keyring.rs
│       ├── token.rs
│       ├── resolve.rs
│       ├── account_store.rs
│       ├── provider.rs
│       └── oauth/
│           ├── mod.rs
│           ├── pkce.rs
│           └── device_code.rs
├── sigilforge-daemon/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       └── api/
│           ├── mod.rs
│           ├── server.rs
│           └── handlers.rs
├── sigilforge-cli/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       └── client.rs
└── docs/
    ├── ARCHITECTURE.md
    ├── ROADMAP.md
    ├── INTERFACES.md
    └── NEXT_STEPS.md
```
