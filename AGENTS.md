# Agent instructions

Vaultty is a desktop SSH client with an encrypted vault (hosts, keys, tunnels, terminal, SFTP and an optional
MCP server), written in Rust with GPUI. It stores credentials: correctness and security come before speed.

## Skills: read before the first edit

The standards live in `.agents/skills/<name>/SKILL.md`. Read the ones that apply to the files you touch, in
full, before editing; they are short.

| Skill | Read it when |
|---|---|
| `vaultty-coding-standard` | Always: any Rust change in `crates/*` |
| `vaultty-security-rules` | The change touches secrets, the vault format, host keys, the MCP server, tunnels or proxies, the updater, or parses data from a server or an imported file |
| `vaultty-desktop-ui` | The change touches `crates/desktop` |

## Layout

- `crates/core` (lib `vault`), `store`, `vaults`: the encrypted vault, from the format up to its operations.
- `crates/ssh`, `sftp`, `proxy`, `terminal`: one protocol each. `crates/sessions`: tabs, tunnels, connections.
- `crates/mcp`: the MCP server. `crates/settings`, `themes`, `updater`, `system`: app support.
- `crates/desktop`: the UI, and only the UI. `crates/licenses`: development tool.

## Commands

```sh
just check                    # formatting, clippy (warnings are errors) and tests: what CI runs
cargo test -p <crate>         # tests of one crate
scripts/clippy -p <crate>     # Windows: scripts/clippy.ps1 -p <crate>
cargo run -p desktop          # run the app
```

## Rules that have no exception

- Everything written is in English: code, comments, UI text, errors, commit messages.
- Do the smallest change that fully solves the task; do not refactor what was not asked for.
- Run the checks of the crates you touched and report their real output. Never say something works without
  having run it.
- Do not commit, push, bump `gpui-kit`, add dependencies or change the on-disk vault format unless asked.
- No credits or attribution lines for AI tools in commits, pull requests, code or documentation.
- If a security rule and the task conflict, stop and ask.
