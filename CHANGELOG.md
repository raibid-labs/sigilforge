# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **`EncryptedFileStore`** - an age-encrypted file backing the `SecretStore`
  trait, so Sigilforge works on a headless host (`encrypted-file-store` feature,
  on by default)
  - Encryption via the `age` crate (X25519 + ChaCha20-Poly1305). A Rust library
    rather than a `sops`/`age`/`gpg` subprocess: a missing binary on a minimal
    server is the same class of failure as the missing D-Bus session bus this
    backend exists to route around. The file is still a standard age file, so
    `age -d -i <identity> <store>` recovers it without Sigilforge.
  - No session bus, no desktop, no agent, and no prompt on read, so daemons and
    CI can use it unattended
  - Identity file in the config directory, ciphertext in the data directory,
    both created `0600`; an identity readable by group or other is **refused**
  - Atomic whole-store rewrite (temp file, `fsync`, rename) under an advisory
    lock; reads decrypt fresh, so a value written by one process is visible to
    the next
  - `list_keys` works, which it never did on the platform keyrings
  - A zero-byte store file is reported as truncation, not as an empty store
- `sigilforge store init` - first-run setup for a headless host: generates the
  age identity, sets permissions, prints where it lives and that losing it loses
  every secret
- `sigilforge store status` - which backend is selected, what each one reports,
  the paths in play, and the environment overrides
- `open_store` / `open_store_with` / `probe_backends` / `init_encrypted_store`,
  `StoreBackend`, `StoreConfig`, `BackendProbe` in `sigilforge_core::store`
- `KeyringStore::probe` - a real write/read/delete round-trip against the
  platform keyring
- Backend selection by `SIGILFORGE_STORE_BACKEND` (or `[storage] backend` in
  `~/.config/sigilforge/config.toml`), plus `SIGILFORGE_AGE_IDENTITY` and
  `SIGILFORGE_SECRETS_FILE` path overrides
- `StoreError::{BackendUnavailable, NoBackend, InsecurePermissions}`
- GitHub App authentication in `sigilforge-core` (`github_app` module, `github-app` feature)
  - `GitHubAppCredential` holding app id, installation id, and a PEM private key;
    the key is validated on construction, stored in the `SecretStore`, and
    redacted from `Debug`/`Display`
  - RS256 JWT construction with a 60s clock-skew backdate and `exp` clamped to
    GitHub's 10-minute ceiling
  - `GitHubAppTokenManager` mints and caches installation tokens, re-minting only
    within 5 minutes of expiry; implements `TokenManager`, `ReferenceResolver`,
    and `InstallationTokenSource`
  - `auth://github-app/{account}/installation_token` resolution, plus
    `DefaultReferenceResolver::with_github_app`
  - `ArgoCdRepositorySecret` renders an Argo CD repository `Secret` manifest
- `CredentialType::{AppId, InstallationId, PrivateKey, InstallationToken}`
- `ResolveError::NotConfigured` for well-formed references a resolver cannot serve
- CLI: `sigilforge github-app register|list|token|argocd-secret|remove`
- `docs/GITHUB_APP.md` covering the one-time GitHub setup and the Argo CD path

### Fixed
- **Reads reported "not found" when they meant "could not look".** `create_store`
  silently substituted a `MemoryStore` whenever the keyring could not be
  constructed, so on a host with no D-Bus session bus `github-app list` printed
  "No GitHub Apps registered" for an App that was registered, `get-token` and
  `resolve` said the credential did not exist, and `remove-account` reported
  success while deleting nothing. Storage is now probed with a real round-trip,
  and an unreachable backend is an error naming the backend and the reason.
  `MemoryStore` is reachable only by asking for it by name.
- `KeyringStore::try_new` still only constructs an `Entry` (it cannot do better
  without contacting the platform), but nothing relies on it as an availability
  check any more - `open_store` calls `probe` as well.
- `github-app list` opens the secret store *before* reading `accounts.json`, so
  "No GitHub Apps registered" is printed only when storage works and is empty.
- `list-accounts` warns when account metadata is readable but the credentials
  behind it are not.
- **`KeyringStore` did not persist anything.** The `keyring` crate ships no
  credential store in its default features and silently falls back to an
  in-process mock, so every secret written "to the OS keyring" was lost at
  process exit. Platform backends (`sync-secret-service`, `apple-native`,
  `windows-native`) are now selected explicitly. Linux builds need
  `libdbus-1-dev` and `pkg-config`, which CI now installs.
- `GitHubAppTokenManager::register` reads the private key back after writing, so
  a store that accepts writes without persisting them fails at registration
  rather than in a cluster an hour later.

### Changed
- **Breaking:** `create_store(bool)` returns `Result<Box<dyn SecretStore>, StoreError>`
  instead of an infallible `Box<dyn SecretStore>`. The old signature had nowhere
  to report "no storage", which is why it invented one. Prefer `open_store()`.
- The daemon refuses to start without working secret storage, rather than
  serving an in-memory store that answers every request with "not found".
- `sigilforge-core` default features now include `encrypted-file-store`.
- `docs/ROADMAP.md`: Phase 3 marked complete; the `get_token` and `resolve`
  handlers it described as stubs were wired up in 0.3.0
- `docs/NEXT_STEPS.md` rewritten - it described the whole CLI as stubbed, which
  has not been true since 0.1.0

## [0.3.0] - 2025-12-14

### Added
- `sigilforge-client` crate providing a reusable client library for the daemon
- `sigilforge-tui` crate: terminal UI built on the fusabi-tui-runtime (#41)
- `scarab-sigilforge` plugin for Scarab status bar integration (#40)
- `accounts_status` RPC endpoint on the daemon (#39)
- OAuth browser flow for `add-account` in the CLI
- Daemon RPC handlers wired to the real `TokenManager` and `ReferenceResolver`

### Changed
- Switched `fusabi-tui-runtime` dependencies from the git source to crates.io
- Security hardening with async locks across the daemon and core
- Bumped workspace version to 0.3.0

## [0.2.0] - 2025-12-05

### Added
- Comprehensive CI/CD infrastructure with GitHub Actions workflows
  - Lint workflow with rustfmt and clippy
  - Test suite across Linux, macOS, Windows with stable and beta Rust
  - Code coverage reporting with cargo-tarpaulin
  - Security audits with cargo-audit
  - Documentation structure validation
- Release pipeline with automated builds for multiple platforms
  - Linux x86_64 (glibc and musl)
  - macOS x86_64 and ARM64 (Apple Silicon)
  - Windows x86_64
  - SHA256 checksums for all artifacts
  - Automated crates.io publishing
- Documentation versioning structure (docs/versions/)
- RELEASE.md with detailed release process documentation
- STRUCTURE.md describing documentation organization
- CODEOWNERS file for repository governance
- CHANGELOG.md following Keep a Changelog format
- Rustfmt configuration (.rustfmt.toml)
- Dependency management workflow with weekly checks
- Documentation check workflow with link validation

### Changed
- Updated README with documentation structure references
- Enhanced .gitignore for coverage artifacts

## [0.1.0] - 2024-01-15

### Added
- Initial project scaffolding
- Core types: ServiceId, AccountId, Account, CredentialRef
- Traits: SecretStore, TokenManager, ReferenceResolver
- In-memory SecretStore implementation
- Token expiry handling
- Reference URI parsing (`auth://` scheme)
- CLI structure with subcommands
- Daemon placeholder with configuration loading
- Comprehensive architecture documentation
- Development roadmap
- Interface documentation

### Project Structure
- `sigilforge-core`: Core library with domain types and traits
- `sigilforge-daemon`: Background service placeholder
- `sigilforge-cli`: Command-line interface
- Documentation in `docs/` directory

[Unreleased]: https://github.com/raibid-labs/sigilforge/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/raibid-labs/sigilforge/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/raibid-labs/sigilforge/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/raibid-labs/sigilforge/releases/tag/v0.1.0
