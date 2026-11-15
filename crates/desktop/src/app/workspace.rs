// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The window: title bar and, below it, the vault list, the unlock screen or the open vault.
//! Keeps mounted the stateful views of each tab (terminal, SFTP, key detail,
//! vault settings): they are created when the tab opens and released when it closes.

use std::collections::HashMap;

use crate::components::WindowExt;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, div, relative,
};
use sessions::TabKind;
use uuid::Uuid;

use crate::app::prefs::Prefs;
use crate::app::state::{AppState, Route};
use crate::app::tabs::{Edge, Pane, Tab};
use crate::app::theme::{colors, px};
use crate::features::hosts::detail::host_detail;
use crate::features::hosts::known::known_hosts;
use crate::features::hosts::method_picker::open_method_picker;
use crate::features::keys::detail::KeyDetail;
use crate::features::licenses::Licenses;
use crate::features::settings::SettingsView;
use crate::features::sftp::view::SftpView;
use crate::features::terminal::view::TerminalView;
use crate::features::tunnels::detail::tunnel_detail;
use crate::features::vaults::lost::open_lost_alert;
use crate::features::vaults::picker::VaultPicker;
use crate::features::vaults::settings::VaultSettings;
use crate::features::vaults::unlock::Unlock;
use crate::layout::sidebar::Sidebar;
use crate::layout::tab_bar::{DragTab, tab_bar};
use crate::layout::{self, title_bar};

/// The zone of a pane where the mouse is (in fractions of the pane): within a quarter of
/// an edge, that edge; otherwise, the center. Near a corner, the nearest edge wins.
fn edge_at(x: f32, y: f32) -> Edge {
    let edges = [
        (x, Some((false, true))),
        (1. - x, Some((false, false))),
        (y, Some((true, true))),
        (1. - y, Some((true, false))),
    ];

    edges
        .into_iter()
        .filter(|(d, _)| *d < 0.25)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .and_then(|(_, edge)| edge)
}

pub struct Workspace {
    pub state: Entity<AppState>,
    picker: Entity<VaultPicker>,
    /// Always mounted, so unsaved settings survive switching tabs. It is painted in its
    /// tab or, outside a vault, full screen; never in both places at the same time.
    settings: Entity<SettingsView>,
    sidebar: Entity<Sidebar>,
    /// One unlock per vault, so what was typed is not lost when switching vaults.
    unlocks: HashMap<Uuid, Entity<Unlock>>,
    /// All mounted: switching tabs does not restart any terminal or lose an SFTP folder.
    terminals: HashMap<Uuid, Entity<TerminalView>>,
    sftps: HashMap<Uuid, Entity<SftpView>>,
    /// Per tab (`OpenTab::id`): the key detail, with its "Copied" notice.
    key_details: HashMap<Uuid, Entity<KeyDetail>>,
    /// Per tab: typed passwords last as long as the tab does. Locking the vault
    /// closes it, so they do not survive the lock.
    vault_settings: HashMap<Uuid, Entity<VaultSettings>>,
    /// The licenses, while their tab or screen is open (there are almost 900 entries).
    licenses: Option<Entity<Licenses>>,
    _observe: Subscription,
    _appearance: Subscription,
}

impl Workspace {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe_in(&state, window, |this, state, window, cx| {
            this.sync(&state, window, cx);
            cx.notify();
        });

        // With the "System" mode, the theme follows the system. The window's appearance is read: on Linux the app's
        // one does not always notice.
        Prefs::set_system(window.appearance(), cx);

        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Prefs::set_system(window.appearance(), cx);
        });

        Self {
            picker: cx.new(|cx| VaultPicker::new(state.clone(), window, cx)),
            settings: cx.new(|cx| SettingsView::new(state.clone(), window, cx)),
            sidebar: cx.new(|cx| Sidebar::new(state.clone(), window, cx)),
            state,
            unlocks: HashMap::new(),
            terminals: HashMap::new(),
            sftps: HashMap::new(),
            key_details: HashMap::new(),
            vault_settings: HashMap::new(),
            licenses: None,
            _observe: observe,
            _appearance: appearance,
        }
    }

    /// Creates views for new tabs, drops those of closed tabs and alerts about lost vaults.
    fn sync(&mut self, state: &Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) {
        let (tabs, sessions) = {
            let s = state.read(cx);
            (s.tabs.list.clone(), s.session_list.clone())
        };
        let open = |id: &Uuid| tabs.iter().any(|t| t.id == *id);

        self.terminals.retain(|id, _| open(id));
        self.sftps.retain(|id, _| open(id));
        self.key_details.retain(|id, _| open(id));
        self.vault_settings.retain(|id, _| open(id));

        for tab in &tabs {
            match tab.tab {
                Tab::Session(id) => match sessions.iter().find(|i| i.id == id).map(|i| i.kind) {
                    Some(TabKind::Terminal) if !self.terminals.contains_key(&id) => {
                        let view = cx.new(|cx| TerminalView::new(state.clone(), id, window, cx));
                        self.terminals.insert(id, view);
                    }
                    Some(TabKind::Sftp) if !self.sftps.contains_key(&id) => {
                        let view = cx.new(|cx| SftpView::new(state.clone(), id, window, cx));
                        self.sftps.insert(id, view);
                    }
                    _ => {}
                },
                Tab::Key { vault, id } if !self.key_details.contains_key(&tab.id) => {
                    let state = state.clone();
                    let view = cx.new(|cx| KeyDetail::new(state, vault, id, cx));
                    self.key_details.insert(tab.id, view);
                }
                Tab::VaultSettings(vault) if !self.vault_settings.contains_key(&tab.id) => {
                    let state = state.clone();
                    let view = cx.new(|cx| VaultSettings::new(state, vault, window, cx));
                    self.vault_settings.insert(tab.id, view);
                }
                _ => {}
            }
        }

        // Unlocks for vaults that are already open are not needed.
        let unlocked: Vec<Uuid> = state.read(cx).views.keys().copied().collect();
        self.unlocks.retain(|id, _| !unlocked.contains(id));

        let shown =
            state.read(cx).route == Route::Licenses || tabs.iter().any(|t| t.tab == Tab::Licenses);

        match (shown, self.licenses.is_some()) {
            (true, false) => self.licenses = Some(cx.new(|cx| Licenses::new(window, cx))),
            (false, true) => self.licenses = None,
            _ => {}
        }

        let lost = state.update(cx, |s, _| std::mem::take(&mut s.lost));

        if !lost.is_empty() {
            open_lost_alert(lost, window, cx);
        }

        // Host with several methods: asks which one to connect with (once per request).
        if state.read(cx).picking.is_some() && !window.has_active_dialog(cx) {
            open_method_picker(state.clone(), window, cx);
        }
    }

    /// What is shown to the right of the side panel: the panes with their tabs or, if there are none, the
    /// empty state of the section (no bar, like an empty pane in Zed).
    fn content(&self, vault: Uuid, window: &mut Window, cx: &App) -> AnyElement {
        let s = self.state.read(cx);

        self.panes(&s.tabs.panes, cx)
            .unwrap_or_else(|| layout::empty(&self.state, vault, window, cx))
    }

    /// A tree of panes. Those with no tabs from this vault are not shown: their sibling takes up
    /// all the space.
    fn panes(&self, pane: &Pane, cx: &App) -> Option<AnyElement> {
        let (down, first, second) = match pane {
            Pane::Leaf(id) => return self.leaf(*id, cx),
            Pane::Split {
                down,
                first,
                second,
            } => (down, first, second),
        };

        match (self.panes(first, cx), self.panes(second, cx)) {
            (Some(a), Some(b)) => {
                let half = || div().flex_1().min_w_0().min_h_0().overflow_hidden();

                Some(
                    div()
                        .size_full()
                        .flex()
                        .when(*down, |this| this.flex_col())
                        .child(half().child(a))
                        .child(
                            half()
                                .border_color(colors().border)
                                .map(|this| {
                                    if *down {
                                        this.border_t_1()
                                    } else {
                                        this.border_l_1()
                                    }
                                })
                                .child(b),
                        )
                        .into_any_element(),
                )
            }
            (a, b) => a.or(b),
        }
    }

    /// A pane: its tab bar and what it shows. Clicking it gives it focus. While a tab is
    /// dragged over it, the spot where it will land is marked, as in Zed.
    fn leaf(&self, pane: Uuid, cx: &App) -> Option<AnyElement> {
        let s = self.state.read(cx);
        let id = s.shown(pane)?;
        let tab = s.tabs.list.iter().find(|t| t.id == id)?.tab;
        let c = colors();
        let (focus, track, drop) = (self.state.clone(), self.state.clone(), self.state.clone());

        // The half (or all of it, in the center) the tab will take: x, y, width and height in fractions.
        let preview = s
            .tabs
            .drop_target
            .filter(|(p, _)| *p == pane && cx.has_active_drag())
            .map(|(_, edge)| {
                let [x, y, w, h] = match edge {
                    Some((false, true)) => [0., 0., 0.5, 1.],
                    Some((false, false)) => [0.5, 0., 0.5, 1.],
                    Some((true, true)) => [0., 0., 1., 0.5],
                    Some((true, false)) => [0., 0.5, 1., 0.5],
                    None => [0., 0., 1., 1.],
                };

                div()
                    .absolute()
                    .left(relative(x))
                    .top(relative(y))
                    .w(relative(w))
                    .h(relative(h))
                    .bg(c.tab_drop.opacity(0.2))
                    .border_2()
                    .border_color(c.tab_drop.opacity(0.6))
            });

        Some(
            div()
                .size_full()
                .flex()
                .flex_col()
                // In the capture phase: the terminal and SFTP stop the click before it bubbles up.
                .capture_any_mouse_down(move |_, _, cx| {
                    focus.update(cx, |s, cx| {
                        if s.tabs.active != Some(id) {
                            s.show_tab(id, cx);
                        }
                    })
                })
                .child(tab_bar(&self.state, pane, cx))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        // Each pane is notified: the one under the mouse becomes the target
                        // and the others release theirs.
                        .on_drag_move::<DragTab>(move |e, _, cx| {
                            let b = e.bounds;
                            let p = e.event.position;

                            let target = b.contains(&p).then(|| {
                                let x = (p.x - b.left()) / b.size.width;
                                let y = (p.y - b.top()) / b.size.height;
                                (pane, edge_at(x, y))
                            });

                            track.update(cx, |s, cx| s.set_drop_target(pane, target, cx));
                        })
                        .on_drop(move |drag: &DragTab, _, cx| {
                            drop.update(cx, |s, cx| {
                                if let Some((_, edge)) =
                                    s.tabs.drop_target.filter(|(p, _)| *p == pane)
                                {
                                    s.drop_tab(drag.id, pane, edge, cx);
                                }
                            })
                        })
                        .child(self.pane(id, tab, cx))
                        .children(preview),
                )
                .into_any_element(),
        )
    }

    /// What a tab shows (`id` is its `OpenTab::id`).
    fn pane(&self, id: Uuid, tab: Tab, cx: &App) -> AnyElement {
        let element = |view: Option<AnyElement>| view.unwrap_or_else(|| div().into_any_element());

        match tab {
            Tab::Session(id) => element(
                self.terminals
                    .get(&id)
                    .map(|v| v.clone().into_any_element())
                    .or_else(|| self.sftps.get(&id).map(|v| v.clone().into_any_element())),
            ),
            Tab::Host { vault, id } => host_detail(&self.state, vault, id, cx),
            Tab::KnownHosts(vault) => known_hosts(&self.state, vault, cx),
            Tab::Tunnel { vault, id } => tunnel_detail(&self.state, vault, id, cx),
            Tab::Key { .. } => element(
                self.key_details
                    .get(&id)
                    .map(|v| v.clone().into_any_element()),
            ),
            Tab::VaultSettings(_) => element(
                self.vault_settings
                    .get(&id)
                    .map(|v| v.clone().into_any_element()),
            ),
            Tab::Settings => self.settings.clone().into_any_element(),
            Tab::Licenses => element(self.licenses.as_ref().map(|v| v.clone().into_any_element())),
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let state = self.state.read(cx);
        let route = state.route;

        let body = match route {
            Route::Picker => self.picker.clone().into_any_element(),
            Route::Settings => self.settings.clone().into_any_element(),
            Route::Licenses => match &self.licenses {
                Some(licenses) => licenses.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Route::Vault(vault, _) => {
                if state.item(vault).is_none() {
                    layout::not_found("The vault").into_any_element()
                } else if !state.views.contains_key(&vault) {
                    let unlock = self
                        .unlocks
                        .entry(vault)
                        .or_insert_with(|| {
                            let state = self.state.clone();
                            cx.new(|cx| Unlock::new(state, vault, window, cx))
                        })
                        .clone();

                    unlock.into_any_element()
                } else {
                    let content = self.content(vault, window, cx);

                    layout::vault(
                        &self.state,
                        vault,
                        self.sidebar.clone(),
                        content,
                        window,
                        cx,
                    )
                    .into_any_element()
                }
            }
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.background)
            .text_color(c.foreground)
            .text_size(px(13.))
            .child(title_bar::render(&self.state, window, cx))
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::edge_at;

    #[test]
    fn mouse_zone_in_a_pane() {
        assert_eq!(edge_at(0.1, 0.5), Some((false, true)));
        assert_eq!(edge_at(0.9, 0.5), Some((false, false)));
        assert_eq!(edge_at(0.5, 0.1), Some((true, true)));
        assert_eq!(edge_at(0.5, 0.9), Some((true, false)));
        assert_eq!(edge_at(0.5, 0.5), None);

        // In a corner, the nearest edge wins.
        assert_eq!(edge_at(0.05, 0.2), Some((false, true)));
    }
}
