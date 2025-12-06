# Sigilforge Roadmap

This roadmap outlines phases for turning Sigilforge into a reliable,
reusable auth and credential manager for raibid-labs projects and beyond.

## Phase 0 — Core Types and Stubs

- Create Rust workspace with:
  - `sigilforge-core`
  - `sigilforge-daemon`
  - `sigilforge-cli`
- In `sigilforge-core`:
  - Define types for `ServiceId`, `AccountId`, `Account`.
  - Define `CredentialRef`, `SecretValue`, and `Token` structs.
  - Define trait stubs:
    - `SecretStore`
    - `TokenManager`
    - `ReferenceResolver`
- In `sigilforge-daemon`:
  - Implement a basic async main that:
    - Loads a static config or placeholder.
    - Listens on a local port/socket (even if it only logs requests at first).
- In `sigilforge-cli`:
  - Implement a placeholder CLI with subcommands:
    - `accounts list`
    - `accounts add`
    - `token get`
  - Wire them to simple in-memory logic for now.

## Phase 1 — Minimal Secret Storage and CLI

- Implement an in-memory `SecretStore` for testing.
- Implement a simple file-backed `SecretStore`:
  - Plaintext or trivially obfuscated for now (to be replaced later with secure storage).
- Extend CLI:
  - `secret set <ref>`
  - `secret get <ref>`
  - `accounts add <service> <alias>`
  - `accounts list`
- Add basic config file format:
  - A single config file describing services, account aliases, and associated references.

Deliverable: Sigilforge can store, retrieve, and list secrets and accounts locally.

## Phase 2 — Integrate OS Keyring and Basic Tokens

- Integrate with OS keyring:
  - Implement a `KeyringSecretStore` using a cross-platform keyring crate.
  - Add configuration to choose or combine `SecretStore` implementations.
- Extend `Token` model:
  - Add expiry, scopes, raw response fields.
- Implement a simple `TokenManager` that:
  - Reads tokens from `SecretStore`.
  - Exposes `get_access_token` without refresh (for static tokens or manually set tokens).
- CLI:
  - `token set <service> <account>`
  - `token get <service> <account>`

Deliverable: Sigilforge can manage static tokens and secrets through keyring and/or file.

## Phase 3 — OAuth Flows and Refresh

- Integrate with an OAuth2 client library (e.g. oauth2-rs).
- Implement device code flow and/or authorization code + PKCE for a first provider:
  - Good candidates: GitHub, Spotify.
- Extend `TokenManager`:
  - `ensure_access_token(service, account)`:
    - Uses refresh token if available.
    - If no refresh token, triggers or requests an interactive flow.
- CLI:
  - `accounts authorize <service> <account>`
    - Launches browser or prints device-code instructions.
  - `token ensure <service> <account>`

Deliverable: Sigilforge can obtain and refresh tokens for at least one real service.

## Phase 4 — Daemon API and Scryforge Integration

- Design a minimal daemon API, e.g. JSON-RPC:
  - `list_accounts`
  - `get_secret(ref)`
  - `get_access_token(service, account)`
  - `ensure_access_token(service, account)`
- Implement server in `sigilforge-daemon`.
- Implement a small client library in `sigilforge-core` (or separate crate) for
  other applications to call the daemon.
- Integrate with Scryforge:
  - Replace any ad-hoc auth code in Scryforge with calls to Sigilforge.
  - Use references like `auth://spotify/personal/token`.

Deliverable: Scryforge obtains tokens from Sigilforge instead of handling auth
directly.

## Phase 5 — Encrypted Config Files and vals-style References

- Add support for encrypted config files:
  - Integrate ROPS (Rust-native SOPS alternative) or SOPS via CLI.
  - Allow secrets and account info to be stored in encrypted YAML/JSON.
- Add `ReferenceResolver` implementation for:
  - `auth://service/account/...` references (internal).
  - `secret://namespace/...` references (mapped to SecretStore).
  - `vals:...` references:
    - Initially by shelling out to `vals`.
    - Optionally by implementing a subset of vals-like functionality internally.
- Add CLI commands:
  - `resolve <ref>`
  - `check <ref>` for validation.

Deliverable: Sigilforge becomes capable of pulling secrets and tokens from a mix
of keyring, encrypted files, and external secret managers via vals-style refs.

## Phase 6 — Hardening and Optional Extras

- Improve logging and error reporting.
- Add simple auditing (e.g. last-access timestamps per account).
- Add configuration options:
  - Auto-refresh tokens in background vs. on-demand.
  - Token caching policies (in-memory only vs. persisted).
- Consider a small TUI management interface for accounts and secrets
  (potentially reusing components from `fusabi-tui-core`).
- Expand supported providers and flows as needed:
  - Google (Gmail/Calendar)
  - Microsoft Graph (Outlook, To Do, Calendar)
  - Reddit, YouTube, etc.

At this point, Sigilforge should be a stable platform component that can be
relied on by multiple tools and agents.
