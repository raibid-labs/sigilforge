# Sigilforge Development Roadmap

This document outlines the development phases for Sigilforge, from initial scaffolding to full ecosystem integration.

## Current Status

**Version:** v0.3.0
**Overall Completion:** ~70%

| Phase | Status | Completion |
|-------|--------|------------|
| Phase 0: Scaffolding | Complete | 100% |
| Phase 1: Storage & CLI | Complete | 100% |
| Phase 2: OAuth Flows | Complete | 100% |
| Phase 3: Daemon & API | Complete | 100% |
| Phase 4: Resolution & Encrypted Storage | In Progress | ~75% |
| Phase 4.5: GitHub App Auth | Complete | 100% |
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

**Status:** COMPLETE

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
  - `get_token` - wired to the real `TokenManager`
  - `list_accounts` - return account list
  - `get_account` - return single account
  - `add_account` - initiate account setup
  - `remove_account` - delete account
  - `refresh_token` - force refresh
  - `resolve` - wired to the real `ReferenceResolver`
  - `status` - daemon health
  - `accounts_status` - per-account expiry for status-bar plugins

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

### Deliverables

- Daemon runs in background
- CLI communicates via socket
- Client library for Rust apps
- Scryforge can request tokens via daemon

---

## Phase 4: Reference Resolution & Encrypted Storage

**Status:** ~75% Complete

**Goal**: Full reference resolution, and secret storage that works on a server.

### Tasks

- [x] Implement auth:// URI resolution:
  - Parse `auth://service/account/token` format
  - Parse `auth://service/account/api_key` format
  - `ReferenceResolver::resolve()` implementation

- [ ] Add vals-style reference support:
  - Detect `vals:ref+...` syntax
  - Shell out to `vals` for external resolution
  - Cache resolved values

- [x] Implement `EncryptedFileStore`:
  - **age** (the `age` crate, Rust-native) rather than ROPS or a SOPS CLI
    fallback. The driving requirement was a DGX Spark reached over SSH, where a
    missing `sops`/`age`/`gpg` binary is the same class of failure as the
    missing D-Bus session bus that started this. A library has no such failure
    mode, and the file it writes is still readable by the standard `age` and
    `rage` CLIs, so nothing is locked in.
  - GPG is deliberately not supported: it wants an agent and, on a locked key, a
    prompt. A daemon and a CI job have no one to prompt.
  - X25519 identity file, `0600`, in the **config** directory; the ciphertext in
    the **data** directory, so backing up one does not back up the other
  - Refuses an identity file that group or other can read
  - Whole-store atomic rewrite (temp file, `fsync`, rename) under an advisory
    lock, so a crash cannot truncate it
  - Unlike the platform keyrings, it can enumerate: `list_keys` works
  - First-run: `sigilforge store init`

- [x] Honest backend selection (`open_store`):
  - `SIGILFORGE_STORE_BACKEND`, then `[storage] backend` in
    `~/.config/sigilforge/config.toml`, then automatic
  - Every backend is **probed** - a real keyring round-trip, a real decrypt -
    rather than merely constructed
  - An explicitly requested backend is never silently replaced
  - **No silent fallback to `MemoryStore`.** That fallback made an unreachable
    store indistinguishable from an empty one, which is how a registered GitHub
    App came to be reported as unregistered. `memory` is now reachable only by
    name.

- [ ] Add reference resolution to daemon API:
  - `resolve` method handles any reference type
  - Automatic backend detection
  - Error on unresolvable references

- [x] Configuration for encrypted files:
  - `[storage] backend`, `identity_file`, `secrets_file` in `config.toml`
  - `SIGILFORGE_AGE_IDENTITY` / `SIGILFORGE_SECRETS_FILE` overrides
  - Decrypts on read with no prompt, so daemons and CI work unattended

### Deliverables

- `auth://` URIs resolve to credentials
- Sigilforge is usable on a headless host: register in one process, read in the
  next, with no D-Bus session bus
- Storage failures are reported as storage failures, never as missing credentials
- vals references resolve via external tool *(outstanding)*
- Full integration with Scryforge reference system

---

## Phase 4.5: GitHub App Authentication

**Status:** COMPLETE

**Goal**: Machine-to-machine credentials for private repositories, driven by the
first real consumer: Argo CD on the DGX Spark cluster syncing
`raibid-labs/raibid-fish` and `raibid-labs/spark-infra`.

GitHub App auth is a mechanism the existing OAuth machinery cannot express. There
is no authorization code, no browser, and no refresh token; identity is proved by
signing an RS256 JWT with the App's private key, which is then exchanged for an
installation token. See [GITHUB_APP.md](GITHUB_APP.md).

### Tasks

- [x] `GitHubAppCredential` in `sigilforge-core`:
  - App ID, installation ID, PEM private key
  - Key validated on construction, stored in the `SecretStore`, redacted from
    `Debug`/`Display`

- [x] RS256 JWT construction (`github_app::jwt`):
  - `iat` backdated 60s for clock skew, `exp` clamped to GitHub's 10-minute limit
  - PKCS#1 v1.5 signing via the `rsa` crate
  - Accepts PKCS#1 and PKCS#8 PEM

- [x] Installation-token minting and caching (`github_app::token`):
  - `GitHubAppTokenManager`, a **sibling** of `DefaultTokenManager` - the OAuth
    refresh path (`refresh_token` -> token endpoint) does not apply
  - Re-mints only within 5 minutes of `expires_at`
  - Implements `TokenManager` and `ReferenceResolver` so it composes with
    existing generic code

- [x] `auth://` resolution:
  - `auth://github-app/{account}/installation_token`
  - New `CredentialType` variants: `AppId`, `InstallationId`, `PrivateKey`,
    `InstallationToken`
  - `DefaultReferenceResolver::with_github_app` routes these to the App manager

- [x] CLI subcommands:
  - `sigilforge github-app register|list|token|argocd-secret|remove`

- [x] Argo CD consumer path:
  - `argocd-secret` renders a labelled repository `Secret` to **stdout**
  - Never applied to a cluster by Sigilforge; pipe to `kubectl` or encrypt

- [x] Fix `KeyringStore` persistence:
  - The `keyring` crate ships no credential store in its default features, so
    every "stored in the OS keyring" write was going to an in-process mock and
    vanishing at exit. Platform backends are now selected explicitly.

### Deliverables

- Argo CD can sync private repositories using an org-owned, repo-scoped App
- CI and local tooling get a GitHub API token from `sigilforge github-app token`
- No personal access token anywhere in the path

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
