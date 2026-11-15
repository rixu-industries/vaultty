// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The MCP tools. Each call checks permissions at that moment: the host's own if it has any and, if not,
//! the vault's in the settings. What is not part of a host (keys, groups, creating a new host)
//! uses the vault's.
//!
//! Everything the agent sees is in English: descriptions, errors, and the `///` of parameters and responses,
//! which appear in the JSON schema. Hosts, vaults, groups and methods are accepted by name or id, so that
//! the agent does not have to chain calls just to get ids.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::{self, JsonSchema};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use sessions::Sessions;
use uuid::Uuid;
use vaults::{Exec, HostInput, HostView, McpPerms, MethodView, VaultView};

use crate::jobs::{Job, Jobs, State};
use crate::settings::VaultAccess;

/// What the user is asked to approve before a command runs.
pub struct ExecRequest {
    pub vault: String,
    /// Host name and address.
    pub host: String,
    pub command: String,
}

/// Asks the user; `true` if they accept. If there is no answer within `ASK_TIMEOUT`, it counts as a no.
pub type Ask = Arc<dyn Fn(ExecRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

const ASK_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// In the foreground the client waits, so anything longer makes no sense.
const MAX_FOREGROUND: Duration = Duration::from_secs(600);
const MAX_BACKGROUND: Duration = Duration::from_secs(3600);
/// How long `get_command` waits, at most, for a command to finish before answering.
const MAX_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct Tools {
    pub sessions: Arc<Sessions>,
    pub access: Arc<Mutex<Vec<VaultAccess>>>,
    pub ask: Ask,
    pub jobs: Arc<Jobs>,
}

/// What applies to a host: its own permissions or, if it has none, the vault's.
fn effective(vault: McpPerms, host: &HostView) -> McpPerms {
    host.mcp.unwrap_or(vault)
}

/// A host with any permission is visible to the agent; without one, it cannot be found.
fn usable(perms: McpPerms) -> bool {
    perms.read || perms.write || perms.exec != Exec::No
}

/// By id or name, ignoring case.
fn is(query: &str, id: Uuid, name: &str) -> bool {
    let query = query.trim();
    query.eq_ignore_ascii_case(&id.to_string())
        || query.to_lowercase() == name.trim().to_lowercase()
}

/// A vault the agent can use now: it is in the settings and unlocked.
struct Open {
    id: Uuid,
    name: String,
    perms: McpPerms,
    view: VaultView,
}

// ---- Parameters ----

#[derive(Deserialize, JsonSchema)]
struct VaultFilter {
    /// Vault name or id. Omit to use every vault you have access to.
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct HostArg {
    /// Host name, address or id.
    host: String,
    /// Vault name or id. Only needed if the host name exists in several vaults.
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SaveHostArg {
    /// Host name, address or id to edit. Omit to create a new host.
    host: Option<String>,
    /// Vault name or id. Needed to create a host when you have access to several vaults.
    vault: Option<String>,
    /// Required to create. When editing, omitted fields keep their current value.
    name: Option<String>,
    /// Hostname or IP. Required to create.
    address: Option<String>,
    /// SSH port. Defaults to 22 when creating.
    port: Option<u16>,
    /// Group name or id. Use "" to remove the host from its group.
    group: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SaveGroupArg {
    /// New name for the group.
    name: String,
    /// Group name or id to rename. Omit to create a new group.
    group: Option<String>,
    /// Vault name or id. Needed when you have access to several vaults.
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct RunArg {
    /// Host name, address or id.
    host: String,
    /// Shell command to run (no TTY, non-interactive).
    command: String,
    /// Vault name or id. Only needed if the host name exists in several vaults.
    vault: Option<String>,
    /// Access method name, username or id. Only needed if the host has several.
    method: Option<String>,
    /// Seconds before the command is killed. Foreground: default 120, max 600. Background: default
    /// and max 3600.
    timeout_secs: Option<u64>,
    /// true: return immediately with a job_id; poll it with get_command.
    background: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct JobArg {
    /// The job_id returned by run_command.
    job_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct GetArg {
    /// The job_id returned by run_command.
    job_id: String,
    /// Wait up to this many seconds (max 60) for the command to finish before answering. Use it
    /// instead of calling get_command repeatedly.
    wait_secs: Option<u64>,
}

// ---- Responses ----

#[derive(Serialize, JsonSchema)]
struct VaultList {
    vaults: Vec<VaultOut>,
}

#[derive(Serialize, JsonSchema)]
struct VaultOut {
    id: String,
    name: String,
    /// Locked vaults can only be unlocked by the user, in the app.
    unlocked: bool,
    /// Default permissions for its hosts; a host may have its own (see list_hosts).
    permissions: PermsOut,
}

#[derive(Serialize, JsonSchema)]
struct PermsOut {
    read: bool,
    write: bool,
    /// "no", "ask" (the user approves each command in the app) or "direct".
    exec: String,
}

impl From<McpPerms> for PermsOut {
    fn from(p: McpPerms) -> Self {
        Self {
            read: p.read,
            write: p.write,
            exec: match p.exec {
                Exec::No => "no",
                Exec::Ask => "ask",
                Exec::Direct => "direct",
            }
            .into(),
        }
    }
}

#[derive(Serialize, JsonSchema)]
struct HostList {
    hosts: Vec<HostOut>,
    /// Groups of the vaults you can read.
    groups: Vec<GroupOut>,
}

#[derive(Serialize, JsonSchema)]
struct GroupOut {
    id: String,
    name: String,
    vault: String,
}

#[derive(Serialize, JsonSchema)]
struct HostOut {
    id: String,
    name: String,
    address: String,
    port: u16,
    vault: String,
    group: Option<String>,
    notes: String,
    /// Detected on the first connection (ubuntu, debian, macos, windows...).
    os: Option<String>,
    methods: Vec<MethodOut>,
    permissions: PermsOut,
}

#[derive(Serialize, JsonSchema)]
struct MethodOut {
    id: String,
    name: String,
    username: String,
    /// "password" or "key".
    auth: String,
}

#[derive(Serialize, JsonSchema)]
struct KeyList {
    keys: Vec<KeyOut>,
}

#[derive(Serialize, JsonSchema)]
struct KeyOut {
    id: String,
    name: String,
    vault: String,
    algorithm: String,
    fingerprint: String,
    public_key: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct Saved {
    id: String,
    name: String,
    vault: String,
}

#[derive(Serialize, JsonSchema)]
struct RunOut {
    job_id: String,
    /// "running", "done", "timed_out" (killed after timeout_secs), "stopped" (by stop_command or
    /// because the server stopped) or "failed" (see `error`).
    state: String,
    error: Option<String>,
    stdout: String,
    stderr: String,
    /// Missing while running, or if the command was killed.
    exit_status: Option<u32>,
    /// An output exceeded 64 KiB and only its last 64 KiB are kept.
    truncated: bool,
}

// ---- Finding vaults, hosts and permissions ----

impl Tools {
    /// The usable vaults that match `vault` (or all of them), and the names of the locked ones,
    /// to explain why something cannot be found.
    fn vaults(&self, vault: Option<&str>) -> Result<(Vec<Open>, Vec<String>), String> {
        let access = self.access.lock().unwrap().clone();
        let list = self.sessions.vaults().list();
        let (mut open, mut locked) = (Vec::new(), Vec::new());

        for a in access {
            let Some(item) = list.iter().find(|v| v.id == a.id) else {
                continue;
            };

            if vault.is_some_and(|q| !is(q, item.id, &item.name)) {
                continue;
            }

            match self.sessions.vaults().view(item.id) {
                Ok(view) if item.unlocked => open.push(Open {
                    id: item.id,
                    name: item.name.clone(),
                    perms: a.perms,
                    view,
                }),
                _ => locked.push(item.name.clone()),
            }
        }

        if open.is_empty() {
            return Err(match (vault, locked.is_empty()) {
                (_, false) => format!(
                    "Vault {} is locked. Ask the user to unlock it in the Vaultty app.",
                    locked.join(", ")
                ),
                (Some(q), true) => {
                    format!("No vault matches '{q}'. Call list_vaults to see the available vaults.")
                }
                (None, true) => {
                    "You have no access to any vault. Ask the user to add one in Vaultty: \
                    Settings > MCP server."
                        .into()
                }
            });
        }

        Ok((open, locked))
    }

    /// A single vault, for creating something: the one given or, if none is given, the only one.
    fn one_vault(&self, vault: Option<&str>) -> Result<Open, String> {
        let (mut open, _) = self.vaults(vault)?;

        if open.len() > 1 {
            let names: Vec<_> = open.iter().map(|v| v.name.as_str()).collect();
            return Err(format!(
                "You have access to several vaults ({}). Pass `vault`.",
                names.join(", ")
            ));
        }

        Ok(open.remove(0))
    }

    /// The host by name, address or id, with its effective permissions.
    fn find_host(
        &self,
        vault: Option<&str>,
        query: &str,
    ) -> Result<(Open, HostView, McpPerms), String> {
        let (open, locked) = self.vaults(vault)?;
        let mut found = Vec::new();

        for (i, v) in open.iter().enumerate() {
            for h in &v.view.hosts {
                let perms = effective(v.perms, h);
                let hit = is(query, h.id, &h.name) || query.trim() == h.address;

                if hit && usable(perms) {
                    found.push((i, h.clone(), perms));
                }
            }
        }

        match found.len() {
            1 => {
                let (i, host, perms) = found.remove(0);
                let vault = open.into_iter().nth(i).expect("index from this same list");

                Ok((vault, host, perms))
            }
            0 => {
                let mut error = format!(
                    "No host matches '{query}'. Call list_hosts to see the hosts you can use."
                );

                if !locked.is_empty() {
                    error += &format!(
                        " Vault {} is locked, so its hosts are not visible; the user can unlock it in the app.",
                        locked.join(", ")
                    );
                }

                Err(error)
            }
            _ => {
                let options: Vec<_> = found
                    .iter()
                    .map(|(i, h, _)| {
                        format!(
                            "{} ({}, vault {}, id {})",
                            h.name, h.address, open[*i].name, h.id
                        )
                    })
                    .collect();

                Err(format!(
                    "'{query}' matches several hosts: {}. Pass the host id, or `vault`.",
                    options.join("; ")
                ))
            }
        }
    }

    /// The group by name or id within a vault.
    fn find_group(view: &VaultView, query: &str) -> Result<Uuid, String> {
        view.groups
            .iter()
            .find(|g| is(query, g.id, &g.name))
            .map(|g| g.id)
            .ok_or_else(|| {
                let names: Vec<_> = view.groups.iter().map(|g| g.name.as_str()).collect();
                format!(
                    "No group matches '{query}'. Groups in this vault: {}.",
                    if names.is_empty() {
                        "none".into()
                    } else {
                        names.join(", ")
                    }
                )
            })
    }

    /// Notifies the UI: otherwise it would not see what the MCP changed.
    fn changed(&self, vault: Uuid) {
        self.sessions.vault_changed(vault);
    }

    /// The job, if the MCP can still run on its host: removing the permission also cuts off access
    /// to jobs already started.
    fn job(&self, job_id: &str) -> Result<Arc<Job>, String> {
        let job = job_id
            .trim()
            .parse()
            .ok()
            .and_then(|id| self.jobs.get(id))
            .ok_or("Unknown job_id. Jobs are forgotten when the app closes, and only the last 50 are kept.")?;

        let (_, _, perms) = self.find_host(Some(&job.vault.to_string()), &job.host.to_string())?;

        if perms.exec == Exec::No {
            return Err("You are no longer allowed to run commands on that host.".into());
        }

        Ok(job)
    }
}

fn method_of(host: &HostView, query: Option<&str>) -> Result<Uuid, String> {
    let describe = |m: &MethodView| {
        let auth = if m.password { "password" } else { "key" };
        format!("{} ({}, {auth}, id {})", m.title, m.username, m.id)
    };

    let all = || {
        host.methods
            .iter()
            .map(describe)
            .collect::<Vec<_>>()
            .join("; ")
    };

    match (query, host.methods.as_slice()) {
        (_, []) => Err(format!(
            "Host {} has no access methods. The user has to add one in the app.",
            host.name
        )),
        (None, [only]) => Ok(only.id),
        (None, _) => Err(format!(
            "Host {} has several access methods: {}. Pass `method`.",
            host.name,
            all()
        )),
        (Some(q), methods) => methods
            .iter()
            .find(|m| is(q, m.id, &m.title) || q.trim() == m.username)
            .map(|m| m.id)
            .ok_or_else(|| {
                format!(
                    "No access method matches '{q}'. Methods of {}: {}.",
                    host.name,
                    all()
                )
            }),
    }
}

/// Saved credentials go wherever the host points: an agent that moved it would send them, on the
/// user's next connection, to a server of its choosing.
fn moves_credentials(host: &HostView, arg: &SaveHostArg) -> bool {
    let address = arg
        .address
        .as_deref()
        .is_some_and(|a| a.trim() != host.address);
    let port = arg.port.is_some_and(|p| p != host.port);

    !host.methods.is_empty() && (address || port)
}

const MOVES_CREDENTIALS: &str = "This host has saved credentials, so only the user can change its \
    address or port, in the Vaultty app. Its other fields can be edited here.";
const NO_READ: &str = "You do not have read permission here. The user can change it in Vaultty: \
    Settings > MCP server.";
const NO_WRITE: &str = "You do not have write permission here. The user can change it in Vaultty: \
    Settings > MCP server.";

// ---- Herramientas ----

// The first thing the agent reads: how to use the server. The macro only accepts a literal.
#[tool_handler(
    name = "vaultty",
    instructions = "Vaultty manages SSH servers (hosts) stored in encrypted vaults. Refer to vaults, hosts, groups and access methods by name or id; `vault` is only needed when a name is ambiguous. Typical flow: list_hosts, then run_command(host, command). Only vaults that the user added in Vaultty Settings and unlocked in the app are visible, with the permissions shown by list_vaults and list_hosts. Depending on those permissions, the user may have to approve each command in the app. Passwords and private keys are never returned."
)]
impl ServerHandler for Tools {}

#[tool_router]
impl Tools {
    #[tool(
        description = "List the vaults you have access to, whether each one is unlocked, and its default permissions.",
        annotations(read_only_hint = true)
    )]
    fn list_vaults(&self) -> Json<VaultList> {
        let access = self.access.lock().unwrap().clone();
        let list = self.sessions.vaults().list();

        Json(VaultList {
            vaults: access
                .iter()
                .filter_map(|a| {
                    let v = list.iter().find(|v| v.id == a.id)?;

                    Some(VaultOut {
                        id: v.id.to_string(),
                        name: v.name.clone(),
                        unlocked: v.unlocked,
                        permissions: a.perms.into(),
                    })
                })
                .collect(),
        })
    }

    #[tool(
        description = "List the SSH hosts you can read, with their vault, group, access methods (no secrets) and your permissions on each one. Also lists the groups.",
        annotations(read_only_hint = true)
    )]
    fn list_hosts(
        &self,
        Parameters(VaultFilter { vault }): Parameters<VaultFilter>,
    ) -> Result<Json<HostList>, String> {
        let (open, _) = self.vaults(vault.as_deref())?;
        let (mut hosts, mut groups) = (Vec::new(), Vec::new());

        for v in &open {
            hosts.extend(readable_hosts(v));

            if v.perms.read {
                groups.extend(v.view.groups.iter().map(|g| GroupOut {
                    id: g.id.to_string(),
                    name: g.name.clone(),
                    vault: v.name.clone(),
                }));
            }
        }

        Ok(Json(HostList { hosts, groups }))
    }

    #[tool(
        description = "List the SSH keys stored in the vaults: name, algorithm, fingerprint and public key. Private keys are never returned.",
        annotations(read_only_hint = true)
    )]
    fn list_keys(
        &self,
        Parameters(VaultFilter { vault }): Parameters<VaultFilter>,
    ) -> Result<Json<KeyList>, String> {
        let (open, _) = self.vaults(vault.as_deref())?;
        let readable: Vec<_> = open.into_iter().filter(|v| v.perms.read).collect();

        if readable.is_empty() {
            return Err(NO_READ.into());
        }

        Ok(Json(KeyList {
            keys: readable
                .into_iter()
                .flat_map(|v| {
                    let vault = v.name.clone();

                    v.view.keys.into_iter().map(move |k| KeyOut {
                        id: k.id.to_string(),
                        name: k.name,
                        vault: vault.clone(),
                        algorithm: k.algorithm,
                        fingerprint: k.fingerprint,
                        public_key: k.public,
                    })
                })
                .collect(),
        }))
    }

    #[tool(
        description = "Create a host (without `host`) or edit one (with `host`). Access methods (credentials) cannot be set here: the user adds them in the app. Once a host has access methods, only the user can change its address or port.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    fn save_host(&self, Parameters(arg): Parameters<SaveHostArg>) -> Result<Json<Saved>, String> {
        let (vault, existing, perms) = match &arg.host {
            Some(q) => {
                let (v, h, p) = self.find_host(arg.vault.as_deref(), q)?;
                (v, Some(h), p)
            }
            None => {
                let v = self.one_vault(arg.vault.as_deref())?;
                let p = v.perms;
                (v, None, p)
            }
        };

        if !perms.write {
            return Err(NO_WRITE.into());
        }

        if existing
            .as_ref()
            .is_some_and(|h| moves_credentials(h, &arg))
        {
            return Err(MOVES_CREDENTIALS.into());
        }

        let group_id = match arg.group.as_deref() {
            None => existing.as_ref().and_then(|h| h.group_id),
            Some(q) if q.trim().is_empty() => None,
            Some(q) => Some(Self::find_group(&vault.view, q)?),
        };

        let pick = |new: Option<String>, old: Option<&String>| {
            new.or_else(|| old.cloned())
                .unwrap_or_default()
                .trim()
                .to_string()
        };

        let input = HostInput {
            name: pick(arg.name, existing.as_ref().map(|h| &h.name)),
            address: pick(arg.address, existing.as_ref().map(|h| &h.address)),
            port: arg.port.or(existing.as_ref().map(|h| h.port)).unwrap_or(22),
            notes: pick(arg.notes, existing.as_ref().map(|h| &h.notes)),
            group_id,
        };

        if input.name.is_empty() || input.address.is_empty() || input.port == 0 {
            return Err("`name`, `address` and a non-zero `port` are required.".into());
        }

        let name = input.name.clone();
        let id = self
            .sessions
            .vaults()
            .save_host(vault.id, existing.map(|h| h.id), input)
            .map_err(|e| format!("Could not save the host: {e}"))?;
        self.changed(vault.id);

        Ok(Json(Saved {
            id: id.to_string(),
            name,
            vault: vault.name,
        }))
    }

    #[tool(
        description = "Delete a host from its vault and close the app tabs connected to it. This cannot be undone.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    fn delete_host(&self, Parameters(arg): Parameters<HostArg>) -> Result<Json<Saved>, String> {
        let (vault, host, perms) = self.find_host(arg.vault.as_deref(), &arg.host)?;

        if !perms.write {
            return Err(NO_WRITE.into());
        }

        self.sessions
            .vaults()
            .delete(vault.id, host.id)
            .map_err(|e| format!("Could not delete the host: {e}"))?;
        self.sessions.close_host(host.id);
        self.changed(vault.id);

        Ok(Json(Saved {
            id: host.id.to_string(),
            name: host.name,
            vault: vault.name,
        }))
    }

    #[tool(
        description = "Create a host group (without `group`) or rename one (with `group`).",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    fn save_group(&self, Parameters(arg): Parameters<SaveGroupArg>) -> Result<Json<Saved>, String> {
        let vault = self.one_vault(arg.vault.as_deref())?;

        if !vault.perms.write {
            return Err(NO_WRITE.into());
        }

        let id = match arg.group.as_deref() {
            Some(q) => Some(Self::find_group(&vault.view, q)?),
            None => None,
        };

        if arg.name.trim().is_empty() {
            return Err("`name` cannot be empty.".into());
        }

        let id = self
            .sessions
            .vaults()
            .save_group(vault.id, id, &arg.name)
            .map_err(|e| format!("Could not save the group: {e}"))?;
        self.changed(vault.id);

        Ok(Json(Saved {
            id: id.to_string(),
            name: arg.name.trim().to_string(),
            vault: vault.name,
        }))
    }

    #[tool(
        description = "Run a shell command on a host over SSH (no TTY, non-interactive) and return its output. Depending on your permissions the user may have to approve it in the app first; if they refuse or do not answer within 60 s, it fails. Foreground (default) waits for the command: timeout_secs defaults to 120, max 600. For long tasks use background=true, which returns a job_id at once (max 1 hour); poll it with get_command. A command that exceeds its timeout is killed on the server (state timed_out) and you get the output so far. Only the last 64 KiB of each output are kept.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    async fn run_command(
        &self,
        Parameters(arg): Parameters<RunArg>,
    ) -> Result<Json<RunOut>, String> {
        let (vault, host, perms) = self.find_host(arg.vault.as_deref(), &arg.host)?;
        let method = method_of(&host, arg.method.as_deref())?;

        match perms.exec {
            Exec::No => {
                return Err(format!(
                    "You are not allowed to run commands on {}. The user can change it in Vaultty: Settings > MCP server.",
                    host.name
                ));
            }
            Exec::Direct => {}
            Exec::Ask => {
                let request = ExecRequest {
                    vault: vault.name.clone(),
                    host: format!("{} ({})", host.name, host.address),
                    command: arg.command.clone(),
                };

                let accepted = tokio::time::timeout(ASK_TIMEOUT, (self.ask)(request))
                    .await
                    .unwrap_or(false);

                if !accepted {
                    return Err("The user did not approve the command (refused, or no answer within 60 s). Do not retry it unless they ask you to.".into());
                }
            }
        }

        let background = arg.background.unwrap_or(false);
        let (default, max) = if background {
            (MAX_BACKGROUND, MAX_BACKGROUND)
        } else {
            (DEFAULT_TIMEOUT, MAX_FOREGROUND)
        };

        let timeout = arg
            .timeout_secs
            .map_or(default, |s| Duration::from_secs(s.max(1)))
            .min(max);

        let job = self
            .jobs
            .new_job(vault.id, host.id, method, arg.command, timeout);

        if background {
            self.jobs.keep(job.clone());

            let sessions = self.sessions.clone();
            let running = job.clone();
            tokio::spawn(async move { running.run(&sessions).await });
        } else {
            job.run(&self.sessions).await;
        }

        Ok(Json(job_out(&job)))
    }

    #[tool(
        description = "State and output so far of a command started with run_command(background=true). Pass wait_secs (max 60) to wait for it to finish instead of polling.",
        annotations(read_only_hint = true)
    )]
    async fn get_command(
        &self,
        Parameters(arg): Parameters<GetArg>,
    ) -> Result<Json<RunOut>, String> {
        let job = self.job(&arg.job_id)?;
        let wait = Duration::from_secs(arg.wait_secs.unwrap_or(0)).min(MAX_WAIT);

        job.wait(wait).await;

        Ok(Json(job_out(&job)))
    }

    #[tool(
        description = "Stop a background command: its process is killed on the server. Returns the output so far.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true
        )
    )]
    async fn stop_command(
        &self,
        Parameters(arg): Parameters<JobArg>,
    ) -> Result<Json<RunOut>, String> {
        let job = self.job(&arg.job_id)?;
        job.stop.cancel();

        // Give the KILL a moment to arrive and the final state to settle.
        job.wait(Duration::from_secs(2)).await;

        Ok(Json(job_out(&job)))
    }
}

fn job_out(job: &Job) -> RunOut {
    let out = job.out.lock().unwrap().clone();

    let (state, error) = match job.state() {
        State::Running => ("running", None),
        State::Done => ("done", None),
        State::TimedOut => ("timed_out", None),
        State::Stopped => ("stopped", None),
        State::Failed(e) => ("failed", Some(e)),
    };

    RunOut {
        job_id: job.id.to_string(),
        state: state.into(),
        error,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        exit_status: out.status,
        truncated: out.truncated,
    }
}

/// The hosts with read permission, whether their own or inherited from the vault.
fn readable_hosts(vault: &Open) -> Vec<HostOut> {
    let view = &vault.view;

    view.hosts
        .iter()
        .filter_map(|h| {
            let perms = effective(vault.perms, h);

            if !perms.read {
                return None;
            }

            Some(HostOut {
                id: h.id.to_string(),
                name: h.name.clone(),
                address: h.address.clone(),
                port: h.port,
                vault: vault.name.clone(),
                group: h
                    .group_id
                    .and_then(|g| view.groups.iter().find(|x| x.id == g))
                    .map(|g| g.name.clone()),
                notes: h.notes.clone(),
                os: h.os.clone(),
                methods: h
                    .methods
                    .iter()
                    .map(|m| MethodOut {
                        id: m.id.to_string(),
                        name: m.title.clone(),
                        username: m.username.clone(),
                        auth: if m.password { "password" } else { "key" }.into(),
                    })
                    .collect(),
                permissions: perms.into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str, mcp: Option<McpPerms>) -> HostView {
        HostView {
            host_key: None,
            id: Uuid::new_v4(),
            name: name.into(),
            address: "10.0.0.1".into(),
            port: 22,
            group_id: None,
            notes: String::new(),
            os: None,
            methods: vec![],
            mcp,
        }
    }

    const READ: McpPerms = McpPerms {
        read: true,
        write: false,
        exec: Exec::No,
    };

    const NONE: McpPerms = McpPerms {
        read: false,
        write: false,
        exec: Exec::No,
    };

    const WRITE: McpPerms = McpPerms {
        read: true,
        write: true,
        exec: Exec::No,
    };

    /// The tools over a real vault in a temporary folder, with a host `web` that has a password.
    fn tools(perms: McpPerms) -> (tempfile::TempDir, Tools, Uuid, Uuid) {
        let tmp = tempfile::tempdir().unwrap();
        let vaults = vaults::Vaults::at(tmp.path().join("vaults.json"));
        let vault = vaults.create(tmp.path().join("Vault"), "pw").unwrap();

        let input = HostInput {
            name: "web".into(),
            address: "10.0.0.1".into(),
            port: 22,
            ..HostInput::default()
        };
        let host = vaults.save_host(vault, None, input).unwrap();

        let method = vaults::AccessMethod {
            id: Uuid::new_v4(),
            label: String::new(),
            kind: vaults::AccessKind::Password {
                username: "deploy".into(),
                password: "hunter2".into(),
            },
        };
        vaults.save_method(vault, host, method).unwrap();

        let sessions = Sessions::new(
            Arc::new(vaults),
            tokio::runtime::Handle::current(),
            Arc::new(|_| {}),
        );

        let tools = Tools {
            sessions,
            access: Arc::new(Mutex::new(vec![VaultAccess { id: vault, perms }])),
            ask: Arc::new(|_| Box::pin(async { false })),
            jobs: Arc::new(Jobs::new(tokio_util::sync::CancellationToken::new())),
        };

        (tmp, tools, vault, host)
    }

    fn edit(host: &str, address: Option<&str>, notes: Option<&str>) -> Parameters<SaveHostArg> {
        Parameters(SaveHostArg {
            host: Some(host.into()),
            vault: None,
            name: None,
            address: address.map(Into::into),
            port: None,
            group: None,
            notes: notes.map(Into::into),
        })
    }

    fn host_arg(host: &str) -> Parameters<HostArg> {
        Parameters(HostArg {
            host: host.into(),
            vault: None,
        })
    }

    fn run(host: &str) -> Parameters<RunArg> {
        Parameters(RunArg {
            host: host.into(),
            command: "id".into(),
            vault: None,
            method: None,
            timeout_secs: None,
            background: None,
        })
    }

    // Checks that a read-only agent cannot change, delete or run anything.
    #[tokio::test]
    async fn read_only_cannot_write_or_run() {
        let (_tmp, tools, vault, _) = tools(READ);

        assert!(
            tools
                .list_hosts(Parameters(VaultFilter { vault: None }))
                .is_ok()
        );
        assert_eq!(
            tools.save_host(edit("web", None, Some("x"))).err().unwrap(),
            NO_WRITE
        );
        assert_eq!(tools.delete_host(host_arg("web")).err().unwrap(), NO_WRITE);
        assert!(tools.run_command(run("web")).await.is_err());

        let view = tools.sessions.vaults().view(vault).unwrap();
        assert_eq!(view.hosts.len(), 1);
        assert_eq!(view.hosts[0].notes, "");
    }

    // Checks that a host whose own permissions deny everything cannot be found, whatever the vault allows.
    #[tokio::test]
    async fn host_without_permissions_is_invisible() {
        let (_tmp, tools, vault, host) = tools(WRITE);

        tools
            .sessions
            .vaults()
            .edit_host(vault, host, |h| h.mcp = Some(NONE))
            .unwrap();

        assert!(tools.save_host(edit("web", None, Some("x"))).is_err());
        assert!(tools.delete_host(host_arg("web")).is_err());
        assert!(tools.run_command(run("web")).await.is_err());
        assert_eq!(tools.sessions.vaults().view(vault).unwrap().hosts.len(), 1);
    }

    // Checks that an agent with write permission cannot point saved credentials at another server.
    #[tokio::test]
    async fn cannot_move_a_host_with_credentials() {
        let (_tmp, tools, vault, host) = tools(WRITE);
        let address = |tools: &Tools| {
            let view = tools.sessions.vaults().view(vault).unwrap();
            view.hosts
                .iter()
                .find(|h| h.id == host)
                .unwrap()
                .address
                .clone()
        };

        assert_eq!(
            tools
                .save_host(edit("web", Some("evil.example"), None))
                .err()
                .unwrap(),
            MOVES_CREDENTIALS
        );
        assert_eq!(address(&tools), "10.0.0.1");

        // Its other fields, and sending the same address again, are still allowed.
        assert!(
            tools
                .save_host(edit("web", Some("10.0.0.1"), Some("x")))
                .is_ok()
        );

        // Without credentials there is nothing to send elsewhere.
        let method = tools.sessions.vaults().view(vault).unwrap().hosts[0].methods[0].id;
        tools
            .sessions
            .vaults()
            .remove_method(vault, host, method)
            .unwrap();
        assert!(tools.save_host(edit("web", Some("10.0.0.2"), None)).is_ok());
        assert_eq!(address(&tools), "10.0.0.2");
    }

    #[test]
    fn the_host_overrides_the_vault() {
        let open = |perms| Open {
            id: Uuid::new_v4(),
            name: "v".into(),
            perms,
            view: VaultView {
                hosts: vec![
                    host("inherits", None),
                    host("hidden", Some(NONE)),
                    host("visible", Some(READ)),
                ],
                ..VaultView::default()
            },
        };

        let names = |perms| -> Vec<String> {
            readable_hosts(&open(perms))
                .into_iter()
                .map(|h| h.name)
                .collect()
        };

        assert_eq!(names(READ), ["inherits", "visible"]);
        assert_eq!(names(NONE), ["visible"]);
    }

    #[test]
    fn finds_by_id_or_name_ignoring_case() {
        let id = Uuid::new_v4();

        assert!(is(&id.to_string(), id, "web"));
        assert!(is(&id.to_string().to_uppercase(), id, "web"));
        assert!(is(" Web-1 ", id, "web-1"));
        assert!(is("RÉSUMÉ", id, "résumé"));
        assert!(!is("web", id, "web-1"));
    }

    #[test]
    fn picks_a_method_or_explains_which_ones_exist() {
        let method = |title: &str, user: &str| MethodView {
            id: Uuid::new_v4(),
            password: true,
            username: user.into(),
            label: title.into(),
            key_id: None,
            title: title.into(),
            detail: String::new(),
            broken: false,
        };

        let mut h = host("web", None);
        assert!(
            method_of(&h, None)
                .unwrap_err()
                .contains("no access methods")
        );

        h.methods = vec![method("deploy", "deploy")];
        assert_eq!(method_of(&h, None).unwrap(), h.methods[0].id);

        h.methods.push(method("admin", "root"));
        let err = method_of(&h, None).unwrap_err();
        assert!(err.contains("deploy") && err.contains("root"), "{err}");
        assert_eq!(method_of(&h, Some("root")).unwrap(), h.methods[1].id);
        assert_eq!(method_of(&h, Some("ADMIN")).unwrap(), h.methods[1].id);
    }
}
