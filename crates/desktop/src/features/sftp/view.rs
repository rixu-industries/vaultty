// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! SFTP tab view: the two panels and the transfer queue. Items are dragged between panels;
//! system files are dropped onto the remote panel.

use crate::app::state::{AppState, TabEvent};
use crate::app::theme::px;
use crate::app::theme::{colors, mono_font};
use crate::components::{Button, ButtonSize, ButtonVariant, DragLabel, confirm, empty_state, icon};
use crate::components::{ContextMenuExt, PopupMenuItem};
use crate::components::{Input, InputEvent, InputState};
use crate::features::sftp::conflict::open_conflict;
use crate::features::sftp::name_dialog::open_name_dialog;
use crate::features::sftp::{DragEntries, cell, format_date, grid_row, select};
use crate::features::terminal::notice::notice;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, AppContext, ClickEvent, Context, Entity,
    ExternalPaths, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    ease_in_out, relative,
};
use sessions::Sessions;
use sftp::{
    Column, Entry, EntryKind, JobState, QueueView, Side, Sort, format_size, join, sort_entries,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// One of the two panels: folder, listing, selection and sort order.
struct Panel {
    path: String,
    entries: Vec<Entry>,
    selected: Vec<String>,
    anchor: Option<String>,
    sort: Sort,
    show_hidden: bool,
    /// Last error; the previous listing is kept.
    error: Option<String>,
    loading: bool,
    /// Requested folder that has not arrived yet; a late listing of another folder is discarded.
    want: Option<String>,
    input: Entity<InputState>,
    focus: FocusHandle,
}

impl Panel {
    fn visible(&self) -> Vec<Entry> {
        sort_entries(&self.entries, self.sort, self.show_hidden)
    }

    fn selected_entries(&self) -> Vec<Entry> {
        self.visible()
            .into_iter()
            .filter(|e| self.selected.contains(&e.path))
            .collect()
    }
}

pub struct SftpView {
    state: Entity<AppState>,
    tab: Uuid,
    panels: [Panel; 2],
    queue: QueueView,
    queue_open: bool,
    /// Conflict question that is already being shown.
    asked: Option<Uuid>,
    connected: bool,
    _subs: Vec<Subscription>,
}

impl SftpView {
    pub fn new(
        state: Entity<AppState>,
        tab: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let panel = |window: &mut Window, cx: &mut Context<Self>| Panel {
            path: String::new(),
            entries: Vec::new(),
            selected: Vec::new(),
            anchor: None,
            sort: Sort::default(),
            show_hidden: false,
            error: None,
            loading: false,
            want: None,
            input: cx.new(|cx| InputState::new(window, cx)),
            focus: cx.focus_handle(),
        };

        let panels = [panel(window, cx), panel(window, cx)];

        let mut subs = vec![
            cx.subscribe_in(
                &state,
                window,
                move |this, _, event: &TabEvent, window, cx| match event {
                    TabEvent::Queue(id) if *id == tab => this.refresh_queue(window, cx),
                    TabEvent::Reload { tab: id, side } if *id == tab => this.reload(*side, cx),
                    _ => {}
                },
            ),
            cx.observe_in(&state, window, |this, _, window, cx| {
                this.sync_status(window, cx)
            }),
        ];

        for side in [Side::Local, Side::Remote] {
            let input = panels[side.index()].input.clone();

            subs.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        let path = input.read(cx).value().trim().to_string();
                        this.open(side, path, window, cx);
                    }
                },
            ));
        }

        let mut view = Self {
            state,
            tab,
            panels,
            queue: QueueView::default(),
            queue_open: false,
            asked: None,
            connected: false,
            _subs: subs,
        };

        view.sync_status(window, cx);
        view
    }

    fn sessions(&self, cx: &App) -> Arc<Sessions> {
        self.state.read(cx).sessions.clone()
    }

    fn panel(&mut self, side: Side) -> &mut Panel {
        &mut self.panels[side.index()]
    }

    /// Each connection (including reconnects) lists the panels again: each side goes back to its
    /// last folder or, if there is none or it can no longer be opened, to the home folder.
    fn sync_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connected = self
            .state
            .read(cx)
            .session_list
            .iter()
            .find(|t| t.id == self.tab)
            .is_some_and(|t| matches!(t.status, sessions::Status::Connected));

        if connected && !self.connected {
            for side in [Side::Local, Side::Remote] {
                self.start(side, window, cx);
            }

            self.refresh_queue(window, cx);
        }

        self.connected = connected;
        cx.notify();
    }

    fn start(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        let sessions = self.sessions(cx);
        let tab = self.tab;

        cx.spawn_in(window, async move |this, cx| {
            let start = sessions.spawn(sessions.clone().start_dir(tab, side)).await;

            match start {
                Ok(Ok(path)) => {
                    this.update_in(cx, |this, window, cx| {
                        this.open_or_home(side, path, window, cx)
                    })
                    .ok();
                }
                Ok(Err(e)) => {
                    this.update(cx, |this, cx| {
                        this.panel(side).error = Some(e);
                        cx.notify();
                    })
                    .ok();
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    /// Opens `path` and, if it can no longer be opened, the home folder.
    fn open_or_home(
        &mut self,
        side: Side,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sessions = self.sessions(cx);
        let tab = self.tab;
        let task = self.open_task(side, path, window, cx);

        cx.spawn_in(window, async move |this, cx| {
            if task.await {
                return;
            }

            if let Ok(Ok(home)) = sessions.spawn(sessions.clone().home(tab, side)).await {
                this.update_in(cx, |this, window, cx| this.open(side, home, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Opens `path`. If it fails, shows the error and leaves the panel as it was.
    fn open(&mut self, side: Side, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_task(side, path, window, cx).detach();
    }

    fn open_task(
        &mut self,
        side: Side,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<bool> {
        let sessions = self.sessions(cx);
        let tab = self.tab;
        let panel = self.panel(side);
        panel.loading = true;
        panel.want = Some(path.clone());
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = sessions
                .spawn(sessions.clone().open_dir(tab, side, path.clone()))
                .await;
            let result = result.unwrap_or_else(|e| Err(e.to_string()));
            let ok = result.is_ok();

            this.update_in(cx, |this, window, cx| {
                let panel = this.panel(side);

                // A listing that no longer matches (another folder was requested later) changes nothing.
                if panel.want.as_deref() != Some(path.as_str()) {
                    return;
                }

                match result {
                    Ok(entries) => {
                        panel.path = path.clone();
                        panel.entries = entries;
                        panel.selected.clear();
                        panel.anchor = None;
                        panel.error = None;
                        panel
                            .input
                            .update(cx, |i, cx| i.set_value(path, window, cx));
                    }
                    Err(e) => panel.error = Some(e),
                }

                panel.loading = false;
                panel.want = None;
                cx.notify();
            })
            .ok();

            ok
        })
    }

    /// Lists the current folder again, keeping the selection that still exists and the last error.
    fn reload(&mut self, side: Side, cx: &mut Context<Self>) {
        let sessions = self.sessions(cx);
        let tab = self.tab;
        let panel = self.panel(side);
        let path = panel.path.clone();
        panel.loading = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = sessions
                .spawn(sessions.clone().list_dir(tab, side, path.clone()))
                .await;

            this.update(cx, |this, cx| {
                let panel = this.panel(side);

                // If another folder was opened in the meantime, that opening is the one that turns the bar off.
                if panel.path != path || panel.want.is_some() {
                    return;
                }

                panel.loading = false;

                match result {
                    Ok(Ok(entries)) => {
                        let paths: HashSet<&String> = entries.iter().map(|e| &e.path).collect();
                        panel.selected.retain(|p| paths.contains(p));
                        panel.entries = entries;
                    }
                    Ok(Err(e)) => panel.error = Some(e),
                    Err(_) => {}
                }

                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn up(&mut self, side: Side, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(parent) = side.parent(&self.panel(side).path) {
            self.open(side, parent, window, cx);
        }
    }

    /// Runs a file operation, lists again and shows the error if there was one.
    fn run<F>(
        &mut self,
        side: Side,
        op: impl FnOnce(Arc<Sessions>, Uuid) -> F,
        cx: &mut Context<Self>,
    ) where
        F: Future<Output = Result<(), String>> + Send + 'static,
    {
        let sessions = self.sessions(cx);
        let task = sessions.spawn(op(sessions.clone(), self.tab));

        cx.spawn(async move |this, cx| {
            let error = task.await.unwrap_or_else(|e| Err(e.to_string())).err();

            this.update(cx, |this, cx| {
                this.reload(side, cx);
                this.panel(side).error = error;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copies `paths` from `from` into `to_dir` in the other panel. Asks about anything that already exists.
    fn enqueue(&mut self, from: Side, paths: Vec<String>, to_dir: String, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }

        let sessions = self.sessions(cx);
        let task = sessions.spawn(sessions.clone().enqueue(self.tab, from, paths, to_dir));

        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = task.await {
                this.update(cx, |this, cx| {
                    this.panel(from.other()).error = Some(e);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn refresh_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(queue) = self.sessions(cx).queue(self.tab) else {
            return;
        };

        self.queue = queue.view();

        match self.queue.asking.clone() {
            Some(ask) if self.asked != Some(ask.id) => {
                self.asked = Some(ask.id);
                open_conflict(self.state.clone(), self.tab, ask, window, cx);
            }
            None => self.asked = None,
            _ => {}
        }

        cx.notify();
    }

    fn click_row(
        &mut self,
        side: Side,
        path: String,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = self.panel(side);
        window.focus(&panel.focus, cx);

        if e.click_count() >= 2 {
            if let Some(entry) = panel
                .entries
                .iter()
                .find(|x| x.path == path)
                .filter(|x| x.is_dir())
            {
                let dir = entry.path.clone();
                self.open(side, dir, window, cx);
            }

            return;
        }

        let m = e.modifiers();
        let order: Vec<String> = panel.visible().into_iter().map(|e| e.path).collect();
        let (selected, anchor) = select(
            &panel.selected,
            &order,
            &path,
            m.control || m.platform,
            m.shift,
            panel.anchor.as_deref(),
        );

        panel.selected = selected;
        panel.anchor = Some(anchor);
        cx.notify();
    }

    fn on_key(
        &mut self,
        side: Side,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self.panel(side).selected_entries();

        match event.keystroke.key.as_str() {
            "enter" if selected.len() == 1 && selected[0].is_dir() => {
                self.open(side, selected[0].path.clone(), window, cx)
            }
            "f2" if selected.len() == 1 => {
                self.ask_name(side, Some(selected[0].clone()), window, cx)
            }
            "delete" | "backspace" if !selected.is_empty() => {
                self.ask_delete(side, selected, window, cx)
            }
            _ => return,
        }

        cx.stop_propagation();
    }

    fn ask_name(
        &mut self,
        side: Side,
        entry: Option<Entry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity();
        let dir = self.panel(side).path.clone();
        let title = if entry.is_some() {
            "Rename"
        } else {
            "New folder"
        };
        let initial = entry.as_ref().map(|e| e.name.clone()).unwrap_or_default();

        open_name_dialog(title, initial, window, cx, move |name, cx| {
            let to = join(&dir, &name);
            let entry = entry.clone();

            this.update(cx, |this, cx| match entry {
                Some(entry) => {
                    this.run(side, move |s, tab| s.rename(tab, side, entry.path, to), cx)
                }
                None => this.run(side, move |s, tab| s.mkdir(tab, side, to), cx),
            });
        });
    }

    fn ask_delete(
        &mut self,
        side: Side,
        entries: Vec<Entry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = match entries.as_slice() {
            [one] => format!("Delete {}", one.name),
            many => format!("Delete {} items", many.len()),
        };
        let what = entries
            .iter()
            .map(|e| format!("\"{}\"", e.name))
            .collect::<Vec<_>>()
            .join(", ");
        let dirs = if entries.iter().any(|e| e.kind == EntryKind::Dir) {
            " Folders are deleted with all their contents."
        } else {
            ""
        };
        let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
        let this = cx.entity();

        confirm(
            title,
            format!("{what} will be deleted.{dirs} This cannot be undone."),
            "Delete",
            window,
            cx,
            move |_, cx| {
                let paths = paths.clone();
                this.update(cx, |this, cx| {
                    this.run(side, move |s, tab| s.remove(tab, side, paths), cx)
                });
            },
        );
    }

    fn render_panel(&self, side: Side, title: String, cx: &mut Context<Self>) -> AnyElement {
        let c = colors();
        let panel = &self.panels[side.index()];
        let entries = panel.visible();
        let selected = panel.selected_entries();
        // On Windows, `""` is the list of drives: no folders are created there.
        let at_drives = panel.path.is_empty();
        let has_parent = side.parent(&panel.path).is_some();
        let this = cx.entity();
        let tab = self.tab;

        let path_bar = div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap(px(4.))
            .px(px(8.))
            .h(px(40.))
            .border_b_1()
            .border_color(c.border)
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(4.))
                    .max_w(px(160.))
                    .truncate()
                    .text_color(c.muted_foreground)
                    .child(title),
            )
            .child(
                Button::new(SharedString::from(format!("up-{side:?}")))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Icon)
                    .icon(IconName::CornerLeftUp)
                    .tooltip("Go up")
                    .disabled(!has_parent)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| this.update(cx, |this, cx| this.up(side, window, cx))
                    }),
            )
            .child(div().flex_1().min_w_0().child(Input::new(&panel.input)))
            .child(
                Button::new(SharedString::from(format!("reload-{side:?}")))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Icon)
                    .icon(IconName::RefreshCw)
                    .tooltip("Reload")
                    .on_click({
                        let this = this.clone();
                        move |_, _, cx| this.update(cx, |this, cx| this.reload(side, cx))
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("mkdir-{side:?}")))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Icon)
                    .icon(IconName::FolderPlus)
                    .tooltip("New folder")
                    .disabled(at_drives)
                    .on_click({
                        let this = this.clone();
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.ask_name(side, None, window, cx))
                        }
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("hidden-{side:?}")))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Icon)
                    .icon(if panel.show_hidden {
                        IconName::Eye
                    } else {
                        IconName::EyeOff
                    })
                    .tooltip(if panel.show_hidden {
                        "Hide hidden files"
                    } else {
                        "Show hidden files"
                    })
                    .on_click({
                        let this = this.clone();
                        move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                let panel = this.panel(side);
                                panel.show_hidden = !panel.show_hidden;
                                cx.notify();
                            })
                        }
                    }),
            );

        let error = panel.error.clone().map(|error| {
            let this = this.clone();

            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap(px(8.))
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(c.border)
                .text_color(c.destructive)
                .child(div().flex_1().min_w_0().truncate().child(error))
                .child(
                    Button::new(SharedString::from(format!("close-error-{side:?}")))
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Small)
                        .label("Close")
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                this.panel(side).error = None;
                                cx.notify();
                            })
                        }),
                )
        });

        let columns = [
            (Column::Name, "Name"),
            (Column::Size, "Size"),
            (Column::Modified, "Modified"),
            (Column::Perms, "Permissions"),
        ];

        let reduce_motion = cx.reduce_motion();

        let mut header = grid_row()
            .relative()
            .flex_shrink_0()
            .h(px(28.))
            .border_b_1()
            .border_color(c.border)
            .text_size(px(12.))
            .text_color(c.muted_foreground);

        for (column, label) in columns {
            let this = this.clone();
            let sorted = panel.sort.column == column;
            let desc = panel.sort.desc;

            // Clicking the header: the same column flips the order; another sorts by it ascending.
            header = header.child(
                cell(column).child(
                    div()
                        .flex()
                        .items_center()
                        .id(SharedString::from(format!("sort-{side:?}-{column:?}")))
                        .cursor_pointer()
                        .gap(px(4.))
                        .when(column == Column::Size, |this| this.justify_end().w_full())
                        .hover(|s| s.text_color(c.foreground))
                        .child(label)
                        .when(sorted, |this| {
                            this.child(icon(
                                if desc {
                                    IconName::ArrowDown
                                } else {
                                    IconName::ArrowUp
                                },
                                11.,
                                c.muted_foreground,
                            ))
                        })
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| {
                                let panel = this.panel(side);
                                panel.sort = Sort {
                                    column,
                                    desc: panel.sort.column == column && !panel.sort.desc,
                                };
                                cx.notify();
                            })
                        }),
                ),
            );
        }

        // Bar over the edge of the header while the listing arrives: it moves nothing when it appears.
        let loading_bar = div()
            .absolute()
            .left_0()
            .bottom(px(-1.))
            .h(px(2.))
            .bg(c.primary)
            .map(|bar| {
                if reduce_motion {
                    bar.w_full().into_any_element()
                } else {
                    bar.with_animation(
                        SharedString::from(format!("loading-{side:?}")),
                        Animation::new(Duration::from_millis(1200)).repeat(),
                        |bar, delta| {
                            let start = ease_in_out(((delta - 0.4) / 0.6).clamp(0., 1.));
                            let end = ease_in_out((delta / 0.6).clamp(0., 1.));
                            bar.left(relative(start)).right(relative(1. - end))
                        },
                    )
                    .into_any_element()
                }
            });

        header = header.when(panel.loading, |h| h.child(loading_bar));

        let mut list = div().flex().flex_col();

        if entries.is_empty() && !panel.loading {
            list = list
                .size_full()
                .child(empty_state(IconName::FolderOpen, "Empty folder", None));
        }

        for entry in &entries {
            list =
                list.child(self.render_row(side, entry, panel.selected.contains(&entry.path), cx));
        }

        let dir = panel.path.clone();
        let drop_this = this.clone();
        let os_this = this.clone();
        let menu_this = this.clone();
        let key_this = this.clone();
        let menu_selected = selected.clone();

        let body = div()
            .id(SharedString::from(format!("panel-{side:?}")))
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .id(SharedString::from(format!("panel-scroll-{side:?}")))
            .track_focus(&panel.focus)
            .on_key_down(move |e, window, cx| {
                key_this.update(cx, |this, cx| this.on_key(side, e, window, cx))
            })
            .child(list)
            // Dropping on the panel background copies into the current folder.
            .when(!at_drives, |this| {
                let dir = dir.clone();
                let os_dir = dir.clone();

                this.drag_over::<DragEntries>(move |style, drag, _, _| {
                    if drag.side != side && drag.tab == tab {
                        style.bg(c.highlight)
                    } else {
                        style
                    }
                })
                .on_drop(move |drag: &DragEntries, _, cx| {
                    if drag.side != side && drag.tab == tab {
                        let (from, paths, dir) = (drag.side, drag.paths.clone(), dir.clone());
                        drop_this.update(cx, |this, cx| this.enqueue(from, paths, dir, cx));
                    }
                })
                // System files are only dropped on the server panel.
                .when(side == Side::Remote, |this| {
                    this.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(c.highlight))
                        .on_drop(move |paths: &ExternalPaths, _, cx| {
                            let paths: Vec<String> = paths
                                .paths()
                                .iter()
                                .map(|p| p.to_string_lossy().into_owned())
                                .collect();
                            let dir = os_dir.clone();
                            os_this
                                .update(cx, |this, cx| this.enqueue(Side::Local, paths, dir, cx));
                        })
                })
            })
            .context_menu(move |menu, _, _| {
                let selected = menu_selected.clone();
                let this = menu_this.clone();
                let mut menu = menu.min_w(px(200.));

                if let [one] = selected.as_slice()
                    && one.is_dir()
                {
                    let (this, path) = (this.clone(), one.path.clone());
                    menu = menu.item(
                        PopupMenuItem::new("Open")
                            .icon(IconName::FolderInput)
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.open(side, path.clone(), window, cx)
                                })
                            }),
                    );
                }

                if let [one] = selected.as_slice() {
                    let (this, entry) = (this.clone(), one.clone());
                    menu = menu.item(
                        PopupMenuItem::new("Rename")
                            .icon(IconName::Pencil)
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.ask_name(side, Some(entry.clone()), window, cx)
                                })
                            }),
                    );
                }

                if !at_drives {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new("New folder")
                            .icon(IconName::FolderPlus)
                            .on_click(move |_, window, cx| {
                                this.update(cx, |this, cx| this.ask_name(side, None, window, cx))
                            }),
                    );
                }

                let reload = this.clone();
                menu = menu.item(
                    PopupMenuItem::new("Reload")
                        .icon(IconName::RefreshCw)
                        .on_click(move |_, _, cx| {
                            reload.update(cx, |this, cx| this.reload(side, cx))
                        }),
                );

                if !selected.is_empty() {
                    let (this, selected) = (this.clone(), selected.clone());
                    menu = menu.separator().item(
                        PopupMenuItem::new("Delete").icon(IconName::Trash).on_click(
                            move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    this.ask_delete(side, selected.clone(), window, cx)
                                })
                            },
                        ),
                    );
                }

                menu
            });

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(path_bar)
            .children(error)
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_row(
        &self,
        side: Side,
        entry: &Entry,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors();
        let this = cx.entity();
        let tab = self.tab;
        let path = entry.path.clone();
        let is_dir = entry.is_dir();

        let name = match entry.kind {
            EntryKind::Dir => IconName::Folder,
            EntryKind::Link | EntryKind::LinkDir => IconName::Link,
            EntryKind::File => IconName::File,
        };

        // The selection is dragged if the row is part of it; otherwise only the row.
        let panel = &self.panels[side.index()];
        let paths = if selected {
            panel.selected.clone()
        } else {
            vec![path.clone()]
        };
        let label = match paths.as_slice() {
            [one] => sftp::files::file_name(one),
            many => format!("{} items", many.len()),
        };
        let drag = DragEntries { tab, side, paths };

        let menu_this = this.clone();
        let click_this = this.clone();
        let drop_this = this.clone();
        let menu_path = path.clone();
        let click_path = path.clone();
        let drop_dir = path.clone();

        grid_row()
            .id(SharedString::from(format!("{side:?}:{path}")))
            .cursor_pointer()
            .h(px(28.))
            .when(selected, |this| this.bg(c.highlight))
            .when(!selected, |this| this.hover(|s| s.bg(c.list_hover)))
            .child(
                cell(Column::Name).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w_0()
                        .child(icon(
                            name,
                            13.,
                            if is_dir {
                                c.primary
                            } else {
                                c.muted_foreground
                            },
                        ))
                        .child(div().truncate().child(entry.name.clone())),
                ),
            )
            .child(
                cell(Column::Size)
                    .justify_end()
                    .text_color(c.muted_foreground)
                    .child(if is_dir {
                        String::new()
                    } else {
                        format_size(entry.size)
                    }),
            )
            .child(
                cell(Column::Modified)
                    .text_color(c.muted_foreground)
                    .truncate()
                    .child(format_date(entry.modified)),
            )
            .child(
                cell(Column::Perms)
                    .font_family(mono_font())
                    .text_size(px(11.))
                    .text_color(c.muted_foreground)
                    .child(entry.perms.clone()),
            )
            .on_click(move |e, window, cx| {
                click_this.update(cx, |this, cx| {
                    this.click_row(side, click_path.clone(), e, window, cx)
                })
            })
            // Right-click on an unselected row: it becomes the only selected one.
            .on_mouse_down(gpui_kit::MouseButton::Right, move |_, _, cx| {
                menu_this.update(cx, |this, cx| {
                    let panel = this.panel(side);

                    if !panel.selected.contains(&menu_path) {
                        panel.selected = vec![menu_path.clone()];
                        panel.anchor = Some(menu_path.clone());
                        cx.notify();
                    }
                })
            })
            .on_drag(drag, move |_, cursor_offset, _, cx| {
                let label = label.clone();
                cx.new(|_| DragLabel::new(label, cursor_offset))
            })
            // Dropping on a folder copies into it.
            .when(is_dir, |this| {
                this.drag_over::<DragEntries>(move |style, drag, _, _| {
                    if drag.side != side && drag.tab == tab {
                        style.bg(c.highlight).border_1().border_color(c.list_drop)
                    } else {
                        style
                    }
                })
                .on_drop(move |drag: &DragEntries, _, cx| {
                    if drag.side != side && drag.tab == tab {
                        cx.stop_propagation();
                        let (from, paths, dir) = (drag.side, drag.paths.clone(), drop_dir.clone());
                        drop_this.update(cx, |this, cx| this.enqueue(from, paths, dir, cx));
                    }
                })
            })
            .into_any_element()
    }

    fn render_queue(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors();

        // Folders are only created: they add nothing to the list.
        let files: Vec<_> = self
            .queue
            .jobs
            .iter()
            .filter(|j| !j.item.dir || j.state == JobState::Failed)
            .cloned()
            .collect();

        if files.is_empty() {
            return None;
        }

        let active = files.iter().filter(|j| j.live()).count();
        let progress = self.queue.progress();
        let this = cx.entity();
        let tab = self.tab;
        let state = self.state.clone();

        let header = div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .gap(px(8.))
            .px(px(12.))
            .h(px(32.))
            .child(
                Button::new("queue-toggle")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Small)
                    .gap(px(6.))
                    .child(icon(
                        if self.queue_open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronUp
                        },
                        13.,
                        c.muted_foreground,
                    ))
                    .child(if active > 0 {
                        format!("Transfers ({active} pending)")
                    } else {
                        "Transfers".into()
                    })
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| {
                            this.queue_open = !this.queue_open;
                            cx.notify();
                        })
                    }),
            )
            .when(active > 0, |row| {
                row.child(
                    div()
                        .flex_1()
                        .max_w(px(160.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(c.secondary)
                        .overflow_hidden()
                        .child(div().h_full().w(relative(progress)).bg(c.primary)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c.muted_foreground)
                        .child(format!("{:.0}%", progress * 100.)),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .ml_auto()
                    .gap(px(4.))
                    .when(active > 0, |row| {
                        let state = state.clone();
                        row.child(
                            Button::new("cancel-all")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Small)
                                .label("Cancel all")
                                .on_click(move |_, _, cx| {
                                    state.read(cx).sessions.cancel(tab, None)
                                }),
                        )
                    })
                    .when(active < files.len(), |row| {
                        let state = state.clone();
                        row.child(
                            Button::new("clear-finished")
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Small)
                                .label("Clear finished")
                                .on_click(move |_, _, cx| {
                                    state.read(cx).sessions.clear_finished(tab)
                                }),
                        )
                    }),
            );

        let mut list = div()
            .flex()
            .flex_col()
            .id("queue-list")
            .overflow_y_scrollbar();

        if self.queue_open {
            for job in files.iter() {
                let name = sftp::files::file_name(&job.item.src);
                let pct = if job.item.size > 0 {
                    (job.bytes as f32 / job.item.size as f32).min(1.)
                } else {
                    0.
                };
                let state = state.clone();
                let id = job.id;

                let status = if job.state == JobState::Running {
                    div()
                        .flex()
                        .items_center()
                        .flex_1()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .h(px(6.))
                                .rounded_full()
                                .bg(c.secondary)
                                .overflow_hidden()
                                .child(div().h_full().w(relative(pct)).bg(c.primary)),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(12.))
                                .text_color(c.muted_foreground)
                                .child(format!(
                                    "{:.0}% · {} / {}",
                                    pct * 100.,
                                    format_size(job.bytes),
                                    format_size(job.item.size)
                                )),
                        )
                        .into_any_element()
                } else {
                    let label = match job.state {
                        JobState::Waiting => "Waiting",
                        JobState::Done => "Done",
                        JobState::Failed => "Failed",
                        JobState::Cancelled => "Cancelled",
                        JobState::Skipped => "Skipped",
                        JobState::Running => "",
                    };
                    let detail = match (&job.error, job.state) {
                        (Some(e), s) if s != JobState::Cancelled => format!("{label}: {e}"),
                        _ => label.to_string(),
                    };

                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if job.state == JobState::Failed {
                            c.destructive
                        } else {
                            c.muted_foreground
                        })
                        .child(detail)
                        .into_any_element()
                };

                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(12.))
                        .h(px(28.))
                        .border_t_1()
                        .border_color(c.border_subtle)
                        .child(icon(
                            if job.from == Side::Local {
                                IconName::ArrowUp
                            } else {
                                IconName::ArrowDown
                            },
                            12.,
                            c.muted_foreground,
                        ))
                        .child(div().w(relative(0.3)).min_w_0().truncate().child(name))
                        .child(status)
                        .child(
                            Button::new(SharedString::from(format!("cancel-{id}")))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::Icon)
                                .icon(IconName::X)
                                .tooltip("Cancel")
                                .when(!job.live(), |b| b.disabled(true))
                                .on_click(move |_, _, cx| {
                                    state.read(cx).sessions.cancel(tab, Some(id))
                                }),
                        ),
                );
            }
        }

        Some(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .max_h(px(220.))
                .border_t_1()
                .border_color(c.border)
                .bg(c.card)
                .child(header)
                .child(list)
                .into_any_element(),
        )
    }
}

impl Render for SftpView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let s = self.state.read(cx);
        let info = s.session_list.iter().find(|t| t.id == self.tab).cloned();

        let remote = info
            .as_ref()
            .and_then(|t| {
                let host = s
                    .views
                    .get(&t.vault)?
                    .hosts
                    .iter()
                    .find(|h| h.id == t.host)?;
                let method = host.methods.iter().find(|m| m.id == t.method)?;
                Some(format!("{}@{}", method.username, host.name))
            })
            .unwrap_or_else(|| "Server".into());

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(c.background)
            .children(info.map(|info| notice(&self.state, &info, cx)))
            .when(self.connected, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_1()
                        .min_h_0()
                        .items_stretch()
                        .child(self.render_panel(Side::Local, "Local".into(), cx))
                        .child(div().w(px(1.)).bg(c.border))
                        .child(self.render_panel(Side::Remote, remote, cx)),
                )
                .children(self.render_queue(cx))
            })
    }
}
