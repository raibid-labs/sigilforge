# Sigilforge

**Central authentication and credentials manager for the raibid-labs ecosystem.**

Sigilforge is a local daemon + library that provides unified secret storage, OAuth flow management, and credential resolution for applications in the raibid-labs family (Scarab, Hibana, Tolaria, Phage, Fusabi, Scryforge).

## What It Does

Sigilforge acts as a **small, local "vault + token service"** that:

- **Stores credentials securely**: API keys, OAuth refresh tokens, and other sensitive values go into the OS keyring **where there is one**, and into an age-encrypted file where there is not - a server over SSH has no D-Bus session bus, so the keyring is not an option there. Which backend applies:

  | Host | Backend | Setup |
  |------|---------|-------|
  | Desktop session (Linux with an unlocked keyring, macOS, Windows) | OS keyring | none |
  | Headless: SSH, container, CI runner | age-encrypted file, `0600` | `sigilforge store init`, once |
  | Tests | in-memory | `SIGILFORGE_STORE_BACKEND=memory` |

  The backend is probed before use, and an unreachable one is reported as unreachable - never as an empty set of credentials. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#backend-selection).

- **Runs OAuth flows**: Implements OAuth2 device-code and authorization-code+PKCE flows for common providers (Google, Microsoft, Spotify, Reddit, GitHub, etc.) so applications don't need to implement auth themselves.

- **Manages token lifecycles**: Automatically refreshes expired access tokens and persists updated credentials.

- **Resolves credential references**: Uses a URI scheme (`auth://service/account/token`) to provide tokens and secrets to consumers in a uniform way. Optionally supports `vals`-style references for external backends.

## How It Fits in the Ecosystem

```
┌─────────────────────────────────────────────────────────────────┐
│                     Consumer Applications                        │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐    │
│  │ Scryforge │  │   Phage   │  │  Fusabi   │  │ Future CLI│    │
│  │           │  │           │  │   Apps    │  │   Tools   │    │
│  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘    │
│        │              │              │              │           │
│        └──────────────┴──────────────┴──────────────┘           │
│                              │                                   │
│                    ┌─────────▼─────────┐                        │
│                    │    Sigilforge     │                        │
│                    │  (daemon + lib)   │                        │
│                    └─────────┬─────────┘                        │
│                              │                                   │
│        ┌─────────────────────┼─────────────────────┐            │
│        │                     │                     │            │
│  ┌─────▼─────┐        ┌─────▼─────┐        ┌─────▼─────┐       │
│  │ OS Keyring│        │ Encrypted │        │   OAuth   │       │
│  │ (desktop) │        │   File    │        │ Providers │       │
│  └───────────┘        │(age, head-│        │           │       │
│                       │  less)    │        └───────────┘       │
│                       └───────────┘                            │
└─────────────────────────────────────────────────────────────────┘
```

### Example Usage

**Scryforge** requesting a Gmail token:
```
auth://gmail/personal/token
```

**Phage** using the same Spotify account:
```
auth://spotify/main/token
```

**CLI** for manual management:
```bash
# Add a new account (starts OAuth flow)
sigilforge add-account spotify personal

# List all configured accounts
sigilforge list-accounts

# Get a fresh access token
sigilforge get-token spotify personal
```

**Argo CD / CI** using a GitHub App for private repositories:
```bash
# On a headless host, set up storage once (no D-Bus session bus there)
sigilforge store init

# Register once (App ID, installation ID, and PEM from the GitHub App settings page)
sigilforge github-app register raibid-labs \
    --app-id 1234567 --installation-id 89012345 --key-file app.private-key.pem

# A fresh, hour-long installation token whenever you need one
sigilforge github-app token raibid-labs

# Or hand the credential to Argo CD
sigilforge github-app argocd-secret raibid-labs \
    --repo-url https://github.com/raibid-labs/raibid-fish.git | kubectl apply -f -
```
See [docs/GITHUB_APP.md](docs/GITHUB_APP.md).

## Problems It Solves

1. **Centralized Auth**: Applications don't re-implement OAuth flows; they ask Sigilforge for tokens.

2. **Secure Secret Storage**: Sensitive values live in the OS keyring or an age-encrypted file, not plaintext configs - and a storage failure is reported as a storage failure, not as a missing credential.

3. **Token Lifecycle Management**: Access tokens are refreshed automatically; consumers always get valid tokens.

4. **Consistent Credential Model**: All apps use the same `service/account` model, making account sharing straightforward.

5. **Reference Resolution**: The `auth://` URI scheme and optional `vals`-style references provide a uniform way to access credentials from configs and code.

## Workspace Structure

```
sigilforge/
├── Cargo.toml              # Workspace root
├── sigilforge-core/        # Core types, traits, and logic
├── sigilforge-daemon/      # Background service with local API
├── sigilforge-cli/         # CLI tool for humans
├── sigilforge-client/      # Client library for Rust applications
└── docs/
    ├── STRUCTURE.md        # Documentation organization guide
    ├── ARCHITECTURE.md     # System design and components
    ├── ROADMAP.md          # Development phases
    ├── INTERFACES.md       # Trait definitions and API contracts
    ├── GITHUB_APP.md       # GitHub App setup and Argo CD integration
    ├── NEXT_STEPS.md       # Concrete next tasks for development
    ├── RELEASE.md          # Release process and versioning
    └── versions/           # Versioned documentation snapshots
        └── v0.1.0/        # Documentation for v0.1.0
```

## Getting Started

### Prerequisites

- Rust 1.85+ (2024 edition)
- For the keyring backend: a desktop session (Linux with `libsecret` and a
  running Secret Service, macOS Keychain, Windows Credential Manager)
- On a headless host none of that exists; run `sigilforge store init` once and
  Sigilforge uses an age-encrypted file instead. No extra packages needed - the
  encryption is a Rust library, not a `sops`/`age`/`gpg` binary.

### Building

```bash
cargo build --workspace
```

### Running the Daemon

```bash
cargo run -p sigilforge-daemon
```

### Using the CLI

```bash
cargo run -p sigilforge-cli -- --help
```

### First run on a headless host

```bash
sigilforge store init     # generates an age identity, 0600, and an empty store
sigilforge store status   # shows which backend is selected, and why
```

`store init` prints the path of the identity file. **Back it up.** It is the
only key to the store; there is no escrow and no recovery.

## Configuration

Sigilforge stores its configuration in platform-appropriate directories:

- **Linux**: `~/.config/sigilforge/`
- **macOS**: `~/Library/Application Support/sigilforge/`
- **Windows**: `%APPDATA%\sigilforge\`

See `docs/ARCHITECTURE.md` for details on the configuration format and storage backends.

## Integration

### As a Library

Applications can link `sigilforge-core` directly:

```rust
use sigilforge_core::{TokenManager, ServiceId, AccountId};

async fn get_spotify_token(manager: &impl TokenManager) -> Result<String, Error> {
    let service = ServiceId::new("spotify");
    let account = AccountId::new("personal");
    manager.ensure_access_token(&service, &account).await
}
```

### Via Client Library

For Rust applications that want to communicate with the daemon:

```rust
use sigilforge_client::SigilforgeClient;

async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let client = SigilforgeClient::connect().await?;
    let token = client.get_token("spotify", "personal").await?;
    println!("Got token: {}", token.token);
    Ok(())
}
```

### Via Daemon API

Applications can communicate with `sigilforge-daemon` over a Unix socket (Linux/macOS) or named pipe (Windows):

```json
{"method": "get_token", "params": {"service": "spotify", "account": "personal"}}
```

See `docs/INTERFACES.md` for the full API specification.

## Documentation

- **[STRUCTURE.md](docs/STRUCTURE.md)**: Documentation organization and versioning conventions
- **[ARCHITECTURE.md](docs/ARCHITECTURE.md)**: System design and component details
- **[INTERFACES.md](docs/INTERFACES.md)**: API contracts and trait definitions
- **[ROADMAP.md](docs/ROADMAP.md)**: Development phases and future plans
- **[NEXT_STEPS.md](docs/NEXT_STEPS.md)**: Current development tasks
- **[RELEASE.md](docs/RELEASE.md)**: Release process and versioning workflow
- **[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)**: Including the headless / no-D-Bus case

For version-specific documentation, see [docs/versions/](docs/versions/).

## License

MIT

## Related Projects

- **Scryforge**: Multi-provider data synchronization built on Sigilforge for auth.
- **Phage**: Task management and automation using Fusabi components.
- **Fusabi**: TUI framework and common utilities for raibid-labs applications.
