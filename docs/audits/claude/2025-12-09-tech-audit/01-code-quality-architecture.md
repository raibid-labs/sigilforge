# Code Quality & Architecture Audit

## Overall Assessment: 7.5/10

The project demonstrates excellent separation of concerns with clean layering across four crates. Code quality is generally high with good use of Rust idioms, but several critical issues need attention.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    sigilforge-cli                           │
│                 (Binary: CLI tool)                          │
└─────────────────────┬───────────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────────┐
│                  sigilforge-client                          │
│              (Library: Client SDK)                          │
└─────────────────────┬───────────────────────────────────────┘
                      │ Unix Socket / JSON-RPC
┌─────────────────────▼───────────────────────────────────────┐
│                  sigilforge-daemon                          │
│             (Binary: Background Service)                    │
└─────────────────────┬───────────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────────┐
│                   sigilforge-core                           │
│          (Library: Domain Models & Traits)                  │
└─────────────────────────────────────────────────────────────┘
```

## Strengths

### 1. Clean Crate Boundaries
- **sigilforge-core**: Domain models, traits, storage abstraction
- **sigilforge-daemon**: JSON-RPC server, account management
- **sigilforge-client**: Client interface with fallback strategies
- **sigilforge-cli**: Command-line tool

### 2. Excellent Trait Design
```rust
// Well-designed extension points
pub trait SecretStore: Send + Sync { ... }
pub trait TokenManager: Send + Sync { ... }
pub trait ReferenceResolver: Send + Sync { ... }
```

### 3. Strong Type System Usage
- `ServiceId` normalizes to lowercase, preventing mismatches
- `AccountId` prevents mixing service and account IDs
- `Secret` type redacts in Debug/Display, requires explicit `expose()`
- `CredentialRef` prevents malformed auth:// URIs

### 4. Good Dependency Management
- Workspace-level dependency versioning prevents fragmentation
- Feature flags well-designed: `keyring-store`, `oauth`, `full`

## Critical Issues

### Issue 1: Daemon RPC Returns Stub Tokens

**Location:** `sigilforge-daemon/src/api/handlers.rs:176-181`

```rust
// TODO: Integrate with actual token manager
// For now, return a stub token
Ok(GetTokenResponse {
    token: format!("stub_token_for_{}_{}", service, account),
    expires_at: Some("2025-12-07T00:00:00Z".to_string()),
})
```

**Impact:** Core functionality non-functional
**Fix:** Wire to `DefaultTokenManager::ensure_access_token()`

### Issue 2: Sync Locks in Async Context

**Location:** `sigilforge-core/src/account_store.rs:90-95`

```rust
// std::sync::RwLock blocks the async runtime
let accounts = self.accounts.read().map_err(...)?;
```

**Impact:** Thread starvation under concurrent load
**Fix:** Use `tokio::sync::RwLock` or `parking_lot::RwLock`

### Issue 3: Lock Poisoning in Memory Store

**Location:** `sigilforge-core/src/store/memory.rs:54-87`

```rust
let data = self.data.read().map_err(|e| StoreError::BackendError {
    message: format!("lock poisoned: {}", e),
})?;
```

**Impact:** Panic = permanent failure, no recovery
**Fix:** Use `parking_lot::RwLock` (doesn't poison)

### Issue 4: Error Type Duplication

**Locations:**
- `sigilforge-core/src/error.rs`
- `sigilforge-client/src/types.rs:84-133`

Same concepts (`DaemonUnavailable`, `NetworkError`) defined twice, violating DRY.

**Fix:** Share core error types or use trait-based error abstraction

## High Priority Issues

### Issue 5: Silent Error Suppression

**Location:** `sigilforge-core/src/token_manager.rs:438-441`

```rust
let _ = self.store.delete(&access_key).await;
let _ = self.store.delete(&refresh_key).await;
let _ = self.store.delete(&expiry_key).await;
```

Revoke operation silently ignores deletion failures.

### Issue 6: Inconsistent RPC Parameter Styles

**Location:** `sigilforge-daemon/src/api/server.rs:168-236`

- `get_token` uses array-based params
- `list_accounts` uses optional first param
- `add_account` uses array with multiple elements

Magic index access (`params[0]`, `params[1]`) is error-prone.

### Issue 7: Unbounded Task Spawning

**Location:** `sigilforge-daemon/src/api/server.rs:66-72`

```rust
tokio::spawn(async move {
    if let Err(e) = handle_connection(stream, api).await { ... }
});
```

No limit on concurrent connections - DoS vulnerability.

### Issue 8: No Line Length Limits in RPC

**Location:** `sigilforge-daemon/src/api/server.rs:101-138`

```rust
let mut line = String::new();
let n = reader.read_line(&mut line).await?;
```

Malicious client could send arbitrarily long lines, causing memory exhaustion.

## Medium Priority Issues

### Issue 9: Missing Input Validation

**Location:** `sigilforge-core/src/model.rs:33-35`

`ServiceId` accepts any string, only converts to lowercase. No validation of format (spaces, special chars accepted).

### Issue 10: Secret Comparison Uses `==`

**Location:** `sigilforge-core/src/store/mod.rs:78-84`

Standard equality is timing-sensitive. Should use `subtle::ConstantTimeEq` for cryptographic correctness.

### Issue 11: HTTP Client Per-Manager

**Location:** `sigilforge-core/src/token_manager.rs:85`

Creates new `reqwest::Client` for every `DefaultTokenManager`. Should reuse singleton.

### Issue 12: String Cloning in List Operations

**Location:** `sigilforge-core/src/account_store.rs:214-215`

```rust
.cloned()
.collect()
```

Clones entire `Vec<Account>` on every list operation. With thousands of accounts, this becomes expensive.

## Code Quality Metrics

| Metric | Value | Assessment |
|--------|-------|------------|
| Total LOC | ~4,600 | Reasonable size |
| Crate count | 4 | Well-organized |
| Public traits | 5 | Good abstraction |
| Error types | 6 | Comprehensive |
| `#[allow(...)]` | 3 | Minimal lint suppression |
| Unsafe blocks | 1 | Minimal unsafe code |
| TODO comments | 2 | Known gaps documented |

## Recommendations

### Immediate
1. Wire daemon stubs to actual implementations
2. Replace `std::sync::RwLock` with `tokio::sync::RwLock`
3. Add connection limits to socket server
4. Add line length limits to RPC parser

### Short-term
1. Deduplicate error types
2. Add input validation to ServiceId/AccountId
3. Implement constant-time secret comparison
4. Fix silent error suppression

### Long-term
1. Consider `parking_lot` over `std::sync`
2. Implement metrics/observability
3. Add property-based testing for parsers
