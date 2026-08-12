# Next Steps for Sigilforge Development

This document lists what is actually built and what is worth doing next. It is
meant to be self-contained, so a session can pick up work without other context.

For the phase-by-phase history, see [ROADMAP.md](ROADMAP.md).

## Current State (v0.3.0)

Phases 0-3 are complete and Phase 4.5 (GitHub App auth) has landed. This is a
working credential manager, not a scaffold.

### What works

| Area | State |
|------|-------|
| Domain model | `ServiceId`, `AccountId`, `Account`, `CredentialRef`, `CredentialType` |
| Storage | `MemoryStore` and `KeyringStore` behind the `SecretStore` trait |
| Account metadata | `AccountStore`, persisted to `~/.config/sigilforge/accounts.json` |
| OAuth | Auth code + PKCE and device code flows; `DefaultTokenManager` refreshes |
| GitHub App | RS256 JWT -> installation token, cached; `GitHubAppTokenManager` |
| Daemon | JSON-RPC 2.0 over Unix socket; handlers wired to real implementations |
| CLI | `add-account`, `list-accounts`, `get-token`, `remove-account`, `resolve`, `github-app *` |
| Client library | `sigilforge-client` (`DaemonClient`) |
| TUI | `sigilforge-tui`, plus a `scarab-sigilforge` status-bar plugin |
| Resolution | `auth://` URIs resolve, including GitHub App installation tokens |

### What is not built

| Gap | Where |
|-----|-------|
| `vals:ref+...` resolution | `resolve.rs` returns `ExternalError("not yet implemented")` |
| `EncryptedFileStore` (ROPS/SOPS) | Not started; documented in ARCHITECTURE.md as a future backend |
| `KeyringStore::list_keys` | Returns `BackendError`; platform keyrings cannot enumerate |
| Daemon-side GitHub App RPC | The CLI's `github-app` commands talk to core directly |
| Providers beyond GitHub/Spotify/Google | `ProviderRegistry::with_defaults` |

### Known rough edges

These are real, small, and worth fixing:

1. **`token_expiry` has two encodings.** `DefaultTokenManager::store_token_set`
   writes a Unix timestamp; the CLI's direct `add-account` path writes RFC 3339.
   A token written by one and read by the other looks like it has no expiry. The
   GitHub App code reads both as a workaround (`github_app::token::parse_expiry`);
   the underlying inconsistency should be settled on one format.

2. **`fallback_get_token` cannot refresh.** `sigilforge-cli/src/main.rs` warns
   "Refresh not yet implemented" and tells the user to re-authenticate, even
   though `DefaultTokenManager` can refresh. The direct-mode path should use it.

3. **`KeyringStore::try_new` does not prove the keyring works.** It only
   constructs an `Entry`. On a headless machine it succeeds and later reads fail,
   which is why `create_store`'s documented "fall back to memory if unavailable"
   does not actually trigger. `GitHubAppTokenManager::register` works around this
   by reading the key back after writing; a `probe()` on the store would fix it
   generally.

4. **`sigilforge daemon` (CLI subcommand) is a stub.** It prints
   `[stub] Running daemon in foreground...` and sleeps. The real daemon is the
   separate `sigilforged` binary.

5. **`sigilforge-tui/src/app.rs`** has a `TODO: Implement proper daemon RPC call`.

## Suggested next work

### 1. Finish Phase 4: vals and encrypted file storage

`ResolverConfig` already has `enable_vals`, `vals_path`, and `cache_ttl_secs`
fields that nothing reads.

- Shell out to `vals` for `vals:ref+...` references
- Honour `cache_ttl_secs` for resolved values
- Implement `EncryptedFileStore` over ROPS (Rust-native) with a SOPS CLI fallback

### 2. Unify token expiry storage

Pick RFC 3339 (it is self-describing and already what the CLI writes), migrate
`DefaultTokenManager`, and read both formats for one release.

### 3. Expose GitHub App operations over the daemon

`get_token` currently only understands OAuth accounts. Teaching the daemon about
`github-app` would let several processes share one cached installation token
instead of each minting its own.

### 4. More OAuth providers

`ProviderRegistry::with_defaults` covers GitHub, Spotify, and Google. Microsoft
Graph, Reddit, and Discord are the obvious next ones.

## Build and Verification

The `justfile` has recipes for everything CI runs:

```bash
just ci        # fmt-check + clippy + test
just build     # cargo build
just test      # cargo test --workspace
just clippy    # cargo clippy --workspace -- -D warnings
```

CI is stricter than `just clippy` - it runs:

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --doc
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

Doc tests run in CI, so examples in `///` comments must compile.

### System dependencies

`KeyringStore` uses the Secret Service on Linux, which needs `libdbus-1-dev` and
`pkg-config` at build time. See [CONTRIBUTING.md](../CONTRIBUTING.md).

## Layout

```text
sigilforge/
├── sigilforge-core/
│   └── src/
│       ├── lib.rs
│       ├── model.rs            # ServiceId, AccountId, CredentialRef, CredentialType
│       ├── error.rs
│       ├── store/              # SecretStore trait, memory + keyring backends
│       ├── token.rs            # Token, TokenSet, TokenManager trait
│       ├── token_manager.rs    # DefaultTokenManager (OAuth)
│       ├── resolve.rs          # ReferenceResolver, DefaultReferenceResolver
│       ├── account_store.rs
│       ├── provider.rs
│       ├── oauth/              # pkce.rs, device_code.rs
│       └── github_app/         # mod.rs, jwt.rs, token.rs, argocd.rs
├── sigilforge-daemon/          # JSON-RPC server (sigilforged)
├── sigilforge-cli/             # sigilforge binary; github_app.rs holds App commands
├── sigilforge-client/          # DaemonClient for Rust consumers
├── sigilforge-tui/
├── scarab-sigilforge/
└── docs/
    ├── ARCHITECTURE.md
    ├── INTERFACES.md
    ├── ROADMAP.md
    ├── GITHUB_APP.md           # GitHub App setup and Argo CD integration
    ├── NEXT_STEPS.md
    ├── TROUBLESHOOTING.md
    ├── RELEASE.md
    └── STRUCTURE.md
```

## Integration with Scryforge

Two modes, both working today.

### Library mode (embedded)

```rust,ignore
use sigilforge_core::{TokenManager, DefaultTokenManager, KeyringStore};

let store = KeyringStore::try_new("sigilforge")?;
let manager = DefaultTokenManager::new(store, providers);
let token = manager.ensure_access_token(&service, &account).await?;
```

### Daemon mode (IPC)

```rust,ignore
use sigilforge_client::DaemonClient;

let client = DaemonClient::connect().await?;
let token = client.get_token("spotify", "personal").await?;
```

Daemon mode is preferred for sharing tokens across processes, centralising
refresh, and avoiding concurrent keyring access.
