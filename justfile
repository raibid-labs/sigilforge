# Sigilforge Justfile
# Commands for building, running, and testing the Sigilforge credential management system

# Default recipe - show available commands
default:
    @just --list

# ========================================
# Build Commands
# ========================================

# Build all workspace crates
build:
    cargo build

# Build with release optimizations
build-release:
    cargo build --release

# Build the daemon only
build-daemon:
    cargo build -p sigilforge-daemon

# Build the CLI only
build-cli:
    cargo build -p sigilforge-cli

# Build the core library only
build-core:
    cargo build -p sigilforge-core

# Build with all features enabled
build-full:
    cargo build -p sigilforge-core --features full

# Clean build artifacts
clean:
    cargo clean

# ========================================
# Running
# ========================================

# Run the daemon (sigilforged)
daemon:
    #!/usr/bin/env bash
    echo "Starting Sigilforge daemon..."
    cargo run -p sigilforge-daemon

# Run the CLI
cli *ARGS:
    cargo run -p sigilforge-cli -- {{ARGS}}

# Run daemon in background
daemon-bg:
    #!/usr/bin/env bash
    echo "Starting daemon in background..."
    cargo run -p sigilforge-daemon > /tmp/sigilforged.log 2>&1 &
    DAEMON_PID=$!
    echo "Daemon PID: $DAEMON_PID"
    echo "Logs: /tmp/sigilforged.log"

# Run daemon in release mode
daemon-release:
    #!/usr/bin/env bash
    set -euo pipefail

    BIN_DIR="${CARGO_TARGET_DIR:-target}/release"
    BIN_DIR="${BIN_DIR/#\~/$HOME}"

    if [ ! -x "$BIN_DIR/sigilforged" ]; then
        echo "Release binary not found. Building..."
        cargo build --release -p sigilforge-daemon
    fi

    echo "Starting Sigilforge daemon (release)..."
    "$BIN_DIR/sigilforged"

# Run daemon and CLI in separate tmux panes
dev:
    #!/usr/bin/env bash
    if ! command -v tmux &> /dev/null; then
        echo "Error: tmux not found. Install tmux or use 'just daemon-bg' instead."
        exit 1
    fi

    SESSION="sigilforge-dev"

    if tmux has-session -t $SESSION 2>/dev/null; then
        echo "Attaching to existing session: $SESSION"
        tmux attach-session -t $SESSION
    else
        echo "Creating new tmux session: $SESSION"
        # Create session with daemon in first pane
        tmux new-session -d -s $SESSION -n "sigilforge" "cargo run -p sigilforge-daemon"
        # Split window for CLI usage
        tmux split-window -h -t $SESSION "sleep 2 && echo 'Daemon started. CLI ready:' && echo 'cargo run -p sigilforge-cli -- --help' && bash"
        # Select layout and attach
        tmux select-layout -t $SESSION even-horizontal
        tmux attach-session -t $SESSION
    fi

# ========================================
# Testing
# ========================================

# Check all crates for errors
check:
    cargo check --workspace

# Run tests for all crates
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run tests for a specific crate
test-crate CRATE:
    cargo test -p {{CRATE}}

# Run daemon tests
test-daemon:
    cargo test -p sigilforge-daemon

# Run CLI tests
test-cli:
    cargo test -p sigilforge-cli

# Run core library tests
test-core:
    cargo test -p sigilforge-core

# Run core tests with all features
test-core-full:
    cargo test -p sigilforge-core --features full

# Run only keyring-related tests
test-keyring:
    cargo test -p sigilforge-core --features keyring-store keyring

# Run only OAuth tests
test-oauth:
    cargo test -p sigilforge-core --features oauth oauth

# ========================================
# Code Quality
# ========================================

# Format code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints
clippy:
    cargo clippy --workspace -- -D warnings

# Run clippy with all features
clippy-all:
    cargo clippy --workspace --all-features -- -D warnings

# Run clippy with fixes
clippy-fix:
    cargo clippy --workspace --fix --allow-dirty --allow-staged

# Run all quality checks (format, clippy, test)
ci: fmt-check clippy test

# Quick iteration: check + test
quick: check test

# ========================================
# Documentation
# ========================================

# Build Rust API documentation
doc:
    cargo doc --workspace --no-deps --open

# Build documentation with all features
doc-all:
    cargo doc --workspace --all-features --no-deps --open

# Build documentation including private items
doc-private:
    cargo doc --workspace --no-deps --document-private-items --open

# ========================================
# Dependencies
# ========================================

# Check dependency tree
tree:
    cargo tree

# Update dependencies
update:
    cargo update

# Audit dependencies for security issues
audit:
    cargo audit

# ========================================
# Installation
# ========================================

# Install binaries to ~/.local/bin (or custom prefix)
install PREFIX="~/.local":
    #!/usr/bin/env bash
    set -e

    PREFIX_VALUE="{{PREFIX}}"
    PREFIX_EXPANDED="${PREFIX_VALUE/#\~/$HOME}"
    BIN_DIR="$PREFIX_EXPANDED/bin"

    echo "Installing Sigilforge to $PREFIX_EXPANDED"

    # Build release binaries
    echo "Building release binaries..."
    cargo build --release

    # Detect cargo target directory
    if [ -f "target/release/sigilforged" ]; then
        TARGET_DIR="target/release"
    elif [ -f "$HOME/.cargo/target/release/sigilforged" ]; then
        TARGET_DIR="$HOME/.cargo/target/release"
    else
        echo "Error: Could not find release binaries"
        exit 1
    fi

    echo "Found binaries in: $TARGET_DIR"

    # Create directory
    mkdir -p "$BIN_DIR"

    # Install binaries
    echo "Installing binaries..."
    cp "$TARGET_DIR/sigilforged" "$BIN_DIR/"
    cp "$TARGET_DIR/sigilforge" "$BIN_DIR/"

    # Make executable
    chmod +x "$BIN_DIR/sigilforged"
    chmod +x "$BIN_DIR/sigilforge"

    echo "Installed sigilforged -> $BIN_DIR/sigilforged"
    echo "Installed sigilforge  -> $BIN_DIR/sigilforge"
    echo ""
    echo "Installation complete!"
    echo ""
    echo "Make sure $BIN_DIR is in your PATH:"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""

# Uninstall binaries from ~/.local/bin (or custom prefix)
uninstall PREFIX="~/.local":
    #!/usr/bin/env bash
    PREFIX_VALUE="{{PREFIX}}"
    PREFIX_EXPANDED="${PREFIX_VALUE/#\~/$HOME}"
    BIN_DIR="$PREFIX_EXPANDED/bin"

    echo "Uninstalling Sigilforge from $PREFIX_EXPANDED"

    rm -f "$BIN_DIR/sigilforged"
    rm -f "$BIN_DIR/sigilforge"

    echo "Uninstalled from $PREFIX_EXPANDED"
    echo ""
    echo "Note: Config files remain in ~/.config/sigilforge"
    echo "To remove config: rm -rf ~/.config/sigilforge"

# ========================================
# Utility Commands
# ========================================

# Kill any running sigilforge processes
kill:
    #!/usr/bin/env bash
    echo "Killing sigilforge processes..."
    pkill -f sigilforged || true
    echo "Done"

# Full cleanup (kill processes + cargo clean)
nuke: kill clean

# Show build status
status:
    @echo "Sigilforge Build Status"
    @echo "======================="
    @cargo --version
    @rustc --version
    @echo ""
    @echo "Workspace crates:"
    @cargo metadata --no-deps --format-version 1 | grep -o '"name":"[^"]*"' | cut -d'"' -f4

# Build specific crate
build-crate CRATE:
    cargo build -p {{CRATE}}

# Run specific crate
run-crate CRATE:
    cargo run -p {{CRATE}}

# Watch and rebuild on changes (requires cargo-watch)
watch:
    cargo watch -x check -x test

# Install cargo-watch if not present
install-watch:
    cargo install cargo-watch

# Install cargo-audit if not present
install-audit:
    cargo install cargo-audit

# Full rebuild from scratch
rebuild: clean build

# Run benchmarks
bench:
    cargo bench --workspace

# ========================================
# Aliases
# ========================================

# Alias for build-release
alias br := build-release

# Alias for test
alias t := test

# Alias for fmt
alias f := fmt

# Alias for clippy
alias l := clippy

# Alias for check
alias c := check

# Alias for doc
alias d := doc

# Alias for daemon
alias da := daemon
