// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! What the app does with the system: the main window and the tray icon.
//!
//! Closing the window does not quit the app by itself (`QuitMode::Explicit`): the sessions live in
//! `AppState`, and the window is reopened from the tray (or the Dock) with the tabs as they were.
//! The app menu is in `app::menu`.

use futures::StreamExt;
use gpui_kit::{
    App, AppContext, Bounds, Entity, Global, QuitMode, Window, WindowBounds, WindowOptions, size,
};

use crate::app::menu;
use crate::app::prefs::Prefs;
use crate::app::state::AppState;
use crate::app::workspace::Workspace;

const TRAY_ICON: &[u8] = include_bytes!("../../assets/icons/png/32x32.png");

struct Shell {
    state: Entity<AppState>,
    /// Only while needed: with "Close to tray" or with the window closed.
    tray: Option<system::Tray>,
    tray_events: futures::channel::mpsc::UnboundedSender<system::TrayEvent>,
}

impl Global for Shell {}

pub fn init(state: Entity<AppState>, cx: &mut App) {
    cx.set_quit_mode(QuitMode::Explicit);

    // The icon notifies from its own thread; here it is handled on the UI thread.
    let (tray_events, mut received) = futures::channel::mpsc::unbounded();

    cx.spawn(async move |cx| {
        while let Some(event) = received.next().await {
            cx.update(|cx| match event {
                system::TrayEvent::Open => show_window(cx),
                system::TrayEvent::Quit => cx.quit(),
            });
        }
    })
    .detach();

    cx.set_global(Shell {
        state,
        tray: None,
        tray_events,
    });

    menu::init(cx);

    cx.on_window_closed(|cx, _| {
        if !cx.windows().is_empty() {
            return;
        }

        // On macOS the app stays in the Dock when the window closes, like any other app.
        if Prefs::get(cx).general.close_to_tray || cfg!(target_os = "macos") {
            sync_tray(cx);
        } else {
            cx.quit();
        }
    })
    .detach();

    // Opened by the system at login with "Start in the background": tray only.
    let general = &Prefs::get(cx).general;

    if system::launched_at_login() && general.start_hidden {
        sync_tray(cx);
    } else {
        show_window(cx);
    }
}

/// The app's `AppState`.
pub fn state(cx: &App) -> Entity<AppState> {
    cx.global::<Shell>().state.clone()
}

/// Runs something in the window (opening it if it was closed), such as opening a dialog. It is deferred: the
/// menu actions arrive while GPUI holds the window, and updating it there fails silently.
pub fn with_window(
    cx: &mut App,
    f: impl FnOnce(Entity<AppState>, &mut Window, &mut App) + 'static,
) {
    cx.defer(move |cx| {
        show_window(cx);

        let state = state(cx);

        if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
            let _ = window.update(cx, |_, window, cx| f(state, window, cx));
        }
    });
}

/// Brings the window to the front or reopens it if it was closed.
pub fn show_window(cx: &mut App) {
    if let Some(window) = cx.windows().first() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        cx.activate(true);
        return;
    }

    let state = state(cx);
    let bounds = Bounds::centered(None, size(gpui_kit::px(1200.), gpui_kit::px(760.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(gpui_kit::px(900.), gpui_kit::px(560.))),
        ..gpui_kit::component::TitleBar::window_options()
    };

    gpui_kit::open_window(options, cx, |window, cx| {
        window.set_window_title("Vaultty");
        cx.new(|cx| Workspace::new(state, window, cx))
    })
    .expect("could not open the window");

    cx.activate(true);
    sync_tray(cx);
}

/// Adds or removes the tray icon depending on the settings and on whether the window is open.
pub fn sync_tray(cx: &mut App) {
    let wanted = Prefs::get(cx).general.close_to_tray || cx.windows().is_empty();
    let shell = cx.global_mut::<Shell>();

    match (wanted, shell.tray.is_some()) {
        (true, false) => {
            let events = shell.tray_events.clone();
            let tray = system::Tray::new(TRAY_ICON, move |event| {
                let _ = events.unbounded_send(event);
            });

            // Without a tray (some Linux desktops have none) the app keeps working, just without an icon.
            shell.tray = tray.ok();
        }
        (false, true) => shell.tray = None,
        _ => {}
    }
}
