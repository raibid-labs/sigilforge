# Prioritized Recommendations

## Overview

This document provides actionable recommendations derived from the comprehensive tech audit. Items are organized by priority and include effort estimates and file references.

## Priority Levels

- **P0 (Critical):** Blocking issues, security vulnerabilities, broken functionality
- **P1 (High):** Major gaps affecting core functionality or DX
- **P2 (Medium):** Important improvements for production readiness
- **P3 (Low):** Nice-to-haves and polish

---

## P0: Critical (Fix Immediately)

### 1. Fix Test Compilation Errors

**Issue:** Unsafe `std::env::set_var` calls without `unsafe` blocks
**Impact:** Tests don't compile
**Effort:** 30 minutes

**Files:**
- `sigilforge-client/src/client.rs:342,350,355,362`
- `sigilforge-client/src/fallback.rs:241,249,254,261,266,273,287,297`
- `sigilforge-client/src/lib.rs:137,144`

**Fix:**
```rust
// Before
std::env::set_var("KEY", "value");

// After
unsafe { std::env::set_var("KEY", "value"); }
```

### 2. Wire get_token RPC to TokenManager

**Issue:** Returns hardcoded stub tokens
**Impact:** Core functionality non-functional
**Effort:** 2-4 hours

**File:** `sigilforge-daemon/src/api/handlers.rs:176-181`

**Tasks:**
- Add `DefaultTokenManager` to `ApiState`
- Add `SecretStore` (KeyringStore) to `ApiState`
- Call `token_manager.ensure_access_token()` in handler
- Return actual token from storage

### 3. Wire resolve RPC to ReferenceResolver

**Issue:** Returns hardcoded stub values
**Impact:** Reference resolution non-functional
**Effort:** 2-4 hours

**File:** `sigilforge-daemon/src/api/handlers.rs:271-278`

**Tasks:**
- Implement `DefaultReferenceResolver`
- Add resolver to `ApiState`
- Call resolver in handler
- Return actual resolved value

### 4. Add Socket Permission Management

**Issue:** Socket created with default permissions
**Impact:** Security vulnerability - other users can access
**Effort:** 1 hour

**File:** `sigilforge-daemon/src/api/server.rs:46-47`

**Fix:**
```rust
let listener = UnixListener::bind(socket_path)?;
std::fs::set_permissions(socket_path,
    std::fs::Permissions::from_mode(0o600))?;
```

### 5. Add Peer Credential Verification

**Issue:** No authentication on socket connections
**Impact:** Any process can request credentials
**Effort:** 2-4 hours

**File:** `sigilforge-daemon/src/api/server.rs`

**Tasks:**
- Use `UCred` from tokio/nix to get peer credentials
- Verify UID matches daemon owner
- Reject unauthorized connections

---

## P1: High Priority (This Sprint)

### 6. Replace Sync Locks with Async

**Issue:** `std::sync::RwLock` in async context
**Impact:** Thread starvation under load
**Effort:** 2-3 hours

**Files:**
- `sigilforge-core/src/account_store.rs:90-95`
- `sigilforge-core/src/store/memory.rs:54-87`

**Fix:** Replace with `tokio::sync::RwLock` or `parking_lot::RwLock`

### 7. Add Memory Zeroing for Secrets

**Issue:** `Secret` type doesn't zero memory on drop
**Impact:** Secrets remain in memory
**Effort:** 1-2 hours

**File:** `sigilforge-core/src/store/mod.rs:40-76`

**Fix:**
```rust
use zeroize::{Zeroize, Zeroizing};
pub struct Secret(Zeroizing<String>);
```

### 8. Add Pre-commit Hooks

**Issue:** No local enforcement of formatting/linting
**Impact:** Broken code reaches CI
**Effort:** 1 hour

**Create:** `.pre-commit-config.yaml`

```yaml
repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt -- --check
        language: system
        types: [rust]
        pass_filenames: false
      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false
```

### 9. Create CONTRIBUTING.md

**Issue:** No developer onboarding guide
**Impact:** Friction for new contributors
**Effort:** 2-3 hours

**Content:**
- Development environment setup
- Using justfile commands
- Testing requirements
- Git workflow
- Code review process

### 10. Update ROADMAP.md

**Issue:** Shows all phases as incomplete
**Impact:** Confusing project status
**Effort:** 1-2 hours

**Tasks:**
- Mark Phases 0-2 as complete
- Update Phase 3 status (~80%)
- Add Phase 4+ plans
- Sync with CHANGELOG unreleased

### 11. Add Connection Limits

**Issue:** Unbounded concurrent connections
**Impact:** DoS vulnerability
**Effort:** 1-2 hours

**File:** `sigilforge-daemon/src/api/server.rs:66-72`

**Fix:**
```rust
let semaphore = Arc::new(Semaphore::new(100)); // Max 100 connections
let permit = semaphore.acquire().await?;
tokio::spawn(async move {
    let _permit = permit; // Held for duration
    handle_connection(stream, api).await
});
```

### 12. Add RPC Line Length Limits

**Issue:** No limit on request line length
**Impact:** Memory exhaustion vulnerability
**Effort:** 30 minutes

**File:** `sigilforge-daemon/src/api/server.rs:101-138`

**Fix:**
```rust
const MAX_LINE_LENGTH: usize = 1_000_000; // 1MB
let mut line = String::new();
let n = reader.take(MAX_LINE_LENGTH as u64).read_line(&mut line).await?;
```

---

## P2: Medium Priority (Next Sprint)

### 13. Integrate OAuth Flows into Daemon

**Issue:** OAuth code exists but not accessible via RPC
**Impact:** Can't obtain new tokens
**Effort:** 4-8 hours

**Tasks:**
- Add OAuth flow trigger to `add_account` RPC
- Implement interactive CLI prompts
- Handle browser launch for PKCE flow
- Implement device code polling UI

### 14. Fix Test State Pollution

**Issue:** RPC tests share state, causing failures
**Impact:** Flaky tests
**Effort:** 1-2 hours

**File:** `sigilforge-daemon/tests/rpc_test.rs`

**Fix:** Use unique temp directories per test, add cleanup

### 15. Add TROUBLESHOOTING.md

**Issue:** No troubleshooting guide
**Impact:** Users stuck on common issues
**Effort:** 2-3 hours

**Sections:**
- Daemon won't start
- Socket connection fails
- OAuth flow issues
- Keyring access denied
- Token refresh problems

### 16. Add Flexible Logging

**Issue:** Hardcoded log levels, no RUST_LOG support
**Impact:** Hard to debug issues
**Effort:** 1-2 hours

**Files:**
- `sigilforge-daemon/src/main.rs:34-37`
- `sigilforge-cli/src/main.rs`

**Fix:**
```rust
use tracing_subscriber::EnvFilter;
FmtSubscriber::builder()
    .with_env_filter(EnvFilter::from_default_env())
    .init();
```

### 17. Add IDE Configuration

**Issue:** No editor configuration files
**Impact:** Inconsistent formatting
**Effort:** 30 minutes

**Create:** `.editorconfig`, `.vscode/settings.json`

### 18. Add Missing OAuth Providers

**Issue:** Microsoft, Reddit documented but not configured
**Impact:** Documentation mismatch
**Effort:** 1-2 hours

**File:** `sigilforge-core/src/provider.rs`

### 19. Deduplicate Error Types

**Issue:** Same errors in core and client crates
**Impact:** Maintenance burden
**Effort:** 2-3 hours

**Files:**
- `sigilforge-core/src/error.rs`
- `sigilforge-client/src/types.rs:84-133`

### 20. Add Input Validation

**Issue:** ServiceId/AccountId accept any string
**Impact:** Invalid data stored
**Effort:** 1-2 hours

**File:** `sigilforge-core/src/model.rs:33-35`

**Fix:** Add regex validation or character whitelist

---

## P3: Low Priority (Backlog)

### 21. Add CLI Tests

**Issue:** No tests for CLI commands
**Impact:** Regressions undetected
**Effort:** 4-8 hours

### 22. Add examples/ Directory

**Issue:** No runnable examples
**Impact:** Hard to understand usage
**Effort:** 4-6 hours

### 23. Implement EncryptedFileStore

**Issue:** Documented but not implemented
**Impact:** Documentation mismatch
**Effort:** 8-16 hours (or remove from docs)

### 24. Add Constant-Time Secret Comparison

**Issue:** Standard equality is timing-sensitive
**Impact:** Theoretical timing attack
**Effort:** 1 hour

**File:** `sigilforge-core/src/store/mod.rs:78-84`

### 25. Add Structured Logging

**Issue:** Text-only logs
**Impact:** Hard to parse programmatically
**Effort:** 2-3 hours

### 26. Add Audit Logging

**Issue:** No logging of who accessed what
**Impact:** No accountability
**Effort:** 4-8 hours

### 27. Add rust-toolchain.toml

**Issue:** No pinned Rust version
**Impact:** Build inconsistencies
**Effort:** 15 minutes

```toml
[toolchain]
channel = "1.83"
components = ["rustfmt", "clippy"]
```

### 28. Add Generic Error Messages

**Issue:** Errors reveal account existence
**Impact:** Account enumeration
**Effort:** 1-2 hours

### 29. Fix URL Decoding in OAuth

**Issue:** Manual parsing without decoding
**Impact:** Encoded payloads mishandled
**Effort:** 30 minutes

**File:** `sigilforge-core/src/oauth/pkce.rs:284-294`

### 30. Add Rate Limiting

**Issue:** No rate limiting on RPC
**Impact:** Brute force possible
**Effort:** 2-4 hours

---

## Summary by Effort

| Effort | Count | Items |
|--------|-------|-------|
| < 1 hour | 8 | 1, 4, 8, 12, 17, 27, 29 |
| 1-2 hours | 9 | 7, 10, 11, 14, 16, 18, 20, 24, 28 |
| 2-4 hours | 7 | 2, 3, 5, 6, 9, 19, 30 |
| 4-8 hours | 5 | 13, 15, 21, 22, 26 |
| 8+ hours | 1 | 23 |

## Recommended Sprint Plan

### Sprint 1 (Current)
- P0 items 1-5 (Critical fixes)
- P1 items 6-8 (Security + DX basics)

### Sprint 2
- P1 items 9-12 (Documentation + hardening)
- P2 items 13-14 (OAuth integration)

### Sprint 3
- P2 items 15-20 (Polish)
- P3 items as capacity allows
