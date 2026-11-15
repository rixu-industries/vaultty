# Contributing to Vaultty

Thanks for helping. Open an issue before starting anything large to agree on the approach.

## AI-generated contributions

Using AI tools (code assistants, chatbots, agents) is allowed. You remain the author of the change: a human must review every line you submit, and you must understand the code and how it works. Do not open a pull request with generated code you have not read or cannot explain.

## Before submitting a change

```sh
just check          # formatting, clippy (scripts/clippy, warnings are errors) and tests
```

CI (`.github/workflows/run_tests.yml`) also checks:

- typos and TODO markers;
- clippy and tests on Linux, Windows and macOS;
- unused dependencies (cargo-shear), that `Cargo.lock` is up to date, and licenses (cargo-deny);
- workflow and script lint (actionlint, zizmor, shellcheck).

## Structure

- Logic goes in the crates (`vaults`, `sftp`, `sessions`, `terminal`, `ssh`, `store`, `core`, `mcp`…);
  `crates/desktop` is only the UI.
- `gpui-kit` is pinned to an exact version. Do not bump it without testing the app by hand.

## Style

- Language: what the user sees (UI, errors that can appear on screen, MCP server descriptions) goes in
  **English**. Comments, internal documentation and test names also go in **English**.
- Short comments that explain the why, not what the code already says.
- In the UI, sizes go through `crate::app::theme::px`, which scales with the UI size set in Settings.
- Blank lines between blocks (`if`, `for`, `match`); `rustfmt` does not add them on its own.

## When to abstract

Create a new function, component or option only if it pins down a decision that must be the same in several places,
names a large block, or hides something complicated (state, closures, focus, async…). Do not create one for a single
short use or "just in case": abstract it on the second or third use.

In `crates/desktop/src/components`, `ui/` holds generic primitives used on several screens; anything used by a single
screen lives next to it. Always import from `crate::components::X`.

## Contributor License Agreement

Before we can merge your first pull request you must sign the
[Vaultty Contributor License Agreement](https://cla-assistant.io/rixu-industries/vaultty). The CLA assistant bot comments on
your pull request with a link; signing takes one click with your GitHub account and covers all your future
contributions.

The CLA lets us distribute the project under GPL-3.0-or-later today and change its license in the future, including
under other open source or proprietary terms. You keep the copyright in your contributions.
