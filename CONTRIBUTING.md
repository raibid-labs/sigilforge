# Contributing to Sigilforge

Thank you for your interest in contributing to Sigilforge! This guide will help you get started with development, testing, and submitting contributions.

## Development Environment Setup

### Rust Toolchain

Sigilforge requires **Rust 1.83 or later** with the 2024 edition enabled.

Install or update Rust using [rustup](https://rustup.rs/):

```bash
# Install rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Update to the latest stable version
rustup update stable
rustup default stable

# Verify version (should be 1.83+)
rustc --version
```

### Optional Tools

While not strictly required, these tools improve the development experience:

**cargo-watch** - Automatically rebuild and test on file changes:
```bash
just install-watch
# or manually:
cargo install cargo-watch

# Usage:
just watch
```

**cargo-audit** - Check dependencies for security vulnerabilities:
```bash
just install-audit
# or manually:
cargo install cargo-audit

# Usage:
just audit
```

### IDE Setup

We recommend using an editor with [rust-analyzer](https://rust-analyzer.github.io/) support:

- **VS Code**: Install the "rust-analyzer" extension
- **IntelliJ/CLion**: Rust plugin includes rust-analyzer
- **Vim/Neovim**: Use [coc-rust-analyzer](https://github.com/fannheyward/coc-rust-analyzer) or [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig)
- **Emacs**: Use [lsp-mode](https://emacs-lsp.github.io/lsp-mode/) with rust-analyzer

### System Dependencies

Sigilforge uses the OS keyring for secure credential storage. Ensure you have the appropriate libraries installed:

**Linux:**
```bash
# Debian/Ubuntu
sudo apt-get install libdbus-1-dev libsecret-1-dev

# Fedora/RHEL
sudo dnf install dbus-devel libsecret-devel
```

**macOS/Windows:** No additional dependencies required (uses native Keychain/Credential Manager).

## Building and Testing

Sigilforge uses [just](https://github.com/casey/just) as a command runner. Install it with:

```bash
cargo install just
```

Then run `just` to see all available commands.

### Common Build Commands

```bash
# Build all workspace crates
just build

# Build with release optimizations
just build-release

# Build specific crates
just build-daemon
just build-cli
just build-core

# Clean build artifacts
just clean

# Full rebuild from scratch
just rebuild
```

### Running Tests

```bash
# Run all tests
just test

# Run tests with output visible
just test-verbose

# Run tests for specific crates
just test-daemon
just test-cli
just test-core

# Run quick iteration cycle (check + test)
just quick
```

### Feature Flag Combinations

The `sigilforge-core` crate supports optional features. Test relevant combinations:

```bash
# Test with all features enabled
just test-core-full

# Test keyring-specific functionality
just test-keyring

# Test OAuth functionality
just test-oauth
```

### Running Integration Tests

Daemon integration tests require the daemon to be running in the background:

```bash
# Start daemon in background and run tests
cargo run -p sigilforge-daemon &
DAEMON_PID=$!
cargo test -p sigilforge-daemon
kill $DAEMON_PID
```

Or use the development environment with tmux:

```bash
# Opens split panes with daemon and CLI ready
just dev
```

## Code Style

### Formatting

Sigilforge uses **rustfmt** for consistent code formatting. Configuration is in `.rustfmt.toml`:

- Edition: 2024
- Max line width: 100 characters
- Comments are normalized and wrapped

**Format your code before committing:**

```bash
# Format all code
just fmt

# Check formatting (CI will fail if this fails)
just fmt-check
```

### Linting

All code must pass **clippy** with zero warnings:

```bash
# Run clippy (fails on warnings)
just clippy

# Run clippy with all features
just clippy-all

# Auto-fix clippy warnings (review changes carefully)
just clippy-fix
```

### Comment Conventions

- Use `///` for public API documentation
- Use `//!` for module-level documentation
- Use `//` for inline comments explaining non-obvious logic
- Document all public items (structs, enums, functions, traits)
- Include examples in doc comments for complex APIs

Example:

```rust
/// Manages OAuth2 token lifecycle and automatic refresh.
///
/// The `TokenManager` ensures that consumers always receive valid access tokens
/// by automatically refreshing expired tokens using stored refresh tokens.
///
/// # Examples
///
/// ```no_run
/// use sigilforge_core::{TokenManager, ServiceId, AccountId};
///
/// async fn example(manager: &impl TokenManager) {
///     let token = manager.ensure_access_token(
///         &ServiceId::new("spotify"),
///         &AccountId::new("personal")
///     ).await?;
///     println!("Token: {}", token);
/// }
/// ```
pub trait TokenManager {
    // ...
}
```

## Git Workflow

### Branch Naming

Use descriptive branch names following this pattern:

```
<type>/issue-<number>-<short-description>
```

Types:
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation updates
- `refactor/` - Code refactoring
- `test/` - Test additions or improvements
- `chore/` - Maintenance tasks

Examples:
```
feature/issue-25-contributing-guide
fix/issue-42-keyring-deadlock
docs/issue-18-architecture-diagrams
```

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/) style:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types:
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation changes
- `refactor` - Code refactoring
- `test` - Test changes
- `chore` - Build/tooling changes
- `perf` - Performance improvements

Examples:

```
feat(daemon): Add JSON-RPC server with Unix socket support

Implements a local IPC server using jsonrpsee with Unix sockets on
Linux/macOS and named pipes on Windows. Includes basic methods for
account management and token retrieval.

Closes #10
```

```
fix(core): Prevent keyring deadlock in concurrent token refresh

Use RwLock instead of Mutex to allow concurrent reads while preventing
write conflicts during token refresh operations.

Fixes #42
```

```
docs: Add developer contributing guide

Closes #25
```

### Pull Request Process

1. **Create a feature branch** from `main`:
   ```bash
   git checkout -b feature/issue-XX-description
   ```

2. **Make your changes** following the code style guidelines

3. **Run the CI checks locally** before pushing:
   ```bash
   just ci
   ```
   This runs: `fmt-check`, `clippy`, and `test`

4. **Commit your changes** with conventional commit messages

5. **Push to your fork** and create a pull request:
   ```bash
   git push -u origin feature/issue-XX-description
   ```

6. **Fill out the PR template** with:
   - Summary of changes (bullet points)
   - Test plan and verification steps
   - Related issue numbers

7. **Address review feedback** by adding new commits (don't force-push during review)

8. **Squash commits** if requested before merge

### PR Title Format

Use the same format as commit messages:

```
feat(daemon): Add health check endpoint
fix(cli): Handle missing config file gracefully
docs: Update architecture diagrams
```

## Testing Requirements

### When to Add Tests

- **All new features** require tests demonstrating the feature works
- **Bug fixes** should include a regression test
- **Public API changes** need tests covering the new surface area
- **Refactoring** should maintain or improve test coverage

### Test Organization

```
crate/
└── src/
    ├── lib.rs
    ├── module.rs
    └── tests/          # Integration tests
        ├── mod.rs
        └── test_name.rs
```

Unit tests can live alongside code:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }
}
```

### Running Daemon Integration Tests

Integration tests that require the daemon:

```rust
#[tokio::test]
async fn test_daemon_endpoint() {
    // Tests should handle daemon not running gracefully
    // or start a test daemon instance
}
```

Run with:
```bash
just test-daemon
```

### Test Coverage Goals

While we don't enforce a specific coverage percentage, aim for:
- Critical paths: 100% coverage
- Happy paths: 100% coverage
- Error handling: Representative edge cases
- Public APIs: All documented examples should work

## Documentation

### When to Update Documentation

Update documentation when:

- Adding new public APIs or traits
- Changing existing behavior
- Adding new features or components
- Fixing bugs that affect documented behavior
- Adding new configuration options

### Documentation Locations

| File | Purpose |
|------|---------|
| `README.md` | Project overview, quick start, basic usage |
| `docs/ARCHITECTURE.md` | System design, component responsibilities |
| `docs/INTERFACES.md` | Trait definitions, API contracts |
| `docs/ROADMAP.md` | Future plans and development phases |
| `CHANGELOG.md` | Version history and release notes |
| `CONTRIBUTING.md` | This file - contribution guidelines |
| Code comments | Implementation details and doc comments |

### Changelog Maintenance

**Update `CHANGELOG.md`** for user-visible changes:

```markdown
## [Unreleased]

### Added
- Feature X that does Y (#123)

### Fixed
- Bug Z that caused W (#456)

### Changed
- Behavior A now does B instead of C (#789)
```

See [docs/RELEASE.md](docs/RELEASE.md) for the full release process.

### Versioned Documentation

When making breaking changes to architecture, update both:
1. Current documentation in `docs/`
2. Version-specific snapshot in `docs/versions/vX.Y.Z/`

See [docs/STRUCTURE.md](docs/STRUCTURE.md) for details.

## Additional Resources

- **Architecture Overview**: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- **API Contracts**: [docs/INTERFACES.md](docs/INTERFACES.md)
- **Development Roadmap**: [docs/ROADMAP.md](docs/ROADMAP.md)
- **Release Process**: [docs/RELEASE.md](docs/RELEASE.md)
- **Justfile Commands**: Run `just` to see all available commands

## Getting Help

- **Issues**: Check [existing issues](https://github.com/raibid-labs/sigilforge/issues) or open a new one
- **Discussions**: Start a [discussion](https://github.com/raibid-labs/sigilforge/discussions) for questions
- **Documentation**: Refer to the `docs/` directory for detailed technical documentation

## Code of Conduct

Be respectful, constructive, and collaborative. We're all here to build something useful together.

---

**Thank you for contributing to Sigilforge!**
