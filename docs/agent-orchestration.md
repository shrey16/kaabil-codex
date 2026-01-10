# Agent orchestration

This fork adds an experimental orchestration mode for multi-agent workflows.
When enabled, the main agent adopts an orchestrator persona and can spawn
subagents with distinct personas. Subagents can report back to the
orchestrator using `send_input`.

Enable the feature in `~/.codex/config.toml`:

```toml
[features]
agent_orchestration = true
```

Notes:
- Collab tools (`spawn_agent`, `send_input`, `wait`, `close_agent`) are enabled
  automatically when `agent_orchestration` is on.
- `spawn_agent` accepts an optional `persona` string to specialize the agent.
