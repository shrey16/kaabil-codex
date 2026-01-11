# Configuration

Kaabil Codex reads configuration from `~/.kaabil-codex/config.toml` (or the directory set by `CODEX_HOME`).
Use `docs/example-config.md` as a starting point and adjust settings as needed.

## Agent orchestration

For the kaabil-codex multi-agent orchestration feature, see `docs/agent-orchestration.md`.

## Tool policy

Use `tool_policy` in `config.toml` to allow or deny tools and shell commands for
the entire session:

```toml
[tool_policy]
tool_allowlist = ["shell", "mcp__playwright__*"]
tool_denylist = ["apply_patch"]
shell_command_allowlist = ["rg *", "cargo test -p codex-*"]
shell_command_denylist = ["cargo test --all-features*"]
```

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.kaabil-codex/config.toml`.
See `docs/example-config.md` for a minimal `mcp_servers` stanza.

## Notify

Codex can run a notification hook when the agent finishes a turn.
Use `docs/example-config.md` for a minimal `notify` example.
