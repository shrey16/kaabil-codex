# Execution policy

Kaabil Codex loads `.rules` files from the `rules/` folder in each config layer:

- `/etc/codex/rules` (system, if present)
- `${CODEX_HOME}/rules` (defaults to `~/.kaabil-codex/rules`)
- `./.codex/rules` in the repo root or parent directories

Rules are evaluated from lowest to highest precedence. Each matching rule returns a decision:

- `allow`: run the command (may bypass sandbox if explicitly allowed)
- `prompt`: request approval before running
- `forbidden`: block the command

See `docs/example-config.md` and `docs/sandbox.md` for related configuration.
