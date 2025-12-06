# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/raibid-labs/sigilforge/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/raibid-labs/sigilforge/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/raibid-labs/sigilforge/releases/tag/v0.1.0
