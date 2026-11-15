// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Terminal of a tab: paints the screen of the `Emulator` from `sessions` and passes it what is typed,
//! the mouse and the size. The SSH session does not depend on this view: it runs apart and writes to the emulator.
//!
//! Text arrives through GPUI's input handler (this is how dead keys work, ´ + a = á, and the
//! IME); special keys and shortcuts with Ctrl or Alt go through `on_key_down`.

use crate::app::prefs::Prefs;
use crate::app::state::{AppState, TabEvent};
use crate::app::theme::{terminal_background, terminal_palette};
use crate::components::{Button, ButtonSize, ContextMenuExt, PopupMenuItem};
use crate::features::terminal::notice::notice;
use gpui_kit::assets::IconName;
use gpui_kit::base::input::{Copy, Paste, SelectAll};
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, Font, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, Rgba, ScrollEvent, SharedString, StrikethroughStyle, Styled, Subscription,
    TextRun, UTF16Selection, UnderlineStyle, Window, canvas, div, fill, point, px, size,
};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use terminal::{Emulator, Key, Modifiers, Rgb8, encode_key, encode_mouse};
use uuid::Uuid;

/// Margin around the terminal, like the `p-2` of the web version.
const PADDING: f32 = 8.;

/// Size of a cell, measured with the terminal font, and of the font size it was measured with.
#[derive(Clone, Copy, Default)]
struct Cell {
    width: Pixels,
    height: Pixels,
    font_size: Pixels,
}

pub struct TerminalView {
    state: Entity<AppState>,
    tab: Uuid,
    emulator: Option<Arc<Mutex<Emulator>>>,
    focus: FocusHandle,
    font: Font,
    /// The built-in one, unless another is picked in Settings.
    default_family: SharedString,
    /// The settings `cell` was measured with: if they change, it is measured again.
    measured: Option<terminal::Settings>,
    cell: Cell,
    /// Where the grid sits in the window (set by `canvas` when painting).
    origin: Point<Pixels>,
    /// Mouse selection in progress.
    selecting: bool,
    /// Whether it was in the foreground last time: when it comes back, it takes focus.
    was_active: bool,
    _subs: Vec<Subscription>,
}

impl TerminalView {
    pub fn new(
        state: Entity<AppState>,
        tab: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let emulator = state.read(cx).sessions.terminal(tab);
        let subs = vec![
            cx.subscribe(&state, move |_, _, event: &TabEvent, cx| {
                if matches!(event, TabEvent::Screen(id) if *id == tab) {
                    cx.notify();
                }
            }),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];

        // The web version's font if installed; otherwise the system monospace one.
        let names = window.text_system().all_font_names();
        let family: SharedString = if names.iter().any(|n| n == "Cascadia Mono") {
            "Cascadia Mono".into()
        } else {
            let app: &App = cx;
            gpui_kit::component::ActiveTheme::theme(app)
                .mono_font_family
                .clone()
        };

        Self {
            state,
            tab,
            emulator,
            focus: cx.focus_handle(),
            font: gpui_kit::font(family.clone()),
            default_family: family,
            measured: None,
            cell: Cell::default(),
            origin: Point::default(),
            selecting: false,
            was_active: false,
            _subs: subs,
        }
    }

    fn measure(&mut self, settings: &terminal::Settings, window: &Window) {
        let family = settings
            .font_family
            .clone()
            .map(SharedString::from)
            .unwrap_or_else(|| self.default_family.clone());

        self.font = gpui_kit::font(family);

        let text = window.text_system();
        let id = text.resolve_font(&self.font);
        let size = px(settings.font_size);
        let width = text
            .advance(id, size, 'm')
            .map(|s| s.width)
            .unwrap_or(size * 0.6);

        self.cell = Cell {
            width,
            height: (size * 1.2).round(),
            font_size: size,
        };
        self.measured = Some(settings.clone());
    }

    fn write(&self, bytes: Vec<u8>, cx: &App) {
        if let Some(emulator) = &self.emulator {
            // Typing scrolls back to the bottom, as in any terminal.
            emulator.lock().unwrap().scroll_to_bottom();
        }

        self.state.read(cx).sessions.write(self.tab, bytes);
    }

    fn with_emulator<R>(&self, f: impl FnOnce(&mut Emulator) -> R) -> Option<R> {
        self.emulator.as_ref().map(|e| f(&mut e.lock().unwrap()))
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.with_emulator(|e| e.selection_text()).flatten() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };

        if let Some(bytes) = self.with_emulator(|e| e.paste(&text)) {
            self.write(bytes, cx);
        }
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.with_emulator(|e| e.select_all());
        cx.notify();
    }

    /// Clears the history and leaves only the current line.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.with_emulator(|e| e.clear());
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        // AltGr on Windows arrives as Ctrl+Alt: then the character wins over the shortcut.
        let ctrl = m.control && !event.prefer_character_input;
        let alt = m.alt && !event.prefer_character_input;
        let mods = Modifiers {
            shift: m.shift,
            ctrl,
            alt,
        };
        let key = keystroke.key.as_str();

        // Ctrl+key always goes to the server, as in Termius or xterm. Copy and paste use Cmd on macOS
        // and Ctrl+Shift on Windows and Linux (also Ctrl/Shift+Insert).
        let clipboard = if cfg!(target_os = "macos") {
            m.platform && !m.control && !m.alt
        } else {
            ctrl && m.shift && !alt
        };

        if (clipboard && key == "c") || (ctrl && !m.shift && key == "insert") {
            self.copy(cx);
            self.with_emulator(|e| e.clear_selection());
            cx.notify();
            cx.stop_propagation();
            return;
        }

        if (clipboard && key == "v") || (m.shift && !ctrl && key == "insert") {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }

        let special = match key {
            "enter" => Some(Key::Enter),
            "backspace" => Some(Key::Backspace),
            "tab" => Some(Key::Tab),
            "escape" => Some(Key::Escape),
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            "home" => Some(Key::Home),
            "end" => Some(Key::End),
            "pageup" => Some(Key::PageUp),
            "pagedown" => Some(Key::PageDown),
            "insert" => Some(Key::Insert),
            "delete" => Some(Key::Delete),
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
                Some(Key::F(f[1..].parse().unwrap()))
            }
            _ => None,
        };

        // Shift+PageUp/PageDown scroll the history, as in xterm.
        if m.shift && !ctrl && matches!(special, Some(Key::PageUp | Key::PageDown)) {
            let lines = self.with_emulator(|e| e.lines()).unwrap_or(24) as i32;
            let delta = if special == Some(Key::PageUp) {
                lines
            } else {
                -lines
            };
            self.with_emulator(|e| e.scroll(delta));
            cx.notify();
            cx.stop_propagation();
            return;
        }

        // With Ctrl or Alt, the letter is a shortcut (^C, Alt+x…); without them, the text arrives through the input handler.
        let key = match special {
            Some(key) => key,
            None if (ctrl || alt) && key.chars().count() == 1 => Key::Text(key.to_string()),
            None => return,
        };

        let app_cursor = self.with_emulator(|e| e.app_cursor()).unwrap_or(false);

        if let Some(bytes) = encode_key(&key, mods, app_cursor) {
            self.with_emulator(|e| e.clear_selection());
            self.write(bytes, cx);
            cx.stop_propagation();
        }
    }

    /// Cell under a point of the window: line, column, and whether it falls in its right half.
    fn cell_at(&self, position: Point<Pixels>) -> (usize, usize, bool) {
        let x = ((position.x - self.origin.x) / self.cell.width).max(0.);
        let y = ((position.y - self.origin.y) / self.cell.height).max(0.);

        (y as usize, x as usize, x.fract() > 0.5)
    }

    /// The remote program wants the mouse and selection is not requested (Shift forces selection).
    fn reports_mouse(&self, shift: bool) -> bool {
        !shift && self.with_emulator(|e| e.mouse_mode()).unwrap_or(false)
    }

    fn mouse_button(button: MouseButton) -> Option<terminal::MouseButton> {
        match button {
            MouseButton::Left => Some(terminal::MouseButton::Left),
            MouseButton::Middle => Some(terminal::MouseButton::Middle),
            MouseButton::Right => Some(terminal::MouseButton::Right),
            _ => None,
        }
    }

    fn send_mouse(
        &self,
        button: terminal::MouseButton,
        pressed: bool,
        motion: bool,
        position: Point<Pixels>,
        m: gpui_kit::Modifiers,
        cx: &App,
    ) {
        let (line, column, _) = self.cell_at(position);
        let sgr = self.with_emulator(|e| e.sgr_mouse()).unwrap_or(true);
        let mods = Modifiers {
            shift: m.shift,
            ctrl: m.control,
            alt: m.alt,
        };

        if let Some(bytes) = encode_mouse(button, pressed, motion, line, column, mods, sgr) {
            self.state.read(cx).sessions.write(self.tab, bytes);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);

        if self.reports_mouse(event.modifiers.shift) {
            if let Some(button) = Self::mouse_button(event.button) {
                self.send_mouse(button, true, false, event.position, event.modifiers, cx);
            }

            return;
        }

        if event.button != MouseButton::Left {
            return;
        }

        let (line, column, right) = self.cell_at(event.position);
        let words = event.click_count >= 2;

        self.with_emulator(|e| e.start_selection(line, column, right, words));

        if words {
            self.with_emulator(|e| e.update_selection(line, column, true));
        }

        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting && event.dragging() {
            let (line, column, right) = self.cell_at(event.position);
            self.with_emulator(|e| e.update_selection(line, column, right));
            cx.notify();
        } else if event.dragging() && self.reports_mouse(event.modifiers.shift) {
            self.send_mouse(
                terminal::MouseButton::Left,
                true,
                true,
                event.position,
                event.modifiers,
                cx,
            );
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.selecting = false;

            // A click without dragging leaves no selection.
            if !self.with_emulator(|e| e.has_selection()).unwrap_or(false) {
                self.with_emulator(|e| e.clear_selection());
            }

            cx.notify();
        } else if self.reports_mouse(event.modifiers.shift)
            && let Some(button) = Self::mouse_button(event.button)
        {
            self.send_mouse(button, false, false, event.position, event.modifiers, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = event.delta.pixel_delta(self.cell.height).y;
        let lines = (dy / self.cell.height).round() as i32;

        if lines == 0 {
            return;
        }

        let (mouse, alt_screen, app_cursor) = self
            .with_emulator(|e| (e.mouse_mode(), e.alt_screen(), e.app_cursor()))
            .unwrap_or_default();

        if mouse {
            let button = if lines > 0 {
                terminal::MouseButton::WheelUp
            } else {
                terminal::MouseButton::WheelDown
            };

            for _ in 0..lines.unsigned_abs() {
                self.send_mouse(button, true, false, event.position, event.modifiers, cx);
            }
        } else if alt_screen {
            // Alternate screen (less, vim…): the wheel sends arrow keys.
            let key = if lines > 0 { Key::Up } else { Key::Down };

            for _ in 0..lines.unsigned_abs() {
                if let Some(bytes) = encode_key(&key, Modifiers::default(), app_cursor) {
                    self.state.read(cx).sessions.write(self.tab, bytes);
                }
            }
        } else {
            self.with_emulator(|e| e.scroll(lines));
            cx.notify();
        }
    }

    fn grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let focus = self.focus.clone();
        let emulator = self.emulator.clone();
        let (font, cell, tab) = (self.font.clone(), self.cell, self.tab);
        let sessions = self.state.read(cx).sessions.clone();

        canvas(
            move |bounds, _, _| {
                // Resizes to the available size: the next screen already arrives with it.
                let columns = (bounds.size.width / cell.width).floor().max(2.) as usize;
                let lines = (bounds.size.height / cell.height).floor().max(1.) as usize;

                sessions.resize(tab, columns, lines);
                bounds
            },
            move |bounds, _, window, cx| {
                view.update(cx, |this, _| this.origin = bounds.origin);
                window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);

                let Some(emulator) = emulator else {
                    return;
                };

                let screen = emulator.lock().unwrap().screen(&terminal_palette());
                let focused = focus.is_focused(window);
                paint_screen(&screen, &font, cell, bounds, focused, window, cx);
            },
        )
        .size_full()
    }
}

/// Paints the screen's backgrounds, text and cursor, cell by cell.
fn paint_screen(
    screen: &terminal::Screen,
    font: &Font,
    cell: Cell,
    bounds: Bounds<Pixels>,
    focused: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let color = |c: Rgb8| -> Hsla {
        Rgba {
            r: c.r as f32 / 255.,
            g: c.g as f32 / 255.,
            b: c.b as f32 / 255.,
            a: 1.,
        }
        .into()
    };

    for (i, line) in screen.lines.iter().enumerate() {
        let y = bounds.origin.y + cell.height * i as f32;

        for run in line {
            if let Some(bg) = run.bg {
                let x = bounds.origin.x + cell.width * run.column as f32;
                window.paint_quad(fill(
                    Bounds::new(
                        point(x, y),
                        size(cell.width * run.width as f32, cell.height),
                    ),
                    color(bg),
                ));
            }
        }

        let Some(first) = line.first() else {
            continue;
        };

        let text: String = line.iter().map(|r| r.text.as_str()).collect();
        let runs: Vec<TextRun> = line
            .iter()
            .map(|r| TextRun {
                len: r.text.len(),
                font: Font {
                    weight: if r.bold {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    style: if r.italic {
                        FontStyle::Italic
                    } else {
                        FontStyle::Normal
                    },
                    ..font.clone()
                },
                color: color(r.fg),
                background_color: None,
                underline: r.underline.then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: Some(color(r.fg)),
                    wavy: false,
                }),
                strikethrough: r.strike.then(|| StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(color(r.fg)),
                }),
            })
            .collect();

        // shortcut: `force_width` puts each character in one cell, so wide glyphs (CJK, emoji) come out
        // narrow; paint them separately if needed.
        let shaped =
            window
                .text_system()
                .shape_line(text.into(), cell.font_size, &runs, Some(cell.width));
        let x = bounds.origin.x + cell.width * first.column as f32;
        let _ = shaped.paint(
            point(x, y),
            cell.height,
            gpui_kit::TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    if let Some((line, column)) = screen.cursor {
        let origin = point(
            bounds.origin.x + cell.width * column as f32,
            bounds.origin.y + cell.height * line as f32,
        );
        let cursor = color(terminal_palette().cursor);

        let quad = if screen.thin_cursor {
            fill(Bounds::new(origin, size(px(2.), cell.height)), cursor)
        } else if focused {
            fill(
                Bounds::new(origin, size(cell.width, cell.height)),
                cursor.opacity(0.6),
            )
        } else {
            // Without focus, the block cursor is only an outline.
            gpui_kit::outline(
                Bounds::new(origin, size(cell.width, cell.height)),
                cursor,
                gpui_kit::BorderStyle::Solid,
            )
        };

        window.paint_quad(quad);
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Text typed by the user (and what the IME composes) goes straight to the server. The terminal has no
/// editable text of its own, so the ranges and the selection are always empty.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            return;
        }

        self.with_emulator(|e| e.clear_selection());
        self.write(text.as_bytes().to_vec(), cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        _: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        // Text still being composed is not sent: it arrives whole in `replace_text_in_range`.
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The IME window appears next to the terminal cursor.
        let (line, column) = self
            .with_emulator(|e| e.screen(&terminal_palette()).cursor)
            .flatten()
            .unwrap_or((0, 0));

        Some(Bounds::new(
            point(
                bounds.origin.x + self.cell.width * column as f32,
                bounds.origin.y + self.cell.height * line as f32,
            ),
            size(self.cell.width, self.cell.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &Prefs::get(cx).terminal;

        if self.measured.as_ref() != Some(settings) {
            let settings = settings.clone();
            self.measure(&settings, window);
        }

        let active = self.state.read(cx).tabs.active == Some(self.tab);

        if active && !self.was_active {
            window.focus(&self.focus, cx);
        }

        self.was_active = active;

        let info = self
            .state
            .read(cx)
            .session_list
            .iter()
            .find(|t| t.id == self.tab)
            .cloned();
        let view = cx.entity();

        // Button next to the cursor that fills in the `sudo` password with the method's. It is never
        // sent on its own: the prompt is text, and any program on the server can imitate it.
        let sudo = self
            .state
            .read(cx)
            .sessions
            .sudo_prompt(self.tab)
            .then(|| self.with_emulator(|e| e.cursor()).flatten())
            .flatten()
            .map(|(line, column)| {
                let (state, tab, focus) = (self.state.clone(), self.tab, self.focus.clone());

                // Above and to the right of the cursor, which sits in its bottom-left corner. On the
                // first line there is no room: it goes below, with the cursor in the top-left corner.
                let x = px(PADDING) + self.cell.width * (column + 1) as f32;
                let y = px(PADDING) + self.cell.height * line as f32;
                let top = if y >= px(24.) {
                    y - px(24.)
                } else {
                    y + self.cell.height
                };

                div()
                    .absolute()
                    .left(x)
                    .top(top)
                    .rounded(px(6.))
                    .shadow_md()
                    // So the click does not start a selection in the terminal below.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Button::new("fill-sudo")
                            .size(ButtonSize::Small)
                            .icon(IconName::KeyRound)
                            .label("Fill in password")
                            .on_click(move |_, window, cx| {
                                state.read(cx).sessions.fill_sudo(tab);
                                window.focus(&focus, cx);
                            }),
                    )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(terminal_background())
            .children(info.map(|info| notice(&self.state, &info, cx)))
            .child(
                div()
                    .id("terminal")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .p(px(PADDING))
                    .key_context("Terminal")
                    .track_focus(&self.focus)
                    .cursor_text()
                    .on_key_down(cx.listener(Self::on_key_down))
                    // The app's Edit menu actions.
                    .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy(cx)))
                    .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste(cx)))
                    .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
                    .on_scroll(cx.listener(Self::on_scroll))
                    .child(self.grid(cx))
                    .children(sudo)
                    .context_menu(move |menu, _, _| {
                        let (copy, all, clear, reconnect) =
                            (view.clone(), view.clone(), view.clone(), view.clone());

                        menu.min_w(px(200.))
                            .item(PopupMenuItem::new("Copy").icon(IconName::Copy).on_click(
                                move |_, _, cx| copy.update(cx, |this, cx| this.copy(cx)),
                            ))
                            .item(
                                PopupMenuItem::new("Select all")
                                    .icon(IconName::SquareDashed)
                                    .on_click(move |_, _, cx| {
                                        all.update(cx, |this, cx| this.select_all(cx))
                                    }),
                            )
                            .item(PopupMenuItem::new("Clear").icon(IconName::Eraser).on_click(
                                move |_, _, cx| clear.update(cx, |this, cx| this.clear(cx)),
                            ))
                            .item(
                                PopupMenuItem::new("Reconnect")
                                    .icon(IconName::RefreshCw)
                                    .on_click(move |_, _, cx| {
                                        reconnect.update(cx, |this, cx| {
                                            let tab = this.tab;
                                            this.state.update(cx, |s, _| s.reconnect(tab));
                                        })
                                    }),
                            )
                    }),
            )
    }
}
