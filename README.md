# Vaultty

[![CLA assistant](https://cla-assistant.io/readme/badge/rixu-industries/vaultty)](https://cla-assistant.io/rixu-industries/vaultty)

Desktop application for SSH connections with an encrypted vault: hosts, keys, tunnels, terminal and SFTP in one place.

## Features

- **Encrypted vaults** protected by a master password. Secrets (passwords and private keys) are never stored in plain text.
- **Hosts and groups**, with several access methods per host (password or key).
- **SSH key ring**: import or generate keys and share them between hosts.
- **Terminal** with tabs and split panes.
- **SFTP** with a transfer queue.
- **Tunnels**: port forwarding, SOCKS4/5 and HTTP proxy.
- **Optional MCP server**, so an MCP client can query and manage your hosts.
- **Themes**: several built in, plus import of VS Code themes.
- Signed updates.

## Installation

Download it from the [latest release](https://github.com/rixu-industries/vaultty/releases/latest). Every package below updates itself:

- **Windows:** `vaultty_<version>_x64-setup.exe` or `_arm64-setup.exe`. It installs for the current user, without admin rights.
- **macOS:** `Vaultty_<version>_aarch64.dmg` (Apple Silicon) or `_x64.dmg` (Intel). The app is not notarized yet, so the first time macOS blocks it: open it, then allow it under System Settings → Privacy & Security.
- **Linux:** `vaultty_<version>_x86_64.AppImage` or `_aarch64.AppImage`. Make it executable (`chmod +x`) and run it.

A build from source or a distro package does not update itself.

## Build from source

You need a recent stable Rust toolchain (edition 2024).

```sh
cargo run -p desktop
```

With [`just`](https://github.com/casey/just) installed, `just` lists the available recipes (`run`, `test`, `check`, `package`…).

## Releases

Releases are automatic. Every Monday the **Release** workflow takes the newest commit of `main` that passed CI and, if the app changed since the last release (`crates/`, `Cargo.toml` or `Cargo.lock`), publishes it with the patch version bumped. It can also be run by hand from GitHub Actions to ship a fix right away.

- **Version:** the higher of the `Cargo.toml` version and the last `v*` tag with the patch bumped. To ship a minor or major, raise the version in `Cargo.toml`; nothing else changes. The bot never commits to `main`: the tag is the source of truth.
- **Contents:** for x86-64 and ARM64, the NSIS installer on Windows, the `.dmg` and the `.app.tar.gz` the updater uses on macOS, and the AppImage on Linux. Each comes with its updater signature (`.sig`), plus the `latest.json` that lists them for the updater. Notes are generated from the merged PRs.
- **Safety:** the release is built as a draft and only published once everything is uploaded, so the updater never sees a partial release.

It needs the `CARGO_PACKAGER_SIGN_PRIVATE_KEY` secret with the contents of the private key that matches `crates/updater/updater.key.pub` and, if the key has a password, `CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`. The updater signature is not an operating system signature: Windows SmartScreen and macOS Gatekeeper still warn on first install.

## Structure

The logic lives in independent crates (`core`, `vaults`, `sessions`, `ssh`, `sftp`, `terminal`, `proxy`, `store`, `settings`, `themes`, `mcp`, `updater`); `crates/desktop` is only the UI.

## Privacy

Vaultty sends one anonymous message a day to count installations in use: a random identifier, the version and the operating system, and nothing about you or your vaults. Turn it off in the Windows installer or in Settings → General. The details are in [PRIVACY.md](PRIVACY.md).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). AI tools are allowed if a human has reviewed the generated code and understands it. To report a vulnerability, see [SECURITY.md](SECURITY.md).

## License

Vaultty is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, version 3 or (at your option) any later version. It is distributed WITHOUT ANY WARRANTY. See the [LICENSE](LICENSE) file for more details.

Copyright (C) 2026 José García "Ruxwez" ([Rixu Industries](https://rixu.dev)).
