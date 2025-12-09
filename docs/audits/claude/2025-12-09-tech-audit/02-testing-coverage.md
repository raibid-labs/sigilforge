# Testing Coverage Audit

## Overall Assessment: 6/10

The project has solid core tests with meaningful assertions and good helper functions. However, there are critical gaps in CLI testing, daemon integration tests, and a blocking compilation error in the client library tests.

## Test Statistics

| Metric | Value |
|--------|-------|
| Total test functions | 113 |
| Unit tests | 68 |
| Integration tests | 45 |
| Files with tests | 15/28 (53%) |
| Files without tests | 13 (47%) |

## Test Distribution by Crate

### sigilforge-core (Well Tested)
```
tests/account_lifecycle.rs    19 tests ✓
tests/token_refresh.rs        11 tests ✓
src/model.rs                   6 tests ✓
src/account_store.rs          10 tests ✓
src/store/memory.rs            5 tests ✓
src/store/keyring.rs           4 tests ✓
src/store/mod.rs               4 tests ✓
src/provider.rs                7 tests ✓
src/token.rs                   2 tests ✓
src/oauth/mod.rs               3 tests ✓
src/oauth/pkce.rs              2 tests ✓
src/oauth/device_code.rs       4 tests ✓
src/token_manager.rs           5 tests ✓
src/resolve.rs                 2 tests ✓
```

### sigilforge-daemon (Partial Coverage)
```
tests/shutdown_test.rs         3 tests ✓
tests/rpc_test.rs              4 tests (2 failing)
src/main.rs                    NO TESTS ✗
src/config.rs                  NO TESTS ✗
src/api/server.rs              NO TESTS ✗
src/api/handlers.rs            Indirect only
```

### sigilforge-client (Compilation Error)
```
src/resolve.rs                 6 tests
src/socket.rs                  4 tests
src/lib.rs                     3 tests
src/client.rs                  4 tests (BROKEN)
src/fallback.rs                5 tests (BROKEN)
src/types.rs                   NO TESTS ✗
```

### sigilforge-cli (No Tests)
```
src/main.rs                    NO TESTS ✗
src/client.rs                  NO TESTS ✗
```

## Critical Issues

### Issue 1: Test Compilation Failures (BLOCKING)

**Location:** Multiple files in sigilforge-client

```rust
// sigilforge-client/src/client.rs:342
std::env::set_var("GITHUB_TOKEN", "test_token");  // MISSING unsafe block

// sigilforge-client/src/fallback.rs:241
std::env::set_var("SIGILFORGE_GITHUB_PERSONAL_TOKEN", "env_token");  // MISSING unsafe
```

**Impact:** `cargo test` fails to compile for sigilforge-client
**Affected Lines:**
- `client.rs`: 342, 350, 355, 362
- `fallback.rs`: 241, 249, 254, 261, 266, 273, 287, 297
- `lib.rs`: 137, 144

**Fix:** Wrap in `unsafe { }` blocks or use test isolation crate

### Issue 2: RPC Test State Pollution

**Location:** `sigilforge-daemon/tests/rpc_test.rs`

```
test test_add_and_list_accounts: FAILED
  Expected: 0 accounts
  Got: 2 accounts
  Cause: Previous test left accounts in shared AccountStore
```

Tests share state through socket/temp directory without cleanup.

### Issue 3: No CLI Tests

The entire CLI crate has zero tests:
- No tests for argument parsing
- No tests for subcommand execution
- No tests for error output formatting
- No tests for daemon communication

## Test Quality Analysis

### Strengths

1. **Good Naming Convention**
   ```rust
   test_add_duplicate_account_fails()
   test_ensure_access_token_refreshes_expired_token()
   ```

2. **Meaningful Assertions** (not smoke tests)
   ```rust
   // Verifies token refreshed AND persisted
   assert_eq!(token.access_token, "new_access_token");
   ```

3. **Good Helper Functions**
   ```rust
   fn test_store() -> (TempDir, AccountStore)
   fn test_account() -> Account
   fn setup_manager() -> DefaultTokenManager
   ```

4. **Mock Strategy with wiremock**
   - Excellent HTTP mocking in `token_refresh.rs`
   - Routes POST /token requests
   - Tests both success (200) and error (400) cases

### Weaknesses

1. **Timing-Based Tests (Flaky Risk)**
   ```rust
   // sigilforge-daemon/tests/rpc_test.rs
   sleep(Duration::from_millis(100));  // Could fail on slow CI
   ```

2. **Socket File Collisions**
   ```rust
   // Tests use fixed paths like:
   "/tmp/sigilforge-test-add-list.sock"
   // No test-run isolation
   ```

3. **No Concurrent Access Testing**
   - No tests for RwLock contention
   - No stress tests for MemoryStore
   - No concurrent account update tests

## Missing Test Coverage

### Critical Gaps

| Module | Missing Tests |
|--------|---------------|
| CLI main.rs | All command testing |
| CLI client.rs | Daemon communication |
| Daemon main.rs | Startup, shutdown, signal handling |
| Daemon config.rs | Config loading, validation |
| Daemon handlers.rs | Direct handler unit tests |

### Edge Cases Not Tested

**Account Store:**
- Corrupted JSON in accounts.json
- Permission denied errors
- Disk full scenarios
- Very long account/service IDs
- Special characters in IDs

**Token Operations:**
- Timezone edge cases
- Very large token values
- Unicode in scopes
- Token type other than "Bearer"

**Daemon:**
- Multiple daemon instances
- Out of memory scenarios
- Malformed JSON-RPC requests
- Missing required fields

**OAuth:**
- Device code polling timeout
- User code expiration
- Invalid redirect URIs
- Non-standard token responses

## Integration Test Coverage

### What Exists (Good)
1. **Account Lifecycle** (19 tests) - Complete CRUD flow
2. **Token Refresh** (11 tests) - Full token lifecycle with mocks
3. **Daemon RPC** (4 tests) - E2E over Unix sockets
4. **Daemon Shutdown** (3 tests) - Graceful shutdown scenarios

### What's Missing
1. CLI to Daemon integration
2. Full OAuth flow integration
3. Client library full stack tests
4. Cross-service scenarios
5. Error recovery tests

## Recommendations

### Immediate (Fix Blockers)
1. Add `unsafe {}` blocks to env var test code
2. Fix socket path collisions with unique IDs
3. Add proper test isolation for RPC tests

### Short-term
1. Add CLI command tests (15-20 tests needed)
2. Add daemon main.rs/config.rs tests (10-15 tests)
3. Replace sleep-based sync with condition variables

### Medium-term
1. Add error path coverage (20-30 tests)
2. Add concurrent access tests
3. Add property-based tests (quickcheck/proptest)
4. Add performance/stress tests

## Test Commands

```bash
# Run all tests
just test

# Run with output
just test-verbose

# Run specific crate
just test-crate sigilforge-core

# Run feature-specific
just test-keyring
just test-oauth

# Run with coverage (CI)
cargo tarpaulin --out Html
```
