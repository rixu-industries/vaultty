// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Open tabs and the split panes that hold them. It knows nothing about vaults, sessions or the UI: what
//! it needs from the rest of the app comes in a [`Scope`], so it can be tested without a GPUI context.

use std::collections::HashMap;

use sessions::TabInfo;
use uuid::Uuid;

/// What a tab shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    /// Terminal or SFTP from `sessions`: the id is that of its `TabInfo`.
    Session(Uuid),
    Host {
        vault: Uuid,
        id: Uuid,
    },
    Key {
        vault: Uuid,
        id: Uuid,
    },
    Tunnel {
        vault: Uuid,
        id: Uuid,
    },
    VaultSettings(Uuid),
    KnownHosts(Uuid),
    Settings,
    /// Help → Open Source Licenses.
    Licenses,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenTab {
    /// For sessions, the same id as their `TabInfo`.
    pub id: Uuid,
    pub tab: Tab,
    /// Preview: the next entry that is opened replaces it. It is drawn in italics.
    pub preview: bool,
    /// The pane it is in (a leaf of `Tabs::panes`).
    pub pane: Uuid,
}

/// Split panes, as in Zed: each leaf is a pane with its own tab bar. Moving a tab
/// to another pane does not touch its view or its session: only `OpenTab::pane` changes.
#[derive(Clone, PartialEq, Debug)]
pub enum Pane {
    Leaf(Uuid),
    Split {
        /// One above the other; otherwise, side by side.
        down: bool,
        first: Box<Pane>,
        second: Box<Pane>,
    },
}

/// Where a tab is dropped on a pane: on an edge (below, above or to one side, and whether the new
/// pane goes first) or, with `None`, in the center.
pub type Edge = Option<(bool, bool)>;

impl Pane {
    fn leaves(&self, out: &mut Vec<Uuid>) {
        match self {
            Pane::Leaf(id) => out.push(*id),
            Pane::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    /// What remains when a pane is removed: its sibling takes its place.
    fn without(self, id: Uuid) -> Option<Pane> {
        match self {
            Pane::Leaf(t) => (t != id).then_some(Pane::Leaf(t)),
            Pane::Split {
                down,
                first,
                second,
            } => match (first.without(id), second.without(id)) {
                (Some(a), Some(b)) => Some(Pane::Split {
                    down,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (a, b) => a.or(b),
            },
        }
    }

    /// Puts the pane `new` next to `target`: below (or above, with `before`) if `down`; otherwise, to the
    /// right (or to the left).
    fn insert(self, target: Uuid, new: Uuid, down: bool, before: bool) -> Pane {
        match self {
            Pane::Leaf(t) if t == target => {
                let (a, b) = if before { (new, t) } else { (t, new) };

                Pane::Split {
                    down,
                    first: Box::new(Pane::Leaf(a)),
                    second: Box::new(Pane::Leaf(b)),
                }
            }
            Pane::Split {
                down: d,
                first,
                second,
            } => Pane::Split {
                down: d,
                first: Box::new(first.insert(target, new, down, before)),
                second: Box::new(second.insert(target, new, down, before)),
            },
            other => other,
        }
    }
}

/// What the tab logic needs to know about the rest of the app: the vault on screen and the sessions,
/// to tell which vault a session tab belongs to.
#[derive(Clone, Copy)]
pub struct Scope<'a> {
    pub current: Option<Uuid>,
    pub sessions: &'a [TabInfo],
}

impl Scope<'_> {
    /// The vault of a tab. `None` for global tabs (the app settings).
    pub fn vault_of(&self, tab: &Tab) -> Option<Uuid> {
        match *tab {
            Tab::Session(id) => self.sessions.iter().find(|i| i.id == id).map(|i| i.vault),
            Tab::Host { vault, .. }
            | Tab::Key { vault, .. }
            | Tab::Tunnel { vault, .. }
            | Tab::VaultSettings(vault)
            | Tab::KnownHosts(vault) => Some(vault),
            Tab::Settings | Tab::Licenses => None,
        }
    }
}

pub struct Tabs {
    /// Open tabs, from all vaults, in the order they are shown.
    pub list: Vec<OpenTab>,
    /// The tab being shown (`OpenTab::id`). `None`: the empty state of the section.
    pub active: Option<Uuid>,
    /// The tab that was shown in each vault when the user left it, to return to it.
    last_active: HashMap<Uuid, Uuid>,
    /// The panes. A single set for all vaults: each vault shows the panes that have its own tabs
    /// (or global ones), and a pane without any tab disappears.
    pub panes: Pane,
    /// The tab each pane shows when the focus is on another pane.
    pane_active: HashMap<Uuid, Uuid>,
    /// While a tab is being dragged: the pane and the area under the mouse, to mark where it will go.
    pub drop_target: Option<(Uuid, Edge)>,
    /// Names set by hand on tabs (right click → Rename); they take precedence over the title.
    pub renames: HashMap<Uuid, String>,
}

impl Tabs {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            active: None,
            last_active: HashMap::new(),
            panes: Pane::Leaf(Uuid::new_v4()),
            pane_active: HashMap::new(),
            drop_target: None,
            renames: HashMap::new(),
        }
    }

    /// Remembers the tab being shown as the one of `vault`, for when the user returns to it.
    pub fn remember(&mut self, vault: Uuid) {
        if let Some(active) = self.active {
            self.last_active.insert(vault, active);
        }
    }

    /// The tab that was shown in `vault` when the user left it.
    pub fn last_in(&self, vault: Uuid) -> Option<Uuid> {
        self.last_active.get(&vault).copied()
    }

    /// Forgets the tab remembered for `vault`.
    pub fn forget(&mut self, vault: Uuid) {
        self.last_active.remove(&vault);
    }

    /// The tab becomes the active one and the one its pane shows.
    pub fn activate(&mut self, id: Uuid) {
        self.active = Some(id);

        if let Some(pane) = self.pane_of(id) {
            self.pane_active.insert(pane, id);
        }
    }

    // ---- Panes ----

    pub fn pane_of(&self, id: Uuid) -> Option<Uuid> {
        self.list.iter().find(|t| t.id == id).map(|t| t.pane)
    }

    /// The visible tabs of a pane.
    pub fn pane_tabs<'a>(
        &'a self,
        pane: Uuid,
        scope: Scope<'a>,
    ) -> impl Iterator<Item = &'a OpenTab> {
        self.visible(scope).filter(move |t| t.pane == pane)
    }

    /// The tab a pane shows: the active one if it is its own, the last one it showed, or the first.
    pub fn shown(&self, pane: Uuid, scope: Scope<'_>) -> Option<Uuid> {
        let has = |id: &Uuid| self.pane_tabs(pane, scope).any(|t| t.id == *id);

        self.active
            .filter(has)
            .or_else(|| self.pane_active.get(&pane).copied().filter(has))
            .or_else(|| self.pane_tabs(pane, scope).next().map(|t| t.id))
    }

    /// Where a new tab goes: the focused pane, or the first visible one.
    pub fn target_pane(&self, scope: Scope<'_>) -> Uuid {
        let mut leaves = Vec::new();
        self.panes.leaves(&mut leaves);

        self.active
            .filter(|_| self.active_tab(scope).is_some())
            .and_then(|id| self.pane_of(id))
            .or_else(|| {
                leaves
                    .iter()
                    .copied()
                    .find(|p| self.pane_tabs(*p, scope).next().is_some())
            })
            .unwrap_or(leaves[0])
    }

    /// Drops the tab `id` on the pane `pane`: in the center it moves into it; on an edge, into a new
    /// pane on that side. The view and the session stay the same. Returns whether anything changed
    /// (the screen needs repainting).
    pub fn drop_tab(&mut self, id: Uuid, pane: Uuid, edge: Edge) -> bool {
        self.drop_target = None;

        let Some(at) = self.list.iter().position(|t| t.id == id) else {
            return false;
        };

        match edge {
            // At the end of the list, which is the end of its pane: dropping it in the empty slot
            // leaves it last as well if it was already there.
            None => {
                let mut tab = self.list.remove(at);
                tab.pane = pane;
                self.list.push(tab);
            }
            Some((down, before)) => {
                // Alone in its pane: splitting it would leave it empty and nothing would change.
                if self.list[at].pane == pane
                    && self.list.iter().filter(|t| t.pane == pane).count() == 1
                {
                    return false;
                }

                let new = Uuid::new_v4();
                self.panes = self.panes.clone().insert(pane, new, down, before);
                self.list[at].pane = new;
            }
        }

        self.prune();
        self.activate(id);
        true
    }

    /// Each pane calls this on every drag move: `target` if the mouse is over it. Otherwise,
    /// it only clears the target when it was its own. Returns whether it changed, to repaint only then.
    pub fn set_drop_target(&mut self, pane: Uuid, target: Option<(Uuid, Edge)>) -> bool {
        let next = match target {
            Some(t) => Some(t),
            None if self.drop_target.is_some_and(|(p, _)| p == pane) => None,
            None => self.drop_target,
        };

        if next != self.drop_target {
            self.drop_target = next;
            return true;
        }

        false
    }

    /// Removes panes without tabs. One always remains.
    fn prune(&mut self) {
        let mut leaves = Vec::new();
        self.panes.leaves(&mut leaves);

        let empty: Vec<Uuid> = leaves
            .into_iter()
            .filter(|p| !self.list.iter().any(|t| t.pane == *p))
            .collect();

        self.panes = empty
            .iter()
            .try_fold(self.panes.clone(), |p, id| p.without(*id))
            .unwrap_or_else(|| Pane::Leaf(Uuid::new_v4()));
        self.pane_active
            .retain(|_, id| self.list.iter().any(|t| t.id == *id));
    }

    // ---- Tabs ----

    /// The tabs that are shown: those of the current vault and the global ones.
    pub fn visible<'a>(&'a self, scope: Scope<'a>) -> impl Iterator<Item = &'a OpenTab> {
        self.list.iter().filter(move |t| {
            let vault = scope.vault_of(&t.tab);
            vault.is_none() || vault == scope.current
        })
    }

    /// What the active tab shows, if it is visible.
    pub fn active_tab(&self, scope: Scope<'_>) -> Option<Tab> {
        let active = self.active?;
        self.visible(scope).find(|t| t.id == active).map(|t| t.tab)
    }

    /// Opens an entry in the preview tab. If it is already open, it is activated.
    pub fn preview(&mut self, tab: Tab, scope: Scope<'_>) {
        if let Some(id) = self.list.iter().find(|t| t.tab == tab).map(|t| t.id) {
            self.activate(id);
            return;
        }

        // As in Zed, each pane has its own preview.
        let pane = self.target_pane(scope);
        let new = OpenTab {
            id: Uuid::new_v4(),
            tab,
            preview: true,
            pane,
        };
        let old = self
            .pane_tabs(pane, scope)
            .find(|t| t.preview)
            .map(|t| t.id)
            .and_then(|id| self.list.iter().position(|t| t.id == id));

        match old {
            Some(at) => self.list[at] = new,
            None => self.insert(new, scope),
        }

        self.activate(new.id);
    }

    /// Opens an entry and pins it: when saving a form or editing it.
    pub fn open_pinned(&mut self, tab: Tab, scope: Scope<'_>) {
        self.preview(tab, scope);

        if let Some(open) = self.list.iter_mut().find(|t| t.tab == tab) {
            open.preview = false;
        }
    }

    /// A tab stops being a preview. Returns whether the tab exists.
    pub fn pin(&mut self, id: Uuid) -> bool {
        if let Some(open) = self.list.iter_mut().find(|t| t.id == id) {
            open.preview = false;
            return true;
        }

        false
    }

    /// Sets the name of a tab. Renaming it pins it: the preview would replace it with the
    /// next entry, and the name would be lost.
    pub fn rename(&mut self, id: Uuid, name: String) {
        self.renames.insert(id, name);
        self.pin(id);
    }

    /// App or vault settings: only one tab of each, always pinned.
    pub fn open_single(&mut self, tab: Tab, scope: Scope<'_>) {
        if let Some(id) = self.list.iter().find(|t| t.tab == tab).map(|t| t.id) {
            self.activate(id);
        } else {
            let new = OpenTab {
                id: Uuid::new_v4(),
                tab,
                preview: false,
                pane: Uuid::nil(),
            };

            self.insert(new, scope);
            self.activate(new.id);
        }
    }

    /// In the focused pane, to the right of the active tab, or at the end.
    pub fn insert(&mut self, mut tab: OpenTab, scope: Scope<'_>) {
        tab.pane = self.target_pane(scope);

        let at = self
            .active
            .and_then(|id| self.list.iter().position(|t| t.id == id))
            .map_or(self.list.len(), |i| i + 1);

        self.list.insert(at, tab);
    }

    /// Removes tabs (without closing sessions). If the active one goes, the tab to its right in its pane is shown or,
    /// if there is none, the one to its left, as in browsers. If its pane is left empty, the focus
    /// moves to whatever another pane shows.
    pub fn remove(&mut self, ids: &[Uuid], scope: Scope<'_>) {
        if ids.is_empty() {
            return;
        }

        let mut next = None;

        if let Some(active) = self.active.filter(|a| ids.contains(a)) {
            let pane = self.pane_of(active);
            let left: Vec<Uuid> = self
                .visible(scope)
                .filter(|t| Some(t.pane) == pane)
                .map(|t| t.id)
                .collect();
            let at = left.iter().position(|id| *id == active);

            next = at
                .and_then(|at| {
                    let right = left[at + 1..].iter().find(|id| !ids.contains(id));
                    let before = left[..at].iter().rev().find(|id| !ids.contains(id));
                    right.or(before).copied()
                })
                .or_else(|| {
                    self.visible(scope)
                        .filter(|t| !ids.contains(&t.id))
                        .find(|t| self.pane_active.get(&t.pane) == Some(&t.id))
                        .or_else(|| self.visible(scope).find(|t| !ids.contains(&t.id)))
                        .map(|t| t.id)
                });
            self.active = None;
        }

        self.list.retain(|t| !ids.contains(&t.id));
        self.last_active.retain(|_, id| !ids.contains(id));
        self.renames.retain(|id, _| !ids.contains(id));
        self.prune();

        if let Some(id) = next {
            self.activate(id);
        }
    }

    /// Drops the tab `id` on `target`: it takes its place (and its pane), pushing it toward
    /// where it came from. Returns whether anything changed (the screen needs repainting).
    pub fn move_tab(&mut self, id: Uuid, target: Uuid) -> bool {
        let (Some(from), Some(to)) = (
            self.list.iter().position(|t| t.id == id),
            self.list.iter().position(|t| t.id == target),
        ) else {
            return false;
        };

        if from == to {
            return false;
        }

        let mut tab = self.list.remove(from);
        tab.pane = self.list[if from < to { to - 1 } else { to }].pane;
        self.list.insert(to, tab);
        self.prune();
        self.activate(id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(current: Option<Uuid>) -> Scope<'static> {
        Scope {
            current,
            sessions: &[],
        }
    }

    fn host_tab(vault: Uuid, pane: Uuid) -> OpenTab {
        OpenTab {
            id: Uuid::new_v4(),
            tab: Tab::Host {
                vault,
                id: Uuid::new_v4(),
            },
            preview: false,
            pane,
        }
    }

    fn tabs_with(panes: Pane, list: Vec<OpenTab>) -> Tabs {
        Tabs {
            list,
            panes,
            ..Tabs::new()
        }
    }

    fn leaves(pane: &Pane) -> Vec<Uuid> {
        let mut out = Vec::new();
        pane.leaves(&mut out);
        out
    }

    // Splitting builds the expected leaf order and removing a leaf keeps its sibling in place.
    #[test]
    fn split_and_remove_leaves_sibling_in_place() {
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

        // a | (c above b)
        let pane = Pane::Leaf(a)
            .insert(a, b, false, false)
            .insert(b, c, true, true);
        let mut leaves = Vec::new();
        pane.leaves(&mut leaves);
        assert_eq!(leaves, [a, c, b]);

        // Without `b`, `c` moves up to the place of the b/c group.
        let rest = pane.without(b).unwrap();
        assert_eq!(rest.clone().without(c), Some(Pane::Leaf(a)));
        assert!(matches!(rest, Pane::Split { down: false, .. }));

        assert_eq!(Pane::Leaf(a).without(a), None);
    }

    // Removing one leaf of a split leaves the other as the whole tree.
    #[test]
    fn without_lifts_the_sibling_of_a_removed_leaf() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let pane = Pane::Leaf(a).insert(a, b, true, false);

        assert_eq!(pane.clone().without(a), Some(Pane::Leaf(b)));
        assert_eq!(pane.without(b), Some(Pane::Leaf(a)));
    }

    // Removing the only leaf leaves no tree at all.
    #[test]
    fn without_the_only_leaf_gives_none() {
        let a = Uuid::new_v4();

        assert_eq!(Pane::Leaf(a).without(a), None);
    }

    // Removing an id that is not a leaf leaves the tree as it was.
    #[test]
    fn without_an_unknown_id_changes_nothing() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let pane = Pane::Leaf(a).insert(a, b, false, false);

        assert_eq!(pane.clone().without(Uuid::new_v4()), Some(pane));
        assert_eq!(Pane::Leaf(a).without(b), Some(Pane::Leaf(a)));
    }

    // The new leaf goes after the target, or before it with `before`.
    #[test]
    fn insert_puts_the_new_leaf_after_or_before_the_target() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

        let after = Pane::Leaf(a).insert(a, b, false, false);
        assert_eq!(leaves(&after), [a, b]);

        let before = Pane::Leaf(a).insert(a, b, false, true);
        assert_eq!(leaves(&before), [b, a]);
    }

    // The `down` flag is kept in the split that is created.
    #[test]
    fn insert_stores_the_direction_of_the_split() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

        let down = Pane::Leaf(a).insert(a, b, true, false);
        assert!(matches!(down, Pane::Split { down: true, .. }));

        let side = Pane::Leaf(a).insert(a, b, false, false);
        assert!(matches!(side, Pane::Split { down: false, .. }));
    }

    // Inserting next to a leaf that does not exist leaves the tree as it was.
    #[test]
    fn insert_with_an_unknown_target_changes_nothing() {
        let a = Uuid::new_v4();

        let pane = Pane::Leaf(a).insert(Uuid::new_v4(), Uuid::new_v4(), true, false);

        assert_eq!(pane, Pane::Leaf(a));
    }

    // A pane with no tabs disappears and its sibling takes its place.
    #[test]
    fn prune_removes_a_pane_without_tabs() {
        let (p1, p2, v) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let panes = Pane::Leaf(p1).insert(p1, p2, false, false);
        let mut tabs = tabs_with(panes, vec![host_tab(v, p1)]);

        tabs.prune();

        assert_eq!(tabs.panes, Pane::Leaf(p1));
    }

    // With no tabs at all, a single empty pane remains.
    #[test]
    fn prune_always_keeps_one_pane() {
        let (p1, p2) = (Uuid::new_v4(), Uuid::new_v4());
        let panes = Pane::Leaf(p1).insert(p1, p2, false, false);
        let mut tabs = tabs_with(panes, Vec::new());

        tabs.prune();

        assert!(matches!(tabs.panes, Pane::Leaf(_)));
        assert_eq!(leaves(&tabs.panes).len(), 1);
    }

    // The per-pane active tab is forgotten when that tab no longer exists.
    #[test]
    fn prune_drops_the_active_entries_of_removed_tabs() {
        let (p1, p2, v) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let panes = Pane::Leaf(p1).insert(p1, p2, false, false);
        let kept = host_tab(v, p2);
        let mut tabs = tabs_with(panes, vec![kept]);

        tabs.pane_active.insert(p1, Uuid::new_v4());
        tabs.pane_active.insert(p2, kept.id);
        tabs.prune();

        assert_eq!(tabs.pane_active.len(), 1);
        assert_eq!(tabs.pane_active.get(&p2), Some(&kept.id));
    }

    // Closing the active tab shows the one to its right in the same pane.
    #[test]
    fn remove_active_shows_the_tab_to_its_right() {
        let (p, v) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c) = (host_tab(v, p), host_tab(v, p), host_tab(v, p));
        let mut tabs = tabs_with(Pane::Leaf(p), vec![a, b, c]);

        tabs.activate(b.id);
        tabs.remove(&[b.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(c.id));
        assert_eq!(tabs.pane_active.get(&p), Some(&c.id));
    }

    // With no tab to its right, closing the active tab shows the one to its left.
    #[test]
    fn remove_active_without_a_right_neighbour_shows_the_left_one() {
        let (p, v) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c) = (host_tab(v, p), host_tab(v, p), host_tab(v, p));
        let mut tabs = tabs_with(Pane::Leaf(p), vec![a, b, c]);

        tabs.activate(c.id);
        tabs.remove(&[c.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(b.id));
    }

    // Tabs closed together are never chosen as the next one.
    #[test]
    fn remove_active_skips_neighbours_that_are_also_removed() {
        let (p, v) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c) = (host_tab(v, p), host_tab(v, p), host_tab(v, p));
        let mut tabs = tabs_with(Pane::Leaf(p), vec![a, b, c]);

        tabs.activate(b.id);
        tabs.remove(&[b.id, c.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(a.id));
    }

    // When the active tab's pane is left empty, the tab another pane was showing is chosen.
    #[test]
    fn remove_active_of_an_emptied_pane_shows_what_another_pane_showed() {
        let (p1, p2, v) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let panes = Pane::Leaf(p1).insert(p1, p2, false, false);
        let (a, b, c) = (host_tab(v, p1), host_tab(v, p2), host_tab(v, p2));
        let mut tabs = tabs_with(panes, vec![a, b, c]);

        tabs.pane_active.insert(p2, c.id);
        tabs.activate(a.id);
        tabs.remove(&[a.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(c.id));
        assert_eq!(tabs.panes, Pane::Leaf(p2));
    }

    // Closing a tab that is not the active one leaves the active tab alone.
    #[test]
    fn remove_a_tab_that_is_not_active_keeps_the_active_one() {
        let (p, v) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c) = (host_tab(v, p), host_tab(v, p), host_tab(v, p));
        let mut tabs = tabs_with(Pane::Leaf(p), vec![a, b, c]);

        tabs.activate(a.id);
        tabs.remove(&[b.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(a.id));
        assert_eq!(tabs.list.len(), 2);
    }

    // Tabs of another vault are not visible, so they are never chosen as the next one.
    #[test]
    fn remove_never_chooses_a_tab_of_another_vault() {
        let (p, v, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (a, hidden, c) = (host_tab(v, p), host_tab(other, p), host_tab(v, p));
        let mut tabs = tabs_with(Pane::Leaf(p), vec![a, hidden, c]);

        tabs.activate(a.id);
        tabs.remove(&[a.id], scope(Some(v)));

        assert_eq!(tabs.active, Some(c.id));

        // With nothing else visible, nothing is shown (the hidden tab stays open).
        tabs.remove(&[c.id], scope(Some(v)));

        assert_eq!(tabs.active, None);
        assert_eq!(tabs.list, vec![hidden]);
    }
}
