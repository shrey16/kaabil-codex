## Sandbox & approvals

Kaabil Codex supports configurable sandboxing and approval policies. These can be set in
`~/.kaabil-codex/config.toml` (or under `CODEX_HOME`).

Common keys:

- `sandbox_mode` (`read-only`, `workspace-write`, `danger-full-access`)
- `network_access` (`restricted`, `enabled`)
- `approval_policy` (`never`, `on-request`, `on-failure`, `untrusted`)

See `docs/example-config.md` and `docs/execpolicy.md` for related settings.
