# Sigilforge Interfaces

This document describes the core interfaces exposed by Sigilforge and
how applications are expected to interact with it.

## Core Types (Conceptual)

### ServiceId

Represents a logical external service, e.g.:

- `gmail`
- `spotify`
- `reddit`
- `youtube`
- `msgraph`
- `github`

### AccountId

Represents a named account under a given service, e.g.:

- `personal`
- `work`
- `lab`

### Account

Ties together a service and an account alias with associated configuration:

- `service: ServiceId`
- `alias: AccountId`
- Additional metadata, such as:
  - Human-friendly display name
  - Default scopes for OAuth flows
  - References to client credentials and tokens

### CredentialRef

A logical reference to a secret:

- For example:
  - `auth://gmail/personal/token`
  - `auth://spotify/lab/refresh`
  - `secret://project-x/db-password`
  - `vals:ref+vault://kv/data/proj#db-pass`

### SecretValue

Opaque string or binary value representing a secret, such as an API key,
refresh token, or password. It may carry additional flags:

- `is_binary: bool`
- `metadata: Map<String, String>`

### Token

Represents an OAuth token set:

- `access_token: String`
- `refresh_token: Option<String>`
- `expires_at: Option<DateTime>`
- `scopes: Vec<String>`
- `raw_response: Option<String>` (for debugging or provider-specific fields)

## Traits

### SecretStore

Abstracts underlying secret storage.

Conceptual interface:

```rust
pub trait SecretStore {
    fn get(&self, cref: &CredentialRef) -> Result<Option<SecretValue>, SecretError>;
    fn set(&mut self, cref: &CredentialRef, value: SecretValue) -> Result<(), SecretError>;
    fn delete(&mut self, cref: &CredentialRef) -> Result<(), SecretError>;
}
```

Implementations:

- In-memory store (for tests).
- Keyring-backed store.
- Encrypted-file-backed store.
- Composite store layered on multiple backends.

### TokenManager

Responsible for token lifecycle.

Conceptual interface:

```rust
pub trait TokenManager {
    fn get_access_token(
        &self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Option<Token>, TokenError>;

    fn ensure_access_token(
        &mut self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<Token, TokenError>;

    fn revoke(
        &mut self,
        service: &ServiceId,
        account: &AccountId,
    ) -> Result<(), TokenError>;
}
```

Typical responsibilities:

- Look up tokens via `SecretStore`.
- Determine whether a token is expired.
- Refresh tokens using provider-specific OAuth endpoints and client credentials.
- Persist updated tokens via `SecretStore`.

### ReferenceResolver

Resolves textual references into `SecretValue`.

Conceptual interface:

```rust
pub trait ReferenceResolver {
    fn resolve(&self, reference: &str) -> Result<SecretValue, ResolveError>;
}
```

Responsibilities:

- Parse reference schemes:
  - `auth://service/account/field`
  - `secret://namespace/path`
  - `vals:ref+...://...`
- Delegate:
  - `auth://` references to `TokenManager` and `SecretStore`.
  - `secret://` references to `SecretStore`.
  - `vals:` references to an external tool or plugin that understands vals.

## Reference Formats

### auth://

Used by applications to obtain tokens or auth-related secrets.

- `auth://service/account/token`
  - Return a valid access token.
- `auth://service/account/refresh`
  - Return a refresh token.
- `auth://service/account/client_id`
  - Return client ID for that account.
- `auth://service/account/client_secret`
  - Return client secret for that account.

Examples:

- `auth://spotify/personal/token`
- `auth://gmail/work/client_id`

### secret://

Used for non-auth secrets:

- `secret://namespace/path`

Examples:

- `secret://project-x/db-password`
- `secret://infra/alertmanager/webhook-token`

These map directly to entries in the configured `SecretStore`.

### vals:...

Used for compatibility with vals-style references:

- `vals:ref+vault://...`
- `vals:ref+awsssm://...`

Sigilforge can:

- Shell out to `vals` (at first) to resolve these.
- Or later, implement direct integrations for some backends.

Applications can treat these as references passed to Sigilforge rather than
calling vals directly.

## Daemon API Surface (Conceptual)

A simple JSON-RPC-like interface might look like:

```json
{
  "method": "list_accounts",
  "params": {}
}
```

```json
{
  "method": "get_access_token",
  "params": {
    "service": "spotify",
    "account": "personal"
  }
}
```

```json
{
  "method": "resolve",
  "params": {
    "reference": "auth://spotify/personal/token"
  }
}
```

The exact JSON schema and transport (Unix socket vs TCP) will be defined
during implementation, but the logical operations remain:

- `list_accounts` → returns known ServiceId/AccountId pairs.
- `get_access_token` / `ensure_access_token` → returns token string and metadata.
- `resolve` → returns `SecretValue` for any recognized reference.

## Usage by Scryforge (Example)

Scryforge might call Sigilforge via a Rust client or Fusabi module as follows:

- Configure accounts:
  - `spotify/personal`
  - `reddit/main`
  - `gmail/work`
- For Spotify provider:
  - When it needs a token:
    - Calls `ensure_access_token("spotify", "personal")`.
- For Gmail IMAP:
  - It might use `auth://gmail/work/app_password` or similar, depending on setup.

This keeps Scryforge and other tools free of direct secret management and
concentrates auth/security responsibilities in Sigilforge.
