# Sigilforge Troubleshooting Guide

This document provides solutions to common issues you may encounter when using Sigilforge.

## Table of Contents

1. [Headless / no D-Bus session bus](#headless--no-d-bus-session-bus)
2. [Daemon Issues](#daemon-issues)
3. [Authentication Issues](#authentication-issues)
4. [CLI Issues](#cli-issues)
5. [Storage Issues](#storage-issues)
6. [Platform-Specific Issues](#platform-specific-issues)

---

## Headless / no D-Bus session bus

This is the first thing to check on a server reached over SSH.

**Symptoms**: any write fails with a D-Bus error, and reads report credentials
as missing.

```
$ sigilforge github-app register hdless --app-id 1 --installation-id 2 --key-file t.pem
Caused by:
  storage error: backend error: failed to set keyring password: Platform secure storage failure:
  DBus error: dbus-launch: No existing session bus was found, and X11 autolaunch support was
  disabled at compile time.
```

**Cause**: `KeyringStore` talks to the Secret Service over the D-Bus **session**
bus, which a plain SSH login does not have. `dbus-launch` cannot start one
without X11 either. No amount of `libsecret` will fix this: there is no session
to attach to.

**Fix**: use the encrypted file store, which needs no session bus, no desktop,
and no prompt on read.

```bash
sigilforge store init      # once per host
sigilforge store status    # confirm: "Selected: encrypted-file"
```

Everything afterwards - `github-app register`, `add-account`, `sigilforged` -
picks it up automatically. See [ARCHITECTURE.md](ARCHITECTURE.md#encryptedfilestore)
for what it writes where.

**Back up the identity file it prints.** It is the only key to the store; there
is no escrow and no recovery.

### "No GitHub Apps registered" when an App *is* registered

**Symptoms**: `github-app register` fails or is skipped, and a later command
reports nothing registered:

```
$ sigilforge github-app list
No GitHub Apps registered          # <-- storage was unavailable, not empty
```

**Cause**: a fixed bug. Reads used to report "not registered" when the real
answer was "the store could not be opened", because `KeyringStore::try_new` only
constructed an `Entry` and never proved the backend worked.

**Fix**: upgrade. Since v0.4.0 `open_store` probes the backend with a real
round-trip and read paths distinguish the two cases:

```
$ sigilforge github-app list
Error: no usable secret storage backend
  - encrypted-file: storage backend 'encrypted-file' is unavailable: no age identity at
    /home/you/.config/sigilforge/age-identity.key; run `sigilforge store init` to create one
  - keyring: storage backend 'keyring' is unavailable: writing a probe entry failed:
    Platform secure storage failure: DBus error: ... No existing session bus was found ...
```

If you see "No GitHub Apps registered" from a current build, the store really is
empty.

### Forcing a backend

```bash
export SIGILFORGE_STORE_BACKEND=encrypted-file   # or keyring, memory, auto
```

or, persistently, in `~/.config/sigilforge/config.toml`:

```toml
[storage]
backend = "encrypted-file"
```

An explicitly requested backend is never silently replaced with another one; if
it does not work, the command fails and says why.

---

## Daemon Issues

### Daemon Won't Start

**Symptoms**: Running `sigilforge-daemon` fails immediately or hangs.

**Possible Causes and Solutions**:

1. **Socket path doesn't exist or lacks permissions**
   ```bash
   # Check socket directory exists
   ls -la ~/.config/sigilforge/

   # Create directory if missing
   mkdir -p ~/.config/sigilforge/
   chmod 700 ~/.config/sigilforge/
   ```

2. **Check daemon logs for errors**
   ```bash
   # Run daemon with verbose logging
   RUST_LOG=debug cargo run -p sigilforge-daemon
   ```

3. **Verify keyring access**
   - On Linux, ensure D-Bus is running: `systemctl --user status dbus`
   - On macOS, check Keychain Access permissions
   - On Windows, ensure Credential Manager service is running

### "Address Already in Use" Error

**Symptoms**: Daemon fails to start with error about socket/pipe already in use.

**Solution**: Kill the existing daemon process and remove the stale socket.

```bash
# Find and kill existing daemon process
ps aux | grep sigilforge-daemon
kill <PID>

# Remove stale socket file (Linux/macOS)
rm ~/.config/sigilforge/daemon.sock

# On Windows, remove named pipe reference (automatic on restart)
```

**Prevention**: Use proper daemon shutdown:
```bash
# Send graceful shutdown signal
kill -TERM <PID>
```

### Can't Connect to Daemon

**Symptoms**: CLI commands timeout or fail with "connection refused" errors.

**Possible Causes and Solutions**:

1. **Daemon not running**
   ```bash
   # Check if daemon is running
   ps aux | grep sigilforge-daemon

   # Start daemon if not running
   cargo run -p sigilforge-daemon &
   ```

2. **Socket path mismatch**
   ```bash
   # Verify daemon socket path (Linux/macOS)
   ls -la ~/.config/sigilforge/daemon.sock

   # Check CLI is using correct path
   sigilforge-cli --verbose status
   ```

3. **Permissions issue**
   ```bash
   # Ensure socket is accessible
   chmod 600 ~/.config/sigilforge/daemon.sock
   ```

---

## Authentication Issues

### OAuth Flow Fails

**Symptoms**: Browser redirect doesn't complete, or authorization code is rejected.

**Possible Causes and Solutions**:

1. **Provider configuration incorrect**
   ```bash
   # Verify service configuration exists
   cat ~/.config/sigilforge/services/<service>.yaml

   # Check client_id and client_secret are set correctly
   # Ensure redirect_uri matches provider settings exactly
   ```

2. **Redirect URI mismatch**
   - OAuth provider: `http://localhost:8080/callback`
   - Sigilforge config: Must match exactly (including port, path, protocol)
   - Update provider settings or Sigilforge config to align

3. **Network/firewall blocking callback**
   ```bash
   # Test if callback port is accessible
   curl http://localhost:8080/

   # Check firewall rules
   sudo ufw status
   ```

### Token Refresh Fails

**Symptoms**: `get-token` returns expired token error or fails to refresh.

**Possible Causes and Solutions**:

1. **Refresh token expired**
   - Some providers (Google, Microsoft) expire refresh tokens after 6 months of inactivity
   - **Solution**: Re-authenticate the account
   ```bash
   sigilforge remove-account <service> <account>
   sigilforge add-account <service> <account>
   ```

2. **Provider revoked access**
   - User may have revoked app permissions in provider settings
   - **Solution**: Check provider dashboard and re-authenticate

3. **Network connectivity issue**
   ```bash
   # Test connectivity to provider
   curl -I https://oauth2.googleapis.com/token
   ```

### Keyring Access Denied

**Symptoms**: "Permission denied" or "Failed to access keyring" errors.

**Possible Causes and Solutions**:

1. **D-Bus not running (Linux)**
   ```bash
   # Check D-Bus status
   systemctl --user status dbus

   # Start D-Bus if not running
   systemctl --user start dbus

   # Enable D-Bus on boot
   systemctl --user enable dbus
   ```

2. **libsecret not installed (Linux)**
   ```bash
   # Ubuntu/Debian
   sudo apt-get install libsecret-1-0 libsecret-1-dev

   # Fedora/RHEL
   sudo dnf install libsecret libsecret-devel

   # Arch
   sudo pacman -S libsecret
   ```

3. **Keyring locked (macOS)**
   - Unlock Keychain Access manually
   - System Preferences > Security & Privacy > Privacy > Full Disk Access
   - Add Terminal or your terminal emulator

4. **No fallback to memory storage (since v0.4.0)**
   - Sigilforge no longer degrades to in-memory storage when the keyring is
     unreachable. It reports the failure and stops.
   - That fallback is what turned "storage is unavailable" into "you have no
     credentials". `MemoryStore` is now used only when asked for by name
     (`SIGILFORGE_STORE_BACKEND=memory`).
   - On a host without a keyring, run `sigilforge store init` and use the
     encrypted file store.

5. **Secrets vanish between commands**
   - Write succeeds, the next command reports the credential is missing
   - Check that `libdbus-1-dev` and `pkg-config` were present when Sigilforge was
     built. Without a platform backend compiled in, the `keyring` crate uses an
     in-process mock that loses everything at exit:
     ```bash
     sudo apt-get install -y libdbus-1-dev pkg-config
     cargo clean -p keyring && cargo build --workspace
     ```
   - Confirm a Secret Service provider is actually running:
     ```bash
     pgrep -a gnome-keyring-daemon || pgrep -a kwalletd
     secret-tool store --label=probe service sigilforge-probe key x  # then Ctrl-D
     secret-tool lookup service sigilforge-probe key x
     ```

---

## CLI Issues

### Commands Timeout

**Symptoms**: CLI commands hang for 30+ seconds then timeout.

**Possible Causes and Solutions**:

1. **Daemon not running**
   ```bash
   # Check daemon status
   sigilforge status

   # Start daemon if needed
   cargo run -p sigilforge-daemon &
   ```

2. **Use verbose mode for diagnostics**
   ```bash
   sigilforge --verbose get-token <service> <account>
   ```

3. **Try direct mode (bypasses daemon)**
   ```bash
   sigilforge --direct list-accounts
   ```

### get-token Returns Stub Value

**Symptoms**: `get-token` returns placeholder like `"stub-token-<service>-<account>"`.

**Known Issue**: Daemon RPC stubs not yet fully wired to actual token manager.

**Workaround**: Use direct mode:
```bash
sigilforge --direct get-token <service> <account>
```

**Status**: See issue #[number] for implementation progress.

### remove-account Doesn't Work

**Symptoms**: Account still appears in `list-accounts` after removal.

**Known Issue**: Account removal not fully implemented.

**Workaround**: Manually remove from storage:
```bash
# Remove from keyring (requires manual keyring access)
# Or delete account from config
rm ~/.config/sigilforge/accounts/<service>-<account>.yaml
```

**Status**: See issue #16 for implementation progress.

---

## Storage Issues

### Accounts Not Persisting

**Symptoms**: Accounts added via `add-account` disappear after daemon restart.

**Possible Causes and Solutions**:

1. **Configuration directory permissions**
   ```bash
   # Check directory exists and is writable
   ls -la ~/.config/sigilforge/

   # Fix permissions if needed
   chmod 700 ~/.config/sigilforge/
   chmod 600 ~/.config/sigilforge/*.yaml
   ```

2. **Secret storage unavailable**
   - Commands now fail rather than writing to memory; check which backend is in
     play:
   ```bash
   sigilforge store status
   RUST_LOG=debug sigilforged
   ```

3. **Disk full**
   ```bash
   # Check available space
   df -h ~/.config/
   ```

### Keyring Unavailable

**Symptoms**: Warning messages about falling back to memory storage.

**Impact**: Secrets (refresh tokens, API keys) not persisted across restarts.

**Solutions by Platform**:

**Linux**:
```bash
# Install secret service
sudo apt-get install gnome-keyring libsecret-1-0

# Start secret service
eval $(echo | gnome-keyring-daemon --unlock)
```

**macOS**:
- Keychain is always available
- If failing, check System Preferences > Security & Privacy

**Windows**:
- Credential Manager should be available by default
- Check Services: `services.msc` > Credential Manager

---

## Platform-Specific Issues

### Linux

#### libsecret / D-Bus Issues

**Problem**: "Failed to connect to D-Bus" or "libsecret unavailable"

**Solution**:
```bash
# Ensure D-Bus session is running
echo $DBUS_SESSION_BUS_ADDRESS

# If empty, start D-Bus
eval $(dbus-launch --sh-syntax)

# Install libsecret
sudo apt-get install libsecret-1-0 libsecret-1-dev gnome-keyring
```

**Headless Systems**: none of the above helps - there is no session bus to
connect to. Use encrypted file storage instead:

```bash
sigilforge store init
```

which is equivalent to setting, in `~/.config/sigilforge/config.toml`:

```toml
[storage]
backend = "encrypted-file"
identity_file = "~/.config/sigilforge/age-identity.key"
secrets_file  = "~/.local/share/sigilforge/secrets.age"
```

See [Headless / no D-Bus session bus](#headless--no-d-bus-session-bus).

### Encrypted file store problems

| Message | Meaning |
|---------|---------|
| `no age identity at ...; run \`sigilforge store init\`` | The store has not been set up on this host. |
| `... is readable by other users (mode 0644); run: chmod 600 ...` | The identity file's permissions were loosened. Sigilforge refuses to use a private key other accounts can read. Fix the mode, and assume the key is compromised if the host is shared. |
| `could not decrypt ... with the identity at ...` | The identity and the store do not match - usually a store restored from another host, or an identity that was regenerated. Restore the matching identity file. |
| `... is zero bytes, which means it was truncated` | A failed copy or a full disk. Restore from backup. Writes are atomic, so this is never a normal state. |
| `... was written by a newer Sigilforge` | Downgrade, or upgrade this host. |
| `timed out waiting for the lock at ...` | Another Sigilforge process is writing, or one died holding the lock. Locks older than 60s are broken automatically; otherwise delete `secrets.age.lock`. |

**Recovering without Sigilforge.** The store is a standard age file, so the
`age` or `rage` CLI can read it:

```bash
age -d -i ~/.config/sigilforge/age-identity.key ~/.local/share/sigilforge/secrets.age
```

**If the identity file is lost**, the store cannot be decrypted by anyone,
including you. Delete both files, run `sigilforge store init`, and re-register
every credential.

#### SELinux Denials

If using SELinux, check for denials:
```bash
sudo ausearch -m avc -ts recent | grep sigilforge
```

### macOS

#### Keychain Access Prompts

**Problem**: Repeated password prompts for keychain access.

**Solution**:
1. Open Keychain Access app
2. Right-click on "login" keychain > Change Settings for Keychain "login"
3. Uncheck "Lock after X minutes of inactivity"
4. Add `sigilforge-daemon` to "Always allow" list for Sigilforge entries

#### Gatekeeper Issues

If running unsigned binaries:
```bash
# Allow running unsigned binary
xattr -dr com.apple.quarantine ./target/release/sigilforge-daemon
```

### Windows

#### Named Pipe Path Issues

**Problem**: Daemon fails to create or connect to named pipe.

**Default path**: `\\.\pipe\sigilforge`

**Solutions**:
1. Ensure no other process is using the pipe name
2. Run with administrator privileges if permission denied
3. Check Windows Firewall isn't blocking local IPC

#### Credential Manager Access

If CLI can't access Windows Credential Manager:
```powershell
# Check Credential Manager service is running
Get-Service -Name "VaultSvc" | Select-Object Status, StartType

# Start service if stopped
Start-Service -Name "VaultSvc"
```

---

## Still Having Issues?

If you've tried the solutions above and still experiencing problems:

1. **Enable debug logging**:
   ```bash
   RUST_LOG=debug sigilforge-daemon
   RUST_LOG=debug sigilforge-cli --verbose <command>
   ```

2. **Check existing issues**: [GitHub Issues](https://github.com/raibid-labs/sigilforge/issues)

3. **File a new issue** with:
   - Platform and version (OS, Rust version)
   - Full error message or log output
   - Steps to reproduce
   - Output of `sigilforge status`

4. **Look for related documentation**:
   - [Architecture](ARCHITECTURE.md) - System design details
   - [Interfaces](INTERFACES.md) - API contracts
   - [README](../README.md) - Getting started guide
