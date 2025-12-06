# Sigilforge Architecture

## Goals

Sigilforge should:

- Act as the **central place** where credentials live:
  - API keys
  - OAuth tokens (access + refresh)
  - Provider-specific secrets
- Provide safe abstraction over:
  - OS keyring
  - Encrypted config files (ROPS/SOPS-like)
  - Optional vals-like external secret resolution
- Offer both:
  - A library (`sigilforge-core`) usable in-process
  - A daemon (`sigilforge-daemon`) exposing a local IPC API
- Present a **reference-oriented interface**:
  - `auth://service/account/token`
  - `auth://service/account/api_key`
  - `vals:ref+...://...` substitutions where appropriate

## High-Level Components

Sigilforge consists of:

1. `sigilforge-core` — core types and traits
2. `sigilforge-daemon` — background service and API implementation
3. `sigilforge-cli` — command-line interface for humans and scripts

### 1. sigilforge-core

Responsibilities:

- Define domain types:
  - `ServiceId`, `AccountId`, `Account`
  - `CredentialRef`, `Token`, `SecretValue`
- Define core traits:
  - `SecretStore` — abstract storage for secrets
  - `TokenManager` (or `TokenProvider`) — token lifecycle and refresh logic
  - `ReferenceResolver` — resolving `auth://` and optionally `vals:` references
- Implement baseline in-memory or file-backed `SecretStore` for initial testing.
- Integrate with external crates (in future phases) for:
  - OS keyring
  - ROPS/SOPS for file encryption
  - OAuth2 flows (via oauth2-rs or similar libraries)

### 2. sigilforge-daemon

Responsibilities:

- Provide a **single, long-lived process** that:

  - Loads configuration (accounts, services, storage backends).
  - Owns the active `SecretStore` and `TokenManager` instances.
  - Exposes a local API over:
    - Unix domain socket, or
    - Local TCP port (optionally with TLS in future).

- API capabilities:

  - List accounts:
    - `ListAccounts() -> [Account]`
  - Manage accounts:
    - `AddAccount(service, alias, config...)`
    - `RemoveAccount(service, alias)`
  - Request tokens and secrets:
    - `GetSecret(ref: String) -> SecretValue`
    - `GetAccessToken(service, account_alias) -> AccessToken`
    - `EnsureAccessToken(service, account_alias) -> AccessToken`
  - Trigger auth flows:
    - `BeginAuthFlow(service, account_alias, scopes...)`
    - `CompleteAuthFlow(...)` (depending on flow type)

The daemon binds the logical model from `sigilforge-core` to an actual runtime
where tokens are kept up to date and available to local tools.

### 3. sigilforge-cli

Responsibilities:

- Provide friendly, scriptable CLI commands such as:

  - `sigilforge accounts list`
  - `sigilforge accounts add gmail personal`
  - `sigilforge accounts add spotify personal`
  - `sigilforge token get spotify personal`
  - `sigilforge token ensure spotify personal`
  - `sigilforge secret set some/custom/ref`
  - `sigilforge secret get some/custom/ref`

- Talk to the daemon if it is running.
- Optionally fall back to a pure library mode (direct use of `sigilforge-core`)
  for simple operations if no daemon is present.

## Credential Model

### Services and Accounts

- **Service**:
  - A logical external system (e.g. `gmail`, `spotify`, `reddit`, `youtube`,
    `msgraph`, `github`).
- **Account**:
  - A named identity under a service (e.g. `personal`, `work`, `lab`).
  - Stored as a combination of `ServiceId + AccountId`.

### Credentials and Secrets

Sigilforge should be able to store and manage:

- Static API keys:
  - e.g. `api_key` for a service
- OAuth client configuration:
  - `client_id`, `client_secret`
- OAuth tokens:
  - `refresh_token`
  - short-lived `access_token` (cached in memory and/or on disk depending on policy)
- Arbitrary named secrets:
  - `auth://service/account/custom`
  - Named references like `secret://project-x/db-password` (internally mapped)

All sensitive values are:

- Stored via `SecretStore` (OS keyring or encrypted file).
- Exposed only via the daemon API or explicit CLI commands.

## Secret Storage Abstraction

`SecretStore` trait (conceptual):

- `get(ref: &CredentialRef) -> Option<SecretValue>`
- `set(ref: &CredentialRef, value: SecretValue)`
- `delete(ref: &CredentialRef)`

Implementations:

1. **Keyring-based store**:
   - Uses platform-native keychain APIs.
   - Ideal for tokens and credentials on a single user machine.

2. **Encrypted-file store**:
   - Uses ROPS or SOPS to encrypt YAML/JSON files with secrets.
   - Ideal for Git-tracked configuration where encryption is required.

3. **Composite store**:
   - Combines keyring and file-based stores.
   - Reads and writes from appropriate backends depending on type of secret.

## Token Management

`TokenManager` responsibilities:

- Represent tokens and their metadata:
  - `access_token`, `refresh_token`
  - expiry timestamps
  - raw provider response data (if needed)
- Provide operations like:

  - `ensure_access_token(service, account) -> AccessToken`
    - If an unexpired access token exists → return it.
    - If expired but a refresh token exists → refresh via OAuth, store new tokens.
    - If no valid credentials exist → return an error or request interactive auth.

- Implement generic OAuth2 flows:

  - Device Code Flow:
    - For CLI-based auth with browser step.
  - Authorization Code + PKCE:
    - For more complex or web-based flows.
  - (Optionally) client credentials flow:
    - Where appropriate (e.g., service-to-service tokens).

The flow specifics (endpoints, scopes) are defined in per-service configuration.

## Reference Resolution

`ReferenceResolver` provides a generic resolution API for string references.

Targets:

- Sigilforge-specific references:
  - `auth://service/account/token`
  - `auth://service/account/api_key`
  - `auth://service/account/refresh`
- Generic secrets:
  - `secret://namespace/path`
- Optional vals-compatible references:
  - `vals:ref+vault://...`
  - `vals:ref+awsssm://...`
  - etc.

The resolution process:

1. Parse the reference string.
2. Route it:
   - `auth://` → use internal account/token model.
   - `secret://` → resolve via configured `SecretStore`.
   - `vals:` → shell out to `vals` (initially) or use an internal adapter later.
3. Return a `SecretValue` or raise an error.

## Integration with Scryforge and Other Apps

Scryforge and other tools should **never** store or manage tokens directly.

Instead, they should:

- Define which service/account they need (e.g. `spotify/personal`).
- Ask Sigilforge for a token or secret:

  - Via library:
    - `sigilforge_core::ensure_access_token(service, account)`
  - Via daemon API:
    - `GET /token?service=spotify&account=personal`
  - Via Fusabi helper:
    - `resolve("auth://spotify/personal/token")`

Sigilforge becomes a reusable infrastructure piece:

- Tools can be built assuming Sigilforge will always provide up-to-date credentials.
- Auth-related flows are centralized and cohesive.

## Workspace Layout (Suggested)

Suggested crates:

- `sigilforge-core/`
  - Domain model and traits
  - In-memory or simple file-based implementations
- `sigilforge-daemon/`
  - Binary crate exposing daemon API
  - Uses `sigilforge-core` for all logic
- `sigilforge-cli/`
  - CLI for human interaction and scripting

Additional directories:

- `docs/`
  - Architecture docs
  - Roadmap
  - Interface definitions
- `examples/`
  - Example integrations (if helpful)

This separation encourages reuse of `sigilforge-core` both with and without the daemon.
