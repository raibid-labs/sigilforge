# Security Audit

## Overall Assessment: 4/10

As a credential management system, security is critical. While the project has good foundational security patterns (Secret type with redaction, PKCE implementation), there are critical gaps in authentication, authorization, socket security, and memory safety that must be addressed before production use.

## Critical Security Issues

### Issue 1: No Authentication (CRITICAL)

**Location:** `sigilforge-daemon/src/api/server.rs:29-92`

**Current State:** NO authentication mechanism between client and daemon

```rust
// Any process that can access the socket can request tokens
pub async fn run(self, socket_path: &Path, shutdown: Receiver<()>) -> Result<()> {
    let listener = UnixListener::bind(socket_path)?;
    // No client verification, no credentials validation
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            handle_connection(stream, api).await  // No auth check
        });
    }
}
```

**Impact:** Any process can request ANY account's tokens
**Risk Level:** CRITICAL

### Issue 2: No Authorization (CRITICAL)

**Location:** `sigilforge-daemon/src/api/handlers.rs:147-289`

**Current State:** All methods callable by any connected client

- `get_token()` - Returns tokens for ANY account
- `list_accounts()` - Returns all accounts
- `add_account()` - ANY client can add accounts
- `resolve()` - ANY client can resolve credentials

**Impact:** No access control on credential operations
**Risk Level:** CRITICAL

### Issue 3: No Socket Permission Management (CRITICAL)

**Location:** `sigilforge-daemon/src/api/server.rs:46-47`

```rust
let listener = UnixListener::bind(socket_path)?;
// No explicit permission setting - uses default umask (typically 022)
// Socket likely created as 0644 (world-readable)
```

**Issues:**
1. Socket may be accessible by other users
2. No SO_PEERCRED verification
3. No peer credential checking

**Impact:** Other users on system could access credentials
**Risk Level:** CRITICAL

### Issue 4: Socket Race Condition (HIGH)

**Location:** `sigilforge-daemon/src/api/server.rs:30-35`

```rust
if socket_path.exists() {
    std::fs::remove_file(socket_path)?;
}
// TOCTOU: Another process could create socket between delete and bind
let listener = UnixListener::bind(socket_path)?;
```

**Impact:** Potential for malicious socket substitution
**Risk Level:** HIGH

## High Priority Security Issues

### Issue 5: No Memory Zeroing (HIGH)

**Location:** `sigilforge-core/src/store/mod.rs:40-76`

```rust
pub struct Secret(String);

impl Drop for Secret {
    // No Drop implementation - memory not zeroed
}
```

The `Secret` type does NOT zero memory after use. Secrets remain in memory until deallocated and overwritten.

**Fix:** Use `zeroize` crate:
```rust
use zeroize::{Zeroize, Zeroizing};
pub struct Secret(Zeroizing<String>);
```

### Issue 6: URL Decoding Missing in OAuth (HIGH)

**Location:** `sigilforge-core/src/oauth/pkce.rs:284-294`

```rust
// Manual parameter parsing without URL decoding
if parts[0] == "code" {
    code = Some(parts[1].to_string());  // Raw value, not decoded
}
```

URL-encoded payloads bypass validation. `code=hello%00world` stored as literal `%00`.

### Issue 7: Account Enumeration (MEDIUM)

**Location:** `sigilforge-daemon/src/api/handlers.rs:162-167`

```rust
return Err(Error::invalid_params(format!(
    "Account {}/{} not found", service, account
)));
```

Error reveals whether account exists - allows enumeration attacks.

**Fix:** Return generic "not found" without specifics.

## Medium Priority Security Issues

### Issue 8: Secret Comparison Timing Attack (MEDIUM)

**Location:** `sigilforge-core/src/store/mod.rs:78-84`

```rust
impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0  // Standard equality - timing sensitive
    }
}
```

**Fix:** Use `subtle::ConstantTimeEq`

### Issue 9: No Connection Limits (MEDIUM)

**Location:** `sigilforge-daemon/src/api/server.rs:66-72`

```rust
tokio::spawn(async move {
    handle_connection(stream, api).await
});
// No limit on concurrent connections - DoS vulnerability
```

**Fix:** Use semaphore or connection pool

### Issue 10: No Line Length Limits (MEDIUM)

**Location:** `sigilforge-daemon/src/api/server.rs:101-138`

```rust
let mut line = String::new();
reader.read_line(&mut line).await?;  // No size limit
```

Malicious client could send arbitrarily long lines causing memory exhaustion.

### Issue 11: PKCE Verifier Not Cleaned Up (LOW)

**Location:** `sigilforge-core/src/oauth/pkce.rs:60,152-156`

Verifier stored in `Arc<Mutex<Option<...>>>` indefinitely. No automatic cleanup or timeout - memory leak if flow abandoned.

## Security Strengths

### 1. Secret Type with Redaction
```rust
impl Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")  // Good - prevents accidental logging
    }
}
```

### 2. PKCE Implementation
- Uses SHA256 challenge method (S256)
- Generates random code verifiers
- Validates state parameter

### 3. Keyring Backend
- Uses OS-level secure storage
- libsecret/Keychain/Credential Manager integration

### 4. Minimal Unsafe Code
Only 1 unsafe block for necessary libc call:
```rust
let uid = unsafe { libc::getuid() };
```

## Logging Concerns

### Potential Information Leaks

**Location:** Multiple files

```rust
// Account names logged
info!("RPC: get_token({}/{})", service, account);

// OAuth URIs and configuration
debug!("Authorization URL: {}", auth_url);

// Request/response JSON could contain sensitive data
trace!("Request: {}", request_json);
```

### Recommendations
1. Filter sensitive data from logs
2. Use structured logging with sensitivity markers
3. Document log retention policy
4. Add audit logging for credential access

## Dependency Security

### Key Dependencies
| Crate | Version | Risk |
|-------|---------|------|
| tokio | 1.41 | Low - actively maintained |
| oauth2 | 4.4 | Low - standard OAuth library |
| keyring | 3 | Medium - platform-specific |
| reqwest | (workspace) | Low - widely used |

### Concerns
1. **Keyring fallback to MemoryStore** - Silent security downgrade
2. **No cargo-audit in docs** - Should document running `cargo audit`

## Security Recommendations

### Immediate (CRITICAL)
1. **Implement socket permissions** - Set to 0600 after binding
2. **Add peer credential verification** - Use SO_PEERCRED
3. **Implement authentication** - At minimum verify UID matches
4. **Add connection limits** - Semaphore-based limiting

### Short-term (HIGH)
5. **Add memory zeroing** - Use `zeroize` crate
6. **Fix URL decoding** - Use proper URL decoding in OAuth
7. **Add rate limiting** - Prevent brute force attacks
8. **Generic error messages** - Prevent enumeration

### Medium-term
9. **Implement authorization model** - Per-account access control
10. **Add audit logging** - Track who accessed what
11. **Constant-time comparison** - Use `subtle` crate
12. **Secure socket creation** - Atomic with proper permissions

## Security Checklist for Production

- [ ] Socket permissions set to 0600
- [ ] Peer credential verification enabled
- [ ] Authentication implemented
- [ ] Authorization checks on all methods
- [ ] Memory zeroing for secrets
- [ ] Connection limits configured
- [ ] Rate limiting enabled
- [ ] Audit logging enabled
- [ ] Generic error messages
- [ ] URL encoding/decoding fixed
- [ ] cargo-audit clean
- [ ] Dependency review completed

## Threat Model Summary

| Threat | Current State | Risk |
|--------|--------------|------|
| Local user access | VULNERABLE | Critical |
| Socket hijacking | VULNERABLE | High |
| Memory scraping | VULNERABLE | High |
| DoS via connections | VULNERABLE | Medium |
| DoS via large requests | VULNERABLE | Medium |
| Account enumeration | VULNERABLE | Medium |
| Timing attacks | VULNERABLE | Low |
| OAuth flow attacks | Partially Protected | Medium |
