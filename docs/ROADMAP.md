# Sigilforge Development Roadmap

This document outlines the development phases for Sigilforge, from initial scaffolding to full ecosystem integration.

## Current Status

**Version:** v0.2.0
**Overall Completion:** ~60%

| Phase | Status | Completion |
|-------|--------|------------|
| Phase 0: Scaffolding | Complete | 100% |
| Phase 1: Storage & CLI | Complete | 100% |
| Phase 2: OAuth Flows | Complete | 100% |
| Phase 3: Daemon & API | In Progress | ~80% |
| Phase 4: Resolution | In Progress | ~20% |
| Phase 5: Expansion | Not Started | 0% |

---

## Phase 0: Scaffolding & Core Types

**Status:** COMPLETE

**Goal**: Establish project structure and define domain model.

### Tasks

- [x] Create workspace with three crates:
  - `sigilforge-core` (library)
  - `sigilforge-daemon` (binary)
  - `sigilforge-cli` (binary)

- [x] Write foundational documentation:
  - README.md with project overview
  - ARCHITECTURE.md with design decisions
  - INTERFACES.md with trait definitions
  - ROADMAP.md (this file)

- [x] Define core domain types in `sigilforge-core`:
  - `ServiceId` - identifier for a service (spotify, gmail, etc.)
  - `AccountId` - identifier for an account within a service
  - `Account` - full account metadata (service, id, scopes, created_at)
  - `CredentialRef` - pointer to a stored credential
  - `Token` - access token with expiry
  - `TokenSet` - access + refresh token pair

- [x] Define trait stubs:
  - `SecretStore` - store/retrieve secrets
  - `TokenManager` - ensure valid tokens
  - `ReferenceResolver` - resolve auth:// URIs

- [x] Implement `MemoryStore`:
  - In-memory `SecretStore` for testing
  - No persistence; HashMap-based

### Deliverables

- Compiling workspace with placeholder mains
- Type definitions with serde derives
- Trait definitions with documentation
- Unit tests for core types

---

## Phase 1: Basic Storage & CLI

**Status:** COMPLETE

**Goal**: Working CLI with OS keyring storage and mock auth.

### Tasks

- [x] Implement `KeyringStore`:
  - Wrap `keyring` crate
  - Handle platform differences (libsecret, Keychain, Credential Manager)
  - Key naming convention: `sigilforge/{service}/{account}/{type}`

- [x] Implement account management:
  - `AccountStore` struct to manage accounts.json
  - CRUD operations for accounts
  - Persist to `~/.config/sigilforge/accounts.json`

- [x] Build CLI commands:
  - `sigilforge add-account <service> <account>` - Add account (prompts for API key)
  - `sigilforge list-accounts` - List all accounts
  - `sigilforge get-token <service> <account>` - Retrieve token/key
  - `sigilforge remove-account <service> <account>` - Delete account

- [x] Add configuration loading:
  - `Config` struct with serde
  - Load from `~/.config/sigilforge/config.toml`
  - Defaults for missing config

- [x] Add mock token provider:
  - Return static tokens for testing
  - Simulate token expiry

### Deliverables

- CLI that can add accounts with API keys
- Credentials stored in OS keyring
- Account metadata persisted to TOML
- Integration tests for KeyringStore

---

## Phase 2: Real OAuth Flows

**Status:** COMPLETE

**Goal**: Working OAuth2 authentication for initial providers.

### Tasks

- [x] Implement OAuth2 flow infrastructure:
  - `OAuthFlow` trait for different flow types
  - Auth code + PKCE flow implementation
  - Device code flow implementation
  - Local callback server for auth code flow

- [x] Add provider configurations:
  - `ProviderConfig` struct with endpoints, scopes
  - Built-in configs for:
    - GitHub (device code)
    - Spotify (auth code + PKCE)
    - Google (auth code + PKCE)

- [x] Implement token refresh:
  - `TokenManager::ensure_access_token()` implementation
  - Automatic refresh before expiry
  - Store updated tokens

- [x] Update CLI for OAuth:
  - `add-account` starts OAuth flow for configured providers
  - Progress output during flow
  - Error handling for user cancellation

- [x] Add error types:
  - `AuthError` enum for auth failures
  - `StoreError` enum for storage failures
  - Proper error propagation

### Deliverables

- Working OAuth flow for GitHub
- Working OAuth flow for Spotify
- Token refresh working automatically
- CLI guides user through OAuth

---

## Phase 3: Daemon & Socket API

**Status:** ~80% Complete

**Goal**: Background service with local API for client applications.

### Tasks

- [x] Implement daemon core:
  - Async runtime setup (tokio)
  - Signal handling (SIGTERM, SIGINT)
  - PID file management
  - Logging to file

- [x] Implement socket server:
  - Unix socket on Linux/macOS
  - Named pipe on Windows
  - JSON-RPC 2.0 protocol
  - Connection handling

- [x] Implement API handlers:
  - `get_token` - **STUB** (see issue #21)
  - `list_accounts` - return account list
  - `get_account` - return single account
  - `add_account` - initiate account setup
  - `remove_account` - delete account
  - `refresh_token` - force refresh
  - `resolve` - **STUB** (see issue #22)
  - `status` - daemon health

- [x] Add daemon management to CLI:
  - `sigilforge daemon start` - start daemon
  - `sigilforge daemon stop` - stop daemon
  - `sigilforge daemon status` - check status
  - Auto-start daemon if not running

- [x] Update CLI to use daemon:
  - Connect to socket by default
  - Fall back to direct mode if daemon unavailable
  - `--direct` flag to bypass daemon

- [x] Add client library:
  - `SigilforgeClient` struct for Rust consumers
  - Connect to daemon
  - Typed request/response

**Note:** `get_token` and `resolve` RPC handlers currently return stub values. See issues #21 and #22 for wiring to actual implementations.

### Deliverables

- Daemon runs in background
- CLI communicates via socket
- Client library for Rust apps
- Scryforge can request tokens via daemon

---

## Phase 4: Reference Resolution & Encrypted Storage

**Status:** ~20% Complete

**Goal**: Full reference resolution and ROPS/SOPS integration.

### Tasks

- [x] Implement auth:// URI resolution:
  - Parse `auth://service/account/token` format
  - Parse `auth://service/account/api_key` format
  - `ReferenceResolver::resolve()` implementation

- [ ] Add vals-style reference support:
  - Detect `vals:ref+...` syntax
  - Shell out to `vals` for external resolution
  - Cache resolved values

- [ ] Implement `EncryptedFileStore`:
  - ROPS integration (Rust-native)
  - SOPS fallback via CLI
  - Support age and GPG encryption
  - Key from environment or config

- [ ] Add reference resolution to daemon API:
  - `resolve` method handles any reference type
  - Automatic backend detection
  - Error on unresolvable references

- [ ] Configuration for encrypted files:
  - Specify ROPS/SOPS file paths
  - Key configuration
  - Auto-decrypt on read

### Deliverables

- `auth://` URIs resolve to credentials
- vals references resolve via external tool
- ROPS-encrypted config files supported
- Full integration with Scryforge reference system

---

## Phase 5: Additional Providers & Polish

**Status:** Not Started

**Goal**: Broad provider support and production hardening.

### Tasks

- [ ] Add more OAuth providers:
  - Google (Gmail, Drive, Calendar)
  - Microsoft (Outlook, Graph)
  - Reddit
  - Discord
  - Twitch
  - Twitter/X

- [ ] Add provider auto-detection:
  - Infer provider from service name
  - Custom provider config support

- [ ] Improve error handling:
  - Detailed error messages
  - Recovery suggestions
  - Retry logic for transient failures

- [ ] Add monitoring:
  - Token expiry warnings
  - Account health checks
  - Metrics export (optional)

- [ ] Security hardening:
  - Audit logging
  - Rate limiting
  - Input validation

- [ ] Documentation:
  - Provider setup guides
  - Troubleshooting guide
  - API reference

### Deliverables

- 10+ supported OAuth providers
- Robust error handling
- Production-ready daemon
- Comprehensive documentation

---

## Future Considerations

### Potential Phase 6+ Features

- **TUI**: Fusabi-based terminal UI for account management
- **Web UI**: Optional local web interface for OAuth flows
- **Multi-machine sync**: Encrypted credential sync across machines
- **Team features**: Shared credentials with access control
- **Audit log**: Track credential access for compliance
- **HSM support**: Hardware security module integration

### Non-Goals (Explicitly Out of Scope)

- Network-accessible vault service
- Multi-tenant credential management
- Built-in secret rotation
- Cloud-hosted backend
