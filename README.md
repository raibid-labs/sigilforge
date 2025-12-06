# Sigilforge

**Central authentication and credentials manager for the raibid-labs ecosystem.**

Sigilforge is a local daemon + library that provides unified secret storage, OAuth flow management, and credential resolution for applications in the raibid-labs family (Scarab, Hibana, Tolaria, Phage, Fusabi, Scryforge).

## What It Does

Sigilforge acts as a **small, local "vault + token service"** that:

- **Stores credentials securely**: API keys, OAuth refresh tokens, and other sensitive values are stored in the OS keyring at runtime, with optional encrypted file storage (SOPS/ROPS) for Git-friendly configuration.

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
│  │           │        │   Files   │        │ Providers │       │
│  └───────────┘        │(ROPS/SOPS)│        │           │       │
│                       └───────────┘        └───────────┘       │
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

## Problems It Solves

1. **Centralized Auth**: Applications don't re-implement OAuth flows; they ask Sigilforge for tokens.

2. **Secure Secret Storage**: Sensitive values live in the OS keyring or encrypted files, not plaintext configs.

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
└── docs/
    ├── STRUCTURE.md        # Documentation organization guide
    ├── ARCHITECTURE.md     # System design and components
    ├── ROADMAP.md          # Development phases
    ├── INTERFACES.md       # Trait definitions and API contracts
    ├── NEXT_STEPS.md       # Concrete next tasks for development
    ├── RELEASE.md          # Release process and versioning
    └── versions/           # Versioned documentation snapshots
        └── v0.1.0/        # Documentation for v0.1.0
```

## Getting Started

### Prerequisites

- Rust 1.85+ (2024 edition)
- A system with keyring support (Linux with `libsecret`, macOS Keychain, Windows Credential Manager)

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

For version-specific documentation, see [docs/versions/](docs/versions/).

## License

MIT

## Related Projects

- **Scryforge**: Multi-provider data synchronization built on Sigilforge for auth.
- **Phage**: Task management and automation using Fusabi components.
- **Fusabi**: TUI framework and common utilities for raibid-labs applications.
