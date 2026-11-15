// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! UI state: the vault list, the view (without secrets) of each open vault, the route,
//! the tabs and the last error. The source of truth lives in `vaults` and `sessions`; here we keep copies
//! for rendering. It is the equivalent of the zustand stores the web version had.
//!
//! Tabs belong to the UI: detail tabs (host, key, tunnel), settings and sessions (terminal and SFTP).
//! `sessions` still owns the connections; here we only decide the order and which one is shown. Each
//! vault is like a project in Zed: its tabs and the global ones (the settings) are shown, and those of another
//! vault stay open, waiting until you switch back to it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{App, Context, EventEmitter, Task, Window};
use serde::{Deserialize, Serialize};
use sessions::{Event, Sessions, TabInfo, TabKind};
use uuid::Uuid;
use vaults::{VaultItem, VaultView, Vaults};

use crate::app::tabs::{Edge, OpenTab, Scope, Tab, Tabs};

/// List in the sidebar of a vault.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Hosts,
    Keys,
    Tunnels,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// The vault list.
    Picker,
    Vault(Uuid, Section),
    /// Settings opened from the vault list, where there are no tabs.
    Settings,
    /// Licenses, opened from the vault list.
    Licenses,
}

/// Sidebar width: minimum, default and maximum (in `theme::px` units).
pub const SIDEBAR_WIDTH: (f32, f32, f32) = (180., 250., 640.);

/// The `layout` key in `settings.json`: how the user left the window, so it can be restored the same way.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub sidebar_width: f32,
    pub sidebar_open: bool,
}

impl Layout {
    /// A `settings.json` edited by hand may contain any width.
    fn clamped(mut self) -> Self {
        let (min, default, max) = SIDEBAR_WIDTH;

        self.sidebar_width = if self.sidebar_width.is_finite() {
            self.sidebar_width.clamp(min, max)
        } else {
            default
        };
        self
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_WIDTH.1,
            sidebar_open: true,
        }
    }
}

/// Host with several methods to connect with: asks which one to use.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Picking {
    pub vault: Uuid,
    pub host: Uuid,
    pub kind: TabKind,
}

/// Notice for the views of a tab (terminal, SFTP), which subscribe to `AppState`.
#[derive(Clone, Debug)]
pub enum TabEvent {
    Screen(Uuid),
    Queue(Uuid),
    Reload { tab: Uuid, side: sftp::Side },
}

pub struct AppState {
    pub vaults: Arc<Vaults>,
    pub sessions: Arc<Sessions>,
    pub list: Vec<VaultItem>,
    /// View of each unlocked vault. If it is missing, the vault is locked.
    pub views: HashMap<Uuid, VaultView>,
    /// Last error of an action, to show it in the status bar.
    pub error: Option<String>,
    /// Vaults being unlocked with their remembered password.
    pub unlocking: HashSet<Uuid>,
    /// Open vaults whose folder disappeared: the user is warned until they dismiss the notice.
    pub lost: Vec<String>,
    pub route: Route,
    /// Open tabs, from all vaults, and the panes that hold them.
    pub tabs: Tabs,
    /// Copy of `sessions.list()`: title and state of each session.
    pub session_list: Vec<TabInfo>,
    /// Sidebar: whether it is expanded, and its width. Saved in `settings.json`.
    pub layout: Layout,
    pub picking: Option<Picking>,
    /// Collapsed groups of the host tree (`vault`, group; `None` is "Ungrouped").
    pub collapsed: HashSet<(Uuid, Option<Uuid>)>,
    /// Text of the sidebar search box.
    pub search: String,
    /// One attempt with the remembered password per vault and run: this way, locking by hand leaves the
    /// vault locked until the app is restarted.
    attempts: HashSet<Uuid>,
    /// Error of the last automatic attempt, to show it in the unlock form.
    auto_errors: HashMap<Uuid, String>,
    /// Open vaults that were already missing their folder in the last check.
    suspect: HashSet<Uuid>,
    /// New version published, if there is one. It is checked once at startup.
    pub update: Option<updater::Update>,
    /// Downloading the new version: when it finishes, the installer closes the app.
    pub updating: bool,
    /// Why the last update attempt failed. It goes on the button itself: the status bar
    /// is not visible outside a vault.
    pub update_error: Option<String>,
}

impl EventEmitter<TabEvent> for AppState {}

impl AppState {
    pub fn new(vaults: Arc<Vaults>, sessions: Arc<Sessions>) -> Self {
        Self {
            list: vaults.list(),
            vaults,
            sessions,
            views: HashMap::new(),
            error: None,
            unlocking: HashSet::new(),
            lost: Vec::new(),
            route: Route::Picker,
            tabs: Tabs::new(),
            session_list: Vec::new(),
            layout: settings::load::<Layout>("layout").clamped(),
            picking: None,
            collapsed: HashSet::new(),
            search: String::new(),
            attempts: HashSet::new(),
            auto_errors: HashMap::new(),
            suspect: HashSet::new(),
            update: None,
            updating: false,
            update_error: None,
        }
    }

    pub fn item(&self, vault: Uuid) -> Option<&VaultItem> {
        self.list.iter().find(|v| v.id == vault)
    }

    /// The vault of the current route.
    pub fn current(&self) -> Option<Uuid> {
        match self.route {
            Route::Vault(vault, _) => Some(vault),
            _ => None,
        }
    }

    /// The sidebar section for the current route.
    pub fn section(&self) -> Option<Section> {
        match self.route {
            Route::Vault(_, section) => Some(section),
            _ => None,
        }
    }

    // ---- Session notices ----

    pub fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Tabs => self.sync_tabs(cx),
            Event::Screen(tab) => cx.emit(TabEvent::Screen(tab)),
            Event::Queue(tab) => cx.emit(TabEvent::Queue(tab)),
            Event::Reload { tab, side } => cx.emit(TabEvent::Reload { tab, side }),
            Event::Clipboard(text) => {
                if crate::app::prefs::Prefs::get(cx).terminal.remote_clipboard {
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text))
                }
            }
            Event::VaultChanged(vault) => self.refresh_view(vault, cx),
            Event::Tunnels => cx.notify(),
        }
    }

    /// Copies the session list. New sessions without a tab (opened from the MCP server)
    /// are added at the end; closed ones are removed.
    fn sync_tabs(&mut self, cx: &mut Context<Self>) {
        self.session_list = self.sessions.list();

        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        for info in &self.session_list {
            if !self.tabs.list.iter().any(|t| t.id == info.id) {
                self.tabs.list.push(OpenTab {
                    id: info.id,
                    tab: Tab::Session(info.id),
                    preview: false,
                    pane: self.tabs.target_pane(scope),
                });
            }
        }

        let list = &self.session_list;
        let gone: Vec<Uuid> = self
            .tabs
            .list
            .iter()
            .filter(|t| matches!(t.tab, Tab::Session(id) if !list.iter().any(|i| i.id == id)))
            .map(|t| t.id)
            .collect();

        self.tabs.remove(&gone, scope);
        cx.notify();
    }

    // ---- Navigation ----

    /// Goes to a route. Tabs stay open: when switching vaults, the last one used in
    /// it is shown (or none), and a global tab (the settings) stays where it is.
    pub fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        let before = self.current();

        // A vault without a folder cannot be opened: it stays in the list, where its location is changed.
        if let Route::Vault(vault, _) = route
            && self.item(vault).is_none_or(|v| v.missing)
        {
            self.route = Route::Picker;
        } else {
            self.route = route;
        }

        let after = self.current();

        if after != before && after.is_some() {
            if let Some(old) = before {
                self.tabs.remember(old);
            }

            let scope = Scope {
                current: after,
                sessions: &self.session_list,
            };

            // Already visible if it belongs to this vault or is global (e.g. settings opened from the list).
            if self.tabs.active_tab(scope).is_none() {
                self.tabs.active = None;

                if let Some(id) = after
                    .and_then(|v| self.tabs.last_in(v))
                    .filter(|id| self.tabs.visible(scope).any(|t| t.id == *id))
                {
                    self.tabs.activate(id);
                }
            }
        }

        if let Route::Vault(vault, _) = self.route {
            self.auto_unlock(vault, cx);
        }

        cx.notify();
    }

    /// Changes the sidebar list. Pressing the section that is already shown collapses or expands the panel, as in
    /// Zed and VS Code.
    pub fn show_section(&mut self, section: Section, toggle: bool, cx: &mut Context<Self>) {
        let Some(vault) = self.current() else {
            return;
        };

        if toggle && self.section() == Some(section) {
            self.layout.sidebar_open = !self.layout.sidebar_open;
        } else {
            self.route = Route::Vault(vault, section);
            self.layout.sidebar_open = true;
        }

        self.save_layout(cx);
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.layout.sidebar_open = !self.layout.sidebar_open;
        self.save_layout(cx);
    }

    /// While the sidebar edge is being dragged. `max` leaves room for the content in
    /// narrow windows. It is saved on release (`save_layout`), not on every move.
    pub fn resize_sidebar(&mut self, width: f32, max: f32, cx: &mut Context<Self>) {
        let (min, _, limit) = SIDEBAR_WIDTH;
        let width = width.clamp(min, limit.min(max).max(min)).round();

        if width != self.layout.sidebar_width {
            self.layout.sidebar_width = width;
            cx.notify();
        }
    }

    /// Saves the sidebar state. If that fails, the status bar reports it.
    pub fn save_layout(&mut self, cx: &mut Context<Self>) {
        let result = settings::save("layout", &self.layout);
        self.report(result, cx);
    }

    pub fn show_tab(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.tabs.activate(id);
        cx.notify();
    }

    /// What the tab logic needs from the rest of the app. It borrows all of `self`: where `self.tabs` is
    /// borrowed mutably, build the `Scope` from its fields instead.
    fn scope(&self) -> Scope<'_> {
        Scope {
            current: self.current(),
            sessions: &self.session_list,
        }
    }

    // ---- Panes ----

    pub fn pane_of(&self, id: Uuid) -> Option<Uuid> {
        self.tabs.pane_of(id)
    }

    /// The visible tabs of a pane.
    pub fn pane_tabs(&self, pane: Uuid) -> impl Iterator<Item = &OpenTab> {
        self.tabs.pane_tabs(pane, self.scope())
    }

    /// The tab a pane shows: the active one if it is its own, the last one it showed, or the first.
    pub fn shown(&self, pane: Uuid) -> Option<Uuid> {
        self.tabs.shown(pane, self.scope())
    }

    /// Drops the tab `id` on the pane `pane`: in the center it moves into it; on an edge, into a new
    /// pane on that side. The view and the session stay the same.
    pub fn drop_tab(&mut self, id: Uuid, pane: Uuid, edge: Edge, cx: &mut Context<Self>) {
        if self.tabs.drop_tab(id, pane, edge) {
            cx.notify();
        }
    }

    /// Each pane calls this on every drag move: `target` if the mouse is over it. Otherwise,
    /// it only clears the target when it was its own. It repaints only if it changes.
    pub fn set_drop_target(
        &mut self,
        pane: Uuid,
        target: Option<(Uuid, Edge)>,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.set_drop_target(pane, target) {
            cx.notify();
        }
    }

    // ---- Tabs ----

    /// The vault of a tab. `None` for global tabs (the app settings).
    pub fn tab_vault(&self, tab: &Tab) -> Option<Uuid> {
        self.scope().vault_of(tab)
    }

    pub fn session(&self, id: Uuid) -> Option<&TabInfo> {
        self.session_list.iter().find(|i| i.id == id)
    }

    /// The tabs that are shown: those of the current vault and the global ones.
    pub fn visible(&self) -> impl Iterator<Item = &OpenTab> {
        self.tabs.visible(self.scope())
    }

    /// What the active tab shows, if it is visible.
    pub fn active_tab(&self) -> Option<Tab> {
        self.tabs.active_tab(self.scope())
    }

    /// Opens an entry in the preview tab. If it is already open, it is activated.
    pub fn preview(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.preview(tab, scope);
        cx.notify();
    }

    /// Opens an entry and pins it: when saving a form or editing it.
    pub fn open_pinned(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.open_pinned(tab, scope);
        cx.notify();
    }

    /// A tab stops being a preview.
    pub fn pin(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.tabs.pin(id) {
            cx.notify();
        }
    }

    /// Sets the name of a tab. Renaming it pins it: the preview would replace it with the
    /// next entry, and the name would be lost.
    pub fn rename_tab(&mut self, id: Uuid, name: String, cx: &mut Context<Self>) {
        self.tabs.rename(id, name);
        cx.notify();
    }

    /// App or vault settings: only one tab of each, always pinned.
    pub fn open_single(&mut self, tab: Tab, cx: &mut Context<Self>) {
        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.open_single(tab, scope);
        cx.notify();
    }

    /// Closes the detail tabs of a host, a key or a tunnel (when it is deleted).
    pub fn close_item(&mut self, item: Uuid, cx: &mut Context<Self>) {
        let ids: Vec<Uuid> = self
            .tabs
            .list
            .iter()
            .filter(|t| {
                matches!(t.tab, Tab::Host { id, .. } | Tab::Key { id, .. } | Tab::Tunnel { id, .. } if id == item)
            })
            .map(|t| t.id)
            .collect();
        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.remove(&ids, scope);
        cx.notify();
    }

    /// Drops the tab `id` on `target`: it takes its place (and its pane), pushing it toward
    /// where it came from.
    pub fn move_tab(&mut self, id: Uuid, target: Uuid, cx: &mut Context<Self>) {
        if self.tabs.move_tab(id, target) {
            cx.notify();
        }
    }

    // ---- Vaults ----

    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    /// Stores the error (or clears it if it succeeded) and returns whether it succeeded.
    pub fn report<T>(&mut self, result: Result<T, String>, cx: &mut Context<Self>) -> Option<T> {
        match result {
            Ok(value) => {
                self.error = None;
                cx.notify();
                Some(value)
            }
            Err(e) => {
                self.set_error(Some(e), cx);
                None
            }
        }
    }

    pub fn refresh_list(&mut self, cx: &mut Context<Self>) {
        self.list = self.vaults.list();
        cx.notify();
    }

    /// Re-reads the view of an open vault after changing it. It also starts or stops its
    /// tunnels: this way they start when the vault is opened and follow every change (from the app or the MCP).
    pub fn refresh_view(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.sessions.sync_tunnels(vault);

        match self.vaults.view(vault) {
            Ok(view) => {
                self.views.insert(vault, view);
            }
            Err(_) => {
                self.views.remove(&vault);
            }
        }

        cx.notify();
    }

    /// Runs a change on the vault and re-reads the view. Returns the result of the change if it succeeded;
    /// otherwise, the error goes to the status bar.
    pub fn change<T>(
        &mut self,
        vault: Uuid,
        action: impl FnOnce(&Vaults) -> Result<T, String>,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        let result = action(&self.vaults);
        let value = self.report(result, cx);

        if self.vaults.is_unlocked(vault) {
            self.refresh_view(vault, cx);
        }

        value
    }

    /// Checks in the background whether a new version exists. Without network access or on error, nothing is reported.
    pub fn check_update(&mut self, cx: &mut Context<Self>) {
        let work = cx.background_executor().spawn(async { updater::check() });

        cx.spawn(async move |this, cx| {
            let Ok(Some(update)) = work.await else {
                return;
            };

            this.update(cx, |this, cx| {
                this.update = Some(update);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Downloads the new version and launches its installer, which closes the app and reopens it.
    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(update) = self.update.clone().filter(|_| !self.updating) else {
            return;
        };

        self.updating = true;
        cx.notify();

        // If all goes well, `download_and_install` does not return: the process ends.
        let work = cx
            .background_executor()
            .spawn(async move { update.download_and_install() });

        cx.spawn(async move |this, cx| {
            let result = work.await;

            this.update(cx, |this, cx| {
                this.updating = false;
                this.update_error = result.err().map(|e| format!("Could not update: {e}"));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Unlocks with the remembered password, only once per run. This way it opens before
    /// the user enters and the unlock form is never shown.
    pub fn auto_unlock(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        let Some(info) = self.item(vault) else {
            return;
        };

        if self.views.contains_key(&vault)
            || !info.remember_password
            || info.missing
            || !self.attempts.insert(vault)
        {
            return;
        }

        self.unlocking.insert(vault);
        let vaults = self.vaults.clone();
        let work = cx
            .background_executor()
            .spawn(async move { vaults.auto_unlock(vault) });

        cx.spawn(async move |this, cx| {
            let result = work.await;

            this.update(cx, |this, cx| {
                this.unlocking.remove(&vault);

                // If it fails (the password changed on another computer, the folder is missing...), the user is asked to type it.
                match result {
                    Ok(true) => {
                        this.refresh_view(vault, cx);
                        this.refresh_list(cx);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        this.auto_errors.insert(vault, e);
                    }
                }

                cx.notify();
            })
            .ok();
        })
        .detach();

        cx.notify();
    }

    /// On startup: unlocks in the background the vaults with a remembered password.
    pub fn auto_unlock_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<Uuid> = self.list.iter().map(|v| v.id).collect();

        for id in ids {
            self.auto_unlock(id, cx);
        }
    }

    /// Takes the error of the automatic attempt, if there was one. It is only shown once.
    pub fn take_auto_error(&mut self, vault: Uuid) -> Option<String> {
        self.auto_errors.remove(&vault)
    }

    /// Unlocks with the typed password. It is slow (Argon2): it runs in the background and `done`
    /// receives the error, if any.
    pub fn unlock(
        &mut self,
        vault: Uuid,
        password: String,
        remember: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let vaults = self.vaults.clone();
        let work = cx
            .background_executor()
            .spawn(async move { vaults.unlock(vault, &password, remember) });

        cx.spawn(async move |this, cx| {
            let result = work.await;

            this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.refresh_view(vault, cx);
                    this.refresh_list(cx);
                }
            })
            .ok();

            result
        })
    }

    /// Creates an empty vault in `dir` and leaves it unlocked. Returns its id.
    pub fn create(
        &mut self,
        dir: PathBuf,
        password: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<Uuid, String>> {
        let vaults = self.vaults.clone();
        let work = cx
            .background_executor()
            .spawn(async move { vaults.create(dir, &password) });

        cx.spawn(async move |this, cx| {
            let result = work.await;

            this.update(cx, |this, cx| {
                if let Ok(vault) = &result {
                    this.refresh_list(cx);
                    this.refresh_view(*vault, cx);
                }
            })
            .ok();

            result
        })
    }

    /// Changes the master password. It is slow (Argon2 twice).
    pub fn change_password(
        &mut self,
        vault: Uuid,
        current: String,
        new: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let vaults = self.vaults.clone();

        cx.background_executor()
            .spawn(async move { vaults.change_password(vault, &current, &new) })
    }

    /// Locks a vault: its key is forgotten and its tabs are closed.
    pub fn lock(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.close_vault_tabs(vault);
        self.vaults.lock(vault);
        self.views.remove(&vault);
        self.refresh_list(cx);
    }

    /// Closes the sessions, detail tabs and settings of a vault.
    fn close_vault_tabs(&mut self, vault: Uuid) {
        let ids: Vec<Uuid> = self
            .tabs
            .list
            .iter()
            .filter(|t| self.tab_vault(&t.tab) == Some(vault))
            .map(|t| t.id)
            .collect();

        self.sessions.close_vault(vault);

        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.remove(&ids, scope);
        self.session_list = self.sessions.list();
        self.tabs.forget(vault);
        self.picking = None;
    }

    /// Removes a vault from the list. It does not delete any file.
    pub fn forget(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.close_vault_tabs(vault);
        self.views.remove(&vault);
        let result = self.vaults.forget(vault);
        self.report(result, cx);
        self.refresh_list(cx);
    }

    /// Adds to the list a vault that already exists in `dir`.
    pub fn add_existing(&mut self, dir: PathBuf, cx: &mut Context<Self>) -> Result<Uuid, String> {
        let vault = self.vaults.add(dir)?;
        self.refresh_list(cx);
        Ok(vault)
    }

    /// Points a vault to its new folder. It is locked, so everything belonging to it is closed; a new
    /// attempt with the remembered password is allowed when it is opened.
    pub fn relocate(
        &mut self,
        vault: Uuid,
        dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.vaults.relocate(vault, dir)?;
        self.close_vault_tabs(vault);
        self.views.remove(&vault);
        self.attempts.remove(&vault);
        self.refresh_list(cx);
        Ok(())
    }

    pub fn forget_password(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        let result = self.vaults.forget_password(vault);
        self.report(result, cx);
        self.refresh_list(cx);
    }

    /// Re-reads the vault from disk (for example, after a conflict with another computer).
    pub fn reload(&mut self, vault: Uuid, cx: &mut Context<Self>) {
        self.change(vault, |v| v.reload(vault), cx);
    }

    /// Checks whether the vaults are still in their folder; it is called every few seconds. An open vault
    /// that is missing twice in a row is considered lost: it is locked and reported. Two checks, not one,
    /// so as not to react to a file that Drive or Git removes for a moment while syncing.
    pub fn check_vaults(&mut self, cx: &mut Context<Self>) {
        let list = self.vaults.list();

        // It is only stored if something changed: this way the whole UI is not repainted each time.
        if list != self.list {
            self.list = list.clone();
            cx.notify();
        }

        for vault in list {
            if !(vault.unlocked && vault.missing) {
                self.suspect.remove(&vault.id);
            } else if self.suspect.insert(vault.id) {
                // First time: wait for the next check.
            } else {
                self.suspect.remove(&vault.id);
                self.lock(vault.id, cx);
                self.lost.push(vault.name.clone());

                if self.current() == Some(vault.id) {
                    self.go(Route::Picker, cx);
                }
            }
        }
    }

    // ---- Sesiones ----

    /// With a single method it connects directly; with several, it asks which one.
    pub fn connect(&mut self, vault: Uuid, host: Uuid, kind: TabKind, cx: &mut Context<Self>) {
        let methods: Vec<Uuid> = self
            .views
            .get(&vault)
            .and_then(|v| v.hosts.iter().find(|h| h.id == host))
            .map(|h| h.methods.iter().map(|m| m.id).collect())
            .unwrap_or_default();

        match methods.as_slice() {
            [] => {}
            [only] => self.open_tab(vault, host, *only, kind, cx),
            _ => {
                self.picking = Some(Picking { vault, host, kind });
                cx.notify();
            }
        }
    }

    pub fn open_tab(
        &mut self,
        vault: Uuid,
        host: Uuid,
        method: Uuid,
        kind: TabKind,
        cx: &mut Context<Self>,
    ) {
        self.picking = None;
        let id = self.sessions.open(vault, host, method, kind);
        self.session_list = self.sessions.list();

        // The host entry in preview (the one opened by the first click of a double click) gives
        // its place to the connection. `Tabs::insert` puts it to its right; when the entry is removed, the connection stays in its place.
        let preview = self
            .tabs
            .list
            .iter()
            .find(|t| t.preview && t.tab == Tab::Host { vault, id: host })
            .map(|t| t.id);

        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.insert(
            OpenTab {
                id,
                tab: Tab::Session(id),
                preview: false,
                pane: Uuid::nil(),
            },
            scope,
        );

        if let Some(old) = preview {
            self.tabs.remove(&[old], scope);
        }

        self.tabs.activate(id);
        cx.notify();
    }

    /// Closes a tab; if it is a session, its connection is closed too.
    pub fn close_tab(&mut self, id: Uuid, cx: &mut Context<Self>) {
        if self.session(id).is_some() {
            self.sessions.close(id);
            self.session_list.retain(|i| i.id != id);
        }

        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.remove(&[id], scope);
        cx.notify();
    }

    /// When a host is deleted, its sessions and its entry are closed.
    pub fn close_host(&mut self, host: Uuid, cx: &mut Context<Self>) {
        let ids: Vec<Uuid> = self
            .session_list
            .iter()
            .filter(|i| i.host == host)
            .map(|i| i.id)
            .collect();

        self.sessions.close_host(host);
        self.session_list.retain(|i| i.host != host);
        let scope = Scope {
            current: self.current(),
            sessions: &self.session_list,
        };

        self.tabs.remove(&ids, scope);
        self.close_item(host, cx);
    }

    pub fn reconnect(&mut self, id: Uuid) {
        self.sessions.reconnect(id);
    }

    /// The user accepts the server's fingerprint: it is saved on the host and it reconnects.
    pub fn trust_host_key(&mut self, id: Uuid, fingerprint: String, cx: &mut Context<Self>) {
        let result = self.sessions.trust_host_key(id, fingerprint);
        self.report(result, cx);
    }

    /// "user@host" of a tab.
    pub fn tab_title(&self, tab: &TabInfo) -> String {
        let host = self
            .views
            .get(&tab.vault)
            .and_then(|v| v.hosts.iter().find(|h| h.id == tab.host));
        let method = host.and_then(|h| h.methods.iter().find(|m| m.id == tab.method));

        match (host, method) {
            (Some(h), Some(m)) => format!("{}@{}", m.username, h.name),
            _ => "?".into(),
        }
    }

    pub fn toggle_group(&mut self, vault: Uuid, group: Option<Uuid>, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&(vault, group)) {
            self.collapsed.insert((vault, group));
        }

        cx.notify();
    }
}

/// Opens the system picker (files if `files`, otherwise folders) and calls `done` with the chosen path.
pub fn pick(
    prompt: &str,
    files: bool,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(PathBuf, &mut Window, &mut App) + 'static,
) {
    let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
        files,
        directories: !files,
        multiple: false,
        prompt: Some(prompt.to_string().into()),
    });

    window
        .spawn(cx, async move |cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                cx.update(|window, cx| done(path, window, cx)).ok();
            }
        })
        .detach();
}
