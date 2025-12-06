# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Release pipeline with automated builds for multiple platforms
- Documentation versioning structure
- RELEASE.md with detailed release process documentation
- CODEOWNERS file for repository governance
- GitHub Actions release workflow with binary artifacts and checksums

### Changed
- Updated README with documentation structure references

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

[Unreleased]: https://github.com/raibid-labs/sigilforge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/raibid-labs/sigilforge/releases/tag/v0.1.0
