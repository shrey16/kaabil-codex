# Sample configuration

Save this as `~/.kaabil-codex/config.toml` (or under `CODEX_HOME`) and tweak to taste.

```toml
model = "gpt-5.1"

[features]
agent_orchestration = true

# Run a notification when the agent finishes a turn.
notify = ["notify-send", "Kaabil Codex finished a turn"]

[mcp_servers.shell-tool]
command = "node"
# Replace with your local checkout path.
args = ["/path/to/kaabil-codex/shell-tool-mcp/bin/mcp-server.js"]
```
