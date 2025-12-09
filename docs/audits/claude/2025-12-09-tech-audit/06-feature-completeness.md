# Feature Completeness Audit

## Overall Assessment: 5/10

Sigilforge has solid foundational infrastructure with working account management and storage backends. However, the daemon's core RPC methods are stubs returning hardcoded values, OAuth flows exist but aren't accessible from the API layer, and the CLI operates in degraded fallback mode for advanced features.

## Implementation Status Overview

```
Core Types & Storage    [##########] 100%
Account Management      [##########] 100%
Daemon Infrastructure   [########--]  80%
OAuth Integration       [#---------]  10%
Token Retrieval via RPC [----------]   0%
Reference Resolution    [----------]   0%
CLI Commands            [#####-----]  50%
Security Controls       [#---------]  10%
```

## Feature Matrix

### Fully Implemented (Working)

| Feature | Location | Status |
|---------|----------|--------|
| ServiceId, AccountId types | `sigilforge-core/src/model.rs` | ✓ |
| Account type with metadata | `sigilforge-core/src/model.rs` | ✓ |
| CredentialRef parsing | `sigilforge-core/src/model.rs` | ✓ |
| MemoryStore backend | `sigilforge-core/src/store/memory.rs` | ✓ |
| KeyringStore backend | `sigilforge-core/src/store/keyring.rs` | ✓ |
| AccountStore persistence | `sigilforge-core/src/account_store.rs` | ✓ |
| Provider registry | `sigilforge-core/src/provider.rs` | ✓ |
| Token expiry detection | `sigilforge-core/src/token.rs` | ✓ |
| Daemon socket server | `sigilforge-daemon/src/api/server.rs` | ✓ |
| Graceful shutdown | `sigilforge-daemon/src/api/server.rs` | ✓ |
| JSON-RPC protocol | `sigilforge-daemon/src/api/server.rs` | ✓ |
| RPC: list_accounts | `sigilforge-daemon/src/api/handlers.rs` | ✓ |
| RPC: add_account | `sigilforge-daemon/src/api/handlers.rs` | ✓ |
| CLI argument parsing | `sigilforge-cli/src/main.rs` | ✓ |
| Client library | `sigilforge-client/src/` | ✓ |
| Fallback strategies | `sigilforge-client/src/fallback.rs` | ✓ |

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| DefaultTokenManager | Trait defined, refresh logic exists | Not wired to daemon RPC |
| ReferenceResolver | Trait defined, URI parsing works | No DefaultResolver impl |
| OAuth PKCE flow | Code exists in oauth/pkce.rs | Not callable from daemon |
| OAuth Device flow | Code exists in oauth/device_code.rs | Not callable from daemon |
| CLI add-account | Creates account metadata | Doesn't start OAuth flow |
| CLI get-token | Parses arguments | Returns stub value |
| CLI resolve | Parses URI | Returns stub value |

### Not Implemented (Stubs)

| Feature | Location | Current Behavior |
|---------|----------|------------------|
| RPC: get_token | handlers.rs:176 | Returns `stub_token_for_{service}_{account}` |
| RPC: resolve | handlers.rs:271 | Returns `resolved_{service}_{account}_{type}` |
| CLI daemon command | main.rs:409 | Infinite sleep loop |
| EncryptedFileStore | Documented | NOT in codebase |
| vals-style references | Documented | NOT implemented |

## Documented vs Implemented

### README Claims vs Reality

| Claim | Reality | Gap |
|-------|---------|-----|
| "Stores credentials securely" | Keyring backend works | Tokens not stored via daemon |
| "Runs OAuth flows" | Code exists | Not accessible from API |
| "Manages token lifecycles" | Logic exists | Not callable via RPC |
| "Resolves credential references" | Parsing works | Resolution returns stubs |
| "OAuth providers: Google, Microsoft, Spotify, Reddit, GitHub" | Only GitHub, Spotify, Google | Microsoft, Reddit missing |

### INTERFACES.md vs Implementation

| Documented Method | Implementation Status |
|-------------------|----------------------|
| `get_token` | Stub |
| `list_accounts` | Working |
| `add_account` | Working (no OAuth) |
| `remove_account` | Not in RPC API |
| `refresh_token` | Not exposed via RPC |
| `resolve` | Stub |
| `status` | Not implemented |

## Stub Code Locations

### Daemon RPC Stubs

**File:** `sigilforge-daemon/src/api/handlers.rs`

```rust
// Line 176-181: get_token() stub
// TODO: Integrate with actual token manager
Ok(GetTokenResponse {
    token: format!("stub_token_for_{}_{}", service, account),
    expires_at: Some("2025-12-07T00:00:00Z".to_string()),
})

// Line 271-278: resolve() stub
// TODO: Integrate with actual resolver
Ok(ResolveResponse {
    value: format!("resolved_{}_{}_{}",
        cred_ref.service, cred_ref.account, cred_ref.credential_type
    ),
})
```

### CLI Stubs

**File:** `sigilforge-cli/src/main.rs`

```rust
// Line 180: OAuth flow stub
println!("  [stub] Would start OAuth flow to obtain tokens here");

// Line 282-289: Token retrieval stub
println!("[stub] Getting token for {}/{}", service, account);

// Line 399: Resolution stub
println!("[stub] Would resolve to actual value");

// Line 409: Daemon stub
println!("[stub] Running daemon in foreground...");
```

## OAuth Provider Status

### Configured Providers

| Provider | Auth URL | Token URL | PKCE | Device Code |
|----------|----------|-----------|------|-------------|
| GitHub | ✓ | ✓ | ✓ | ✓ |
| Spotify | ✓ | ✓ | ✓ | ✗ |
| Google | ✓ | ✓ | ✓ | ✓ |

### Missing Providers (Documented)

- Microsoft (documented in README, not configured)
- Reddit (documented in README, not configured)

## CLI Command Status

| Command | Arguments | Status | Notes |
|---------|-----------|--------|-------|
| `add-account` | `<service> <account> [--scopes]` | Partial | Creates metadata, no OAuth |
| `list-accounts` | `[--service <filter>]` | Working | Via daemon or fallback |
| `get-token` | `<service> <account> [--format]` | Stub | Returns fake token |
| `remove-account` | `<service> <account> [--force]` | Working | Deletes from store |
| `resolve` | `<reference>` | Stub | Parses URI only |
| `daemon` | None | Stub | Infinite sleep loop |
| `--verbose` | Global | Working | Enables debug logging |

## RPC Method Inventory

### Implemented and Working

| Method | Request | Response |
|--------|---------|----------|
| `list_accounts` | `{service?: string}` | `{accounts: Account[]}` |
| `add_account` | `{service, account, scopes}` | `{success: bool}` |

### Implemented but Non-Functional

| Method | Request | Response | Issue |
|--------|---------|----------|-------|
| `get_token` | `{service, account}` | `{token, expires_at}` | Hardcoded stub |
| `resolve` | `{reference}` | `{value}` | Hardcoded stub |

### Not Implemented (Documented)

| Method | Notes |
|--------|-------|
| `remove_account` | Trait exists, not in RPC |
| `refresh_token` | In TokenManager, not in RPC |
| `revoke_tokens` | In TokenManager, not in RPC |
| `get_token_info` | In TokenManager, not in RPC |
| `status` | Mentioned in ARCHITECTURE.md |

## Integration Gaps

### Gap 1: TokenManager Not Wired to Daemon

The daemon has `AccountStore` but no `TokenManager`:

```rust
pub struct ApiState {
    pub accounts: AccountStore,
    // Missing: token_manager: DefaultTokenManager
    // Missing: secret_store: KeyringStore
}
```

### Gap 2: OAuth Flows Unreachable

OAuth code exists but isn't callable:

```
sigilforge-core/src/oauth/
├── pkce.rs         # PKCE flow (not accessible)
├── device_code.rs  # Device flow (not accessible)
└── mod.rs          # OAuth client creation
```

No code path from `add_account` RPC to OAuth flows.

### Gap 3: ReferenceResolver Not Implemented

```rust
pub trait ReferenceResolver: Send + Sync {
    async fn resolve(&self, reference: &str) -> Result<ResolvedValue, ResolveError>;
    // ...
}

// No DefaultResolver implementation exists
```

## Test Coverage of Features

| Feature | Test Coverage |
|---------|--------------|
| Account CRUD | Excellent (19 tests) |
| Token expiry | Good (11 tests) |
| Daemon shutdown | Good (3 tests) |
| RPC list_accounts | Tested |
| RPC add_account | Tested |
| RPC get_token | Tested (stub behavior) |
| OAuth flows | Not tested via RPC |
| CLI commands | Not tested |

## Feature Flag Coverage

```toml
[features]
default = ["keyring-store"]
keyring-store = ["dep:keyring"]      # ✓ Working
oauth = ["dep:oauth2", ...]          # ✓ Compiles, not integrated
full = ["keyring-store", "oauth"]    # ✓ Compiles
```

All feature combinations compile but `oauth` features aren't exposed via daemon.

## Recommendations

### Critical (Wire Stubs to Implementations)

1. **Wire get_token to TokenManager**
   - Create `DefaultTokenManager` in daemon
   - Call `ensure_access_token()` in RPC handler
   - Return actual token from storage

2. **Wire resolve to ReferenceResolver**
   - Implement `DefaultReferenceResolver`
   - Call resolver in RPC handler
   - Return actual resolved value

3. **Integrate OAuth flows**
   - Add OAuth flow trigger to `add_account`
   - Implement CLI prompts for authorization
   - Store tokens via TokenManager

### High Priority

4. **Implement missing providers** (Microsoft, Reddit)
5. **Expose remove_account via RPC**
6. **Expose refresh_token via RPC**
7. **Implement CLI daemon command**

### Medium Priority

8. **Add status RPC method**
9. **Implement EncryptedFileStore** (or remove from docs)
10. **Add token introspection endpoint**

## Completion Estimate

| Component | Current | To Production |
|-----------|---------|---------------|
| Core library | 90% | +5% (validation) |
| Daemon | 40% | +50% (integration) |
| CLI | 50% | +30% (OAuth UI) |
| Client | 90% | +5% (error handling) |
| Security | 10% | +70% (auth/authz) |
| **Overall** | **~50%** | **+40%** |
