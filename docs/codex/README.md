# Sigilforge

Sigilforge is a local **auth and credential manager** for the raibid-labs
ecosystem. It acts as a small, personal "vault + token service" that:

- Manages API keys, OAuth tokens, and other sensitive values.
- Runs OAuth flows (device code, PKCE, etc.) on behalf of client apps.
- Stores and refreshes OAuth tokens securely.
- Provides a **reference-based interface** for requesting tokens and secrets.
- Can integrate with multiple secret backends (OS keyring, encrypted files,
  vals-style references, etc.).

Sigilforge is designed to be used by:

- **Scryforge** — the information rolodex TUI/daemon.
- **Phage** and other processing tools.
- Future Fusabi-based applications in the raibid-labs ecosystem.

The core goals are:

- Centralize authentication and secret handling.
- Keep application code free of token storage and OAuth flow complexity.
- Provide a reusable **library + daemon** that any tool can talk to.
