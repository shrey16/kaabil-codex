<p align="center"><code>cargo install --path codex-rs/cli --locked</code></p>
<p align="center"><strong>Kaabil Codex CLI</strong> is an experimental fork of Codex CLI for local development.
<p align="center">
  <img src="./.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
</p>

---

## Kaabil fork notice

This is an experimental fork of Codex CLI built and maintained by Kaabil for its own development purposes.
It is not the official upstream Codex distribution. Use at your own risk.

Versioning follows upstream with a `-kaabil.N` suffix (example: `0.80.0-kaabil.2`).
To bump to a newer upstream release, run `./scripts/bump-kaabil-version.sh --upstream <version>`
or `./scripts/bump-kaabil-version.sh --auto` to fetch the latest upstream tag.

## Quickstart

### Local setup (from source)

This fork is intended for local builds only. A minimal setup looks like:

```shell
# Clone
git clone https://github.com/shrey16/kaabil-codex.git
cd kaabil-codex

# Build and install the CLI from source
cargo install --path codex-rs/cli --locked
```

If you prefer a `kaabil-codex` command name, add a local alias:

```shell
ln -s ~/.cargo/bin/codex ~/.local/bin/kaabil-codex
```

Ensure `~/.local/bin` is on your `PATH` (or use a different target directory).

Create a config at `~/.kaabil-codex/config.toml` (or set `CODEX_HOME`):

```toml
[features]
agent_orchestration = true
```

To enable MCP tools, add `mcp_servers` entries in the same config file. If you are migrating
from upstream Codex, copy the `[mcp_servers.*]` blocks from `~/.codex/config.toml`.

Run `codex` (or `kaabil-codex`) to start a session. Sessions are saved under
`~/.kaabil-codex/sessions`; `/resume` filters by your current working directory, and
`codex resume --all` shows every session across projects.

## Docs

- [**Installing & building**](./docs/install.md)
- [**Configuration**](./docs/config.md)
- [**Agent orchestration**](./docs/agent-orchestration.md)
- [**Models**](./docs/models.md)
- [**Contributing**](./docs/contributing.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
