// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Colors of the active theme (`themes` crate) and their handoff to the gpui-kit theme, so its components
//! (buttons, fields, menus, dialogs) look the same as the app's own.
//!
//! Colors are stored on the UI thread and read with `colors()` in any `render`: this way
//! views don't need `cx` to paint, and changing the theme only requires repainting the windows.

use std::cell::{Cell, RefCell};

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Pixels, Rgba, SharedString, px as pixels};
use themes::Appearance;

/// The app's colors, one for each place that gets painted (the tokens of `themes::Ui`). Read with
/// `colors()` en cualquier `render`.
pub type Colors = themes::Ui<Hsla>;

/// Interface size that all view measurements are written at.
const BASE_UI_SIZE: f32 = 13.;

/// Interface measurement, written at the default size (13). It is multiplied by the size chosen in
/// Settings, so raising it grows everything at once (text, rows, icons, margins), like Zed's
/// `ui_font_size`. gpui-kit scales on its own: it sets the window's rem to `theme.font_size`,
/// which is the chosen size. Views import this instead of gpui's `px`; where a measurement must not
/// scale (the terminal, which has its own size) `gpui_kit::px` is used.
pub fn px(v: f32) -> Pixels {
    pixels(v * SCALE.get())
}

/// The opposite of `px`: a window measurement, written at the default size. For storing
/// widths that stay valid if the interface size changes.
pub fn unscaled(v: Pixels) -> f32 {
    f32::from(v) / SCALE.get()
}

thread_local! {
    static CURRENT: Cell<Option<(Colors, terminal::Palette, Hsla)>> = const { Cell::new(None) };
    static SCALE: Cell<f32> = const { Cell::new(1.) };
    static MONO: RefCell<SharedString> = RefCell::new(SharedString::default());
}

/// System monospace font (the one gpui-kit resolves: Menlo, Consolas, DejaVu Sans Mono…),
/// for fingerprints, keys and permissions. Writing the name by hand only works on one system.
pub fn mono_font() -> SharedString {
    MONO.with_borrow(|m| m.clone())
}

pub fn hsla(c: themes::Color) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.,
        g: c.g as f32 / 255.,
        b: c.b as f32 / 255.,
        a: c.a as f32 / 255.,
    }
    .into()
}

fn rgb8(c: themes::Color) -> terminal::Rgb8 {
    terminal::Rgb8 {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

fn current() -> (Colors, terminal::Palette, Hsla) {
    CURRENT
        .get()
        .expect("the theme is applied at startup, before painting")
}

pub fn colors() -> Colors {
    current().0
}

pub fn terminal_palette() -> terminal::Palette {
    current().1
}

/// The terminal background may differ from the app's (theme with its own terminal).
pub fn terminal_background() -> Hsla {
    current().2
}

/// Sets a theme: in the app's own colors and in gpui-kit's. Repaints all windows.
pub fn apply(theme: &themes::Theme, ui_font_size: f32, cx: &mut App) {
    let c: Colors = theme.ui.map(hsla);

    let t = &theme.terminal;
    let palette = terminal::Palette {
        background: rgb8(t.background),
        foreground: rgb8(t.foreground),
        cursor: rgb8(t.cursor),
        selection: rgb8(t.selection),
        ansi: t.ansi.map(rgb8),
    };

    CURRENT.set(Some((c, palette, hsla(t.background))));
    SCALE.set(ui_font_size / BASE_UI_SIZE);

    // gpui-kit's mode decides its starting colors (and the scrollbar's); they are overwritten afterwards.
    let mode = match theme.appearance {
        Appearance::Dark => ThemeMode::Dark,
        Appearance::Light => ThemeMode::Light,
    };

    if Theme::global(cx).mode != mode {
        Theme::change(mode, None, cx);
    }

    // `update` and not `global_mut`: this way the `tokens` (which paint buttons and menus) follow `colors`, and
    // also repaints the windows.
    Theme::update(cx, |theme| {
        theme.font_size = pixels(ui_font_size);
        theme.mono_font_size = pixels(ui_font_size - 1.);
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.shadow = false;
        theme.focus_ring = false;

        let t = &mut theme.colors;
        t.background = c.background;
        t.foreground = c.foreground;
        t.border = c.border;
        t.input = c.input;
        t.ring = c.ring;
        t.caret = c.foreground;
        t.primary = c.primary;
        t.primary_hover = c.primary_hover;
        t.primary_active = c.primary_active;
        t.primary_foreground = c.primary_foreground;
        t.secondary = c.secondary;
        t.secondary_hover = c.secondary_hover;
        t.secondary_active = c.secondary_hover;
        t.secondary_foreground = c.secondary_foreground;
        t.muted = c.muted;
        t.muted_foreground = c.muted_foreground;
        t.accent = c.accent;
        t.accent_foreground = c.accent_foreground;
        t.popover = c.popover;
        t.popover_foreground = c.popover_foreground;
        t.danger = c.destructive;
        t.danger_hover = c.destructive_hover;
        t.danger_active = c.destructive_active;
        t.danger_foreground = c.destructive_foreground;
        t.success = c.success;
        t.warning = c.warning;
        t.list = c.background;
        t.list_hover = c.list_hover;
        t.list_active = c.list_active;
        t.title_bar = c.titlebar_background;
        t.title_bar_border = c.titlebar_border;
        t.status_bar = c.statusbar_background;
        t.status_bar_border = c.statusbar_border;
        t.selection = c.selection;
        t.overlay = c.overlay;
        t.tab_bar = c.tab_background;
        t.tab = c.tab_background;
        t.tab_active = c.tab_active_background;
        t.tab_foreground = c.tab_foreground;
        t.tab_active_foreground = c.tab_active_foreground;
        t.sidebar = c.sidebar;
        t.sidebar_foreground = c.sidebar_foreground;
        t.sidebar_border = c.sidebar_border;
        t.sidebar_accent = c.list_active;
        t.sidebar_accent_foreground = c.list_active_foreground;
        t.scrollbar = Hsla::transparent_black();
        t.scrollbar_thumb = c.scrollbar_thumb;
        t.scrollbar_thumb_hover = c.scrollbar_thumb_hover;
        t.drop_target = c.highlight;
    });
    // gpui-kit has already resolved which one is available on this system.
    let mono = Theme::global(cx).mono_font_family.clone();
    MONO.set(mono);
}
