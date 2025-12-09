# Sigilforge Troubleshooting Guide

This document provides solutions to common issues you may encounter when using Sigilforge.

## Table of Contents

1. [Daemon Issues](#daemon-issues)
2. [Authentication Issues](#authentication-issues)
3. [CLI Issues](#cli-issues)
4. [Storage Issues](#storage-issues)
5. [Platform-Specific Issues](#platform-specific-issues)

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

4. **Fallback to memory storage**
   - Sigilforge falls back to in-memory storage if keyring unavailable
   - **Warning**: Secrets not persisted across daemon restarts

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

2. **Keyring fallback to memory**
   - If keyring unavailable, secrets stored in memory only
   - Check logs for keyring initialization errors
   ```bash
   RUST_LOG=debug sigilforge-daemon
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

**Headless Systems**: Use encrypted file storage instead:
```yaml
# ~/.config/sigilforge/config.yaml
storage:
  backend: "encrypted"
  encrypted:
    path: "~/.config/sigilforge/secrets.enc.yaml"
    format: "rops"
```

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
