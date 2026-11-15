---
name: vaultty-desktop-ui
description: Use when adding or changing anything under crates/desktop (GPUI views, forms, dialogs, tabs, panes, settings screens, AppState) - covers threading, state refresh, components and sizing.
---

# Vaultty desktop UI

`crates/desktop` only paints and forwards. `gpui-kit` is pinned to an exact version: do not bump it, and
copy the patterns of a neighbouring view instead of guessing its API.

## Threads

- The UI thread never waits. Anything that touches disk, the network, the credential store or Argon2 runs in
  `cx.background_executor().spawn(...)`; the result is applied with `this.update(cx, …)`. Model:
  `AppState::auto_unlock`.
- SSH and SFTP work runs on the tokio runtime through `sessions.spawn(...)`, never on the GPUI executor.
- What arrives from `sessions` comes as an `Event` and is handled in `AppState::on_event`.

## State

- The source of truth is `vaults` and `sessions`. `AppState` holds copies for painting (`list`, `views`,
  `session_list`).
- Change a vault through `AppState::change(vault, |v| …, cx)`: it reports the error to the status bar and
  re-reads the view (which also syncs tunnels). Do not call `vaults.*` and forget to refresh.
- Errors go to `report`/`set_error`; a form shows its own error next to the field.
- Call `cx.notify()` only when something painted changed.

## Components and layout

- `components/ui/` holds generic primitives used by several screens; anything used by one screen lives next
  to that screen. Import from `crate::components::X`.
- Sizes go through `crate::app::theme::px`, which scales with the UI size from Settings. No raw pixel values.
- Colors come from the theme; no literals.
- Text a user must compare or copy in full (fingerprints, tokens, paths) wraps; it is not truncated.
- A destructive action goes through `confirm(...)` and says what is and is not deleted.
- A setting that exposes something beyond this computer (listen address, permissions) shows a warning next to
  the field.

## Checks

`cargo check -p desktop` and `scripts/clippy -p desktop`. There are no UI tests: say in the report what was
not exercised by hand, and never claim a screen works without having run the app.
