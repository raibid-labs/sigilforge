# Sigilforge TUI

Interactive terminal user interface for Sigilforge OAuth token management.

## Overview

`sigilforge-tui` provides a rich, interactive TUI for managing OAuth tokens and viewing account status. It connects to the Sigilforge daemon and displays real-time token information with color-coded status indicators.

## Features

- **Account List**: View all configured OAuth accounts with status indicators
- **Token Status**: Color-coded display (green=valid, yellow=expiring soon, red=expired)
- **Account Details**: Detailed view of scopes, expiration, and timestamps
- **Token Refresh**: Manually refresh tokens for individual or all accounts
- **Keyboard Navigation**: Vim-style (j/k) and arrow key navigation
- **Auto-refresh**: Automatic account list refresh every 30 seconds

## Installation

Build the TUI as part of the Sigilforge workspace:

```bash
cargo build -p sigilforge-tui
```

Or build in release mode for better performance:

```bash
cargo build -p sigilforge-tui --release
```

## Usage

Start the TUI:

```bash
# From workspace root
cargo run -p sigilforge-tui

# Or if installed
sigilforge-tui
```

### Keyboard Shortcuts

- `j` / `↓` - Select next account
- `k` / `↑` - Select previous account
- `g` - Jump to first account
- `G` - Jump to last account
- `r` - Refresh selected account's token
- `a` - Refresh all accounts
- `q` - Quit

## Requirements

- **Sigilforge daemon must be running**: Start with `sigilforged`
- **Configured accounts**: Use `sigilforge` CLI to add OAuth accounts

## Architecture

The TUI is built using the fusabi-tui-runtime framework:

- **fusabi-tui-core**: Core TUI primitives (buffer, layout, styling)
- **fusabi-tui-render**: Crossterm-based terminal rendering
- **fusabi-tui-widgets**: Widget library (blocks, lists, paragraphs)

### Application Structure

```
sigilforge-tui/
├── src/
│   ├── main.rs      # Entry point, event loop
│   ├── app.rs       # Application state management
│   └── ui.rs        # UI rendering with widgets
└── Cargo.toml
```

## Error Handling

If the daemon is unavailable:
- TUI displays "Daemon Unavailable" warning
- Empty account list with instructions to start daemon
- No token operations are possible

## Development

Run with logging enabled:

```bash
RUST_LOG=debug cargo run -p sigilforge-tui
```

Logs are written to stderr to avoid interfering with the TUI display.

## Future Enhancements

- [ ] Add account (OAuth flow from TUI)
- [ ] Remove account
- [ ] View token details (masked)
- [ ] Search/filter accounts
- [ ] Export account list
- [ ] Configuration view
- [ ] Help screen with detailed key bindings

## Color Theme

The TUI uses the Sigilforge color scheme:

- **Cyan**: Primary UI elements, branding
- **Green**: Valid tokens, success
- **Yellow**: Expiring soon warnings
- **Red**: Expired tokens, errors
- **White**: Normal text
- **Dark Gray**: Dimmed/secondary text

## License

Same as parent Sigilforge project (MIT).
