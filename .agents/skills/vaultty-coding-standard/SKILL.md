---
name: vaultty-coding-standard
description: Use when writing, editing or reviewing any Rust code in this repository (crates/*), before the first edit - covers crate boundaries, style, locking, I/O, errors, tests and the checks that must pass before a change is done.
---

# Vaultty coding standard

The rules every change follows. `CONTRIBUTING.md` is the source for style; this adds what an agent gets wrong
without being told. Touching `core`, `store`, `vaults`, `ssh`, `proxy`, `mcp` or `updater`? Also load
`vaultty-security-rules`. Touching `crates/desktop`? Also load `vaultty-desktop-ui`.

## Where code goes

| Crate | Owns | Must not |
|---|---|---|
| `core` (lib `vault`) | data model, encryption | do I/O |
| `store` | vault folder on disk | know record contents |
| `vaults` | known and open vaults, every change to them | know sessions or UI |
| `ssh`, `sftp`, `proxy`, `terminal` | one protocol each | know vaults or UI |
| `sessions` | tabs, tunnels, connections; emits `Event` | depend on `desktop` |
| `mcp` | MCP tools over `sessions`/`vaults` | return secrets |
| `settings` | `settings.json`, one key per crate | touch other crates' keys |
| `desktop` | UI only | hold logic that a mobile UI would need again |

A dependency only points down this table. Logic first goes in a crate, with a test; the UI calls it.

## Style

- Everything in English: UI text, errors, comments, test names, MCP descriptions.
- Every file starts with the SPDX and copyright header the other files have.
- Comments say why, in one or two lines. No comment that repeats the code. No task markers (CI rejects them).
- Blank line between blocks (`if`, `for`, `match`, `let … else`); `rustfmt` does not add them.
- Abstract on the second or third use, never "just in case". Prefer a fix that deletes code.
- No new dependency if the standard library or a crate already in `Cargo.lock` does it. A new one needs
  `cargo run -p licenses` and must pass `cargo deny`.
- A deliberate limitation is marked `// shortcut: <the limit and when to lift it>`.

## Correctness rules

- **One lock, one change.** Read, modify and save shared state inside a single lock scope
  (`Vaults::change`). Never read under one lock and write under another: a concurrent writer is lost.
- **No lock across slow work.** Never hold a `std::sync::Mutex` across `.await`, Argon2 or network I/O. Copy
  what is needed, release, then work.
- **Lock order:** `open` before `known` in `Vaults`; `tabs` and `tunnels` in `Sessions` are never nested.
  Emit events after dropping the lock.
- **Files that must survive a crash** are written with the atomic writer (temp file, `sync_all`, rename),
  never plain `fs::write`.
- **Errors:** low-level crates use `thiserror` enums; from `vaults` up, `Result<_, String>` with a sentence
  the user can read and act on. Never `unwrap`/`expect` on anything that comes from disk, network or the user;
  `expect("why it cannot fail")` only for true invariants.
- **Background tasks** carry a run/connection id and ignore reports from a replaced run (see `Conn::id`,
  `Running::run`).
- **Text from a server or a file is untrusted:** cap its size and validate its shape before using it.

## Tests

- New logic or a fixed bug gets one test that fails without the change. Put it in the crate that owns the
  logic, in `#[cfg(test)] mod tests` at the end of the file, with a one-line comment saying what it checks.
- Use `tempfile::tempdir()`. Never read or write the real config folder (`Vaults::load`, `settings::save`) and
  never bind a fixed port: bind port 0.
- Tests that need the system credential store are `#[ignore]`.

## Before saying it is done

Run, for each crate touched (Windows: `scripts/clippy.ps1`; elsewhere `scripts/clippy`):

```sh
cargo fmt --all
scripts/clippy -p <crate>      # warnings are errors
cargo test -p <crate>
```

Report the real output. A change that touches a public function also needs its callers compiled:
`cargo check --workspace`. Do not commit, push or add attribution lines unless asked.
