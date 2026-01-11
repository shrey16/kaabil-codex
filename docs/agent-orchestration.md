# Agent orchestration

This fork adds an experimental orchestration mode for multi-agent workflows.
When enabled, the main agent adopts a Team Lead persona and can spawn subagents
with distinct personas. The session behaves like a group chat shared by the
human, Team Lead, and subagents.

Enable the feature in `~/.kaabil-codex/config.toml`:

```toml
[features]
agent_orchestration = true
```

Notes:
- Collab tools (`spawn_agent`, `send_input`, `wait`, `close_agent`, `list_agents`,
  `agent_output`) are enabled automatically when `agent_orchestration` is on.
- `spawn_agent` accepts an optional `persona` string to specialize the agent.
- `spawn_agent` also accepts optional tool allow/deny lists and shell command
  allow/deny lists to restrict what the subagent can access.
- `wait` and `close_agent` accept an optional `timeout_ms`.
- A default trio of subagents (Planner, Builder, Reviewer) is spawned when a
  session starts; use `list_agents` to discover their ids and status.
- Human and Team Lead messages are always appended to the group chat.
- Subagent final replies are forwarded to the group chat automatically. Use
  `send_input` for interim updates or to ask the Team Lead to coordinate.
- Mention subagents with `@<short-id>` or `@<persona>` (for example, `@planner`).
  You can also use the explicit form `[[subagent:<full-id>]]`.
- Subagents receive unread group chat history only when mentioned.
- `agent_output` returns partial output plus recent reasoning and tool events so
  the Team Lead can inspect progress on demand.
- In the TUI, `/agents` lists subagents and lets the human send a group chat ping.

## Restricting subagent tools

Subagents inherit the parent session's tool access by default. To restrict a
subagent, pass allow/deny lists when spawning:

```json
{
  "message": "Run the test suite you are allowed to run.",
  "persona": "Tester",
  "tool_allowlist": ["shell", "mcp__playwright__*"],
  "tool_denylist": ["apply_patch"],
  "shell_command_allowlist": ["cargo test -p codex-*", "rg *"],
  "shell_command_denylist": ["cargo test --all-features*"]
}
```

Tool and command lists accept `*` and `?` wildcards. `tool_allowlist` applies to
tool names (for MCP tools use `mcp__<server>__<tool>`). Shell command patterns
are matched against the raw command string (`shell_command`/`exec_command`) or a
space-joined command for `shell` tool calls.
