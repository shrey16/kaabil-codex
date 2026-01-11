# AGENTS.md

`AGENTS.md` files define repository-specific instructions for the agent. The file’s scope
applies to its directory tree, with deeper files overriding higher-level guidance.

Add an `AGENTS.md` near the repo root to document coding standards, commands, and expectations.

## Hierarchical agents message

When the `hierarchical_agents` feature flag is enabled (via `[features]` in `config.toml`), Codex appends additional guidance about AGENTS.md scope and precedence to the user instructions message and emits that message even when no AGENTS.md is present.
