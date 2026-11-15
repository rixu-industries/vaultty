# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

# Common Vaultty commands. See the list with `just` or `just --list`.
# On Windows, use PowerShell so it does not depend on sh (Git Bash) being in the PATH.
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# Lists the available recipes
default:
    @just --list

# Runs the desktop application
run:
    cargo run -p desktop

# Builds the release binary (target/release/vaultty)
release:
    cargo build -p desktop --release

# Builds the installer in target/installer (unsigned; releases are published by the Release workflow)
[working-directory: 'crates/desktop']
package:
    cargo packager --release {{ if os() == "macos" { "--formats dmg" } else { "" } }}

# Tests. Without arguments: the whole workspace. Example: `just test -p core`
test *args:
    cargo test {{args}}

# Clippy with warnings as errors, as in CI. Also checks unused dependencies and typos when those tools are installed
clippy:
    {{ if os() == "windows" { "powershell.exe -NoProfile -ExecutionPolicy Bypass -File ./scripts/clippy.ps1" } else { "./scripts/clippy" } }}

# Regenerates the licenses shown under Help → Open Source Licenses (when changing dependencies or themes)
licenses:
    cargo run -p licenses

# Formats the code with rustfmt
fmt:
    cargo fmt --all

# Checks formatting without modifying files
fmt-check:
    cargo fmt --all --check

# Checks before a change is accepted: formatting, clippy and tests
check: fmt-check clippy test
