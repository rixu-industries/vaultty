---
name: vaultty-security-rules
description: Use when a change touches secrets, the vault format, host keys, the MCP server or its permissions, tunnels and proxies, the updater, or anything that parses data from a remote server or an imported file - in crates core, store, vaults, ssh, sessions, proxy, mcp, themes or updater.
---

# Vaultty security rules

Vaultty stores credentials. These invariants hold after every change; a change that breaks one is wrong even
if its tests pass. State in the report which ones the change touches.

## Secrets

- Passwords, private keys and passphrases live only in `core::model` types (zeroized on drop, redacted
  `Debug`). Never copy one into a `String` that outlives the call, a log, an error message, an `Event` or
  a view (`VaultView`, `HostView`, `MethodView`, `KeyView`).
- Secrets leave `vaults` only through `target`, `method` and `key`, to open a connection or fill an edit form.
- Nothing secret goes to `settings.json` or `vaults.json`. Remembered passwords and the MCP token go to the
  system credential store.

## Vault format

- Every record is sealed with its vault id, record id, revision and format version as associated data. Do
  not add a field outside the ciphertext that changes behaviour.
- Header values reach Argon2 before being authenticated: keep the caps.
- A new record kind, access kind or tunnel kind is added to the `KNOWN_*` list so older apps report "newer
  version", not "corrupted". Never change the meaning of an existing field; add one with `#[serde(default)]`.

## Host keys

- A server key is never accepted without the user: `ssh::connect` fails with `UnknownHostKey` and only the
  app's prompt may call `trust_host_key`. The MCP and tunnels never accept one.
- Changing a host's address or port drops its saved fingerprint.

## MCP server

- Permissions are checked on every call, at call time, with the host's own permissions over the vault's.
- A tool never returns a secret, whatever its permissions.
- A tool never changes where stored credentials are sent: it cannot change the address or port of a host
  that has access methods, nor add, edit or pick credentials. Those belong to the user, in the app.
- Errors do not reveal hosts, groups or vaults the agent cannot read.
- Requests need the Bearer token (constant-time compare) and an allowed `Host`. Listening beyond loopback is
  refused or warned about: the token travels in plain HTTP.
- Each new tool gets a test that it refuses without the permission it needs.

## Untrusted input

Treat as hostile: everything a remote server sends (file names, sizes, terminal sequences, command output),
imported files (`.vsix`, theme packages, key files) and proxy clients.

- Cap what is read (`take(n)`, explicit maximums); never trust a declared size.
- A name from the other side is a single path component: no separators, `..`, drive or root.
- A terminal sequence may not act on the user's machine (clipboard, files, links) without a setting or a
  confirmation.
- Proxies listen on loopback by default; binding elsewhere shows a warning.

## Updates and CI

- Only packages signed with `crates/updater/updater.key.pub` are installed. Never add an unsigned path.
- Workflow actions stay pinned to a commit SHA, with least `permissions`.

## Red flags: stop and ask the user

- Weakening or skipping a check "to make the test pass".
- A new place where a secret is cloned, formatted or sent.
- A new network listener, a new tool, or a wider default permission.
- Changing the on-disk format in a way older versions cannot read.
