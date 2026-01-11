# Sample configuration

Save this as `~/.kaabil-codex/config.toml` (or under `CODEX_HOME`) and tweak to taste.

```toml
model = "gpt-5.1"
sandbox_mode = "read-only"
approval_policy = "never"

[features]
agent_orchestration = true

# Keep the Team Lead read-only; grant subagents tools per task via spawn overrides.
[tool_policy]
tool_denylist = ["apply_patch", "shell", "exec_command", "write_stdin", "local_shell", "shell_command"]

# Run a notification when the agent finishes a turn.
notify = ["notify-send", "Kaabil Codex finished a turn"]

[mcp_servers.shell-tool]
command = "node"
# Replace with your local checkout path.
args = ["/path/to/kaabil-codex/shell-tool-mcp/bin/mcp-server.js"]
```
