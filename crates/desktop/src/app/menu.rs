// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The app menu (Vaultty, File, Edit, View, Window, Help). It is defined once with `set_menus`:
//! macOS puts it in its menu bar, and on Windows and Linux the title bar draws it (`layout::title_bar`).
//!
//! Actions are global (`cx.on_action`): this way they work even if focus is on a menu or nothing is
//! focused. Those that open a dialog do so in the active window. The Edit ones have no
//! handler here: the text field or the terminal that has focus handles them.

use gpui_kit::base::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{App, Entity, KeyBinding, Menu, MenuItem, OsAction, Window};
use uuid::Uuid;

use crate::app::prefs::Prefs;
use crate::app::shell;
use crate::app::state::{AppState, Route, Section, pick};
use crate::app::tabs::Tab;
use crate::features::hosts::group_form::open_group_form;
use crate::features::hosts::host_form::open_host_form;
use crate::features::keys::generate::open_generate;
use crate::features::keys::key_form::open_key_form;
use crate::features::settings::appearance::UI_SIZES;
use crate::features::tunnels::form::open_tunnel_form;
use crate::features::vaults::create::open_create;

gpui_kit::actions!(
    vaultty,
    [
        CheckForUpdates,
        OpenSettings,
        OpenVaultSettings,
        OpenKnownHosts,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
        NewVault,
        OpenVaultFolder,
        NewHost,
        NewGroup,
        NewKey,
        GenerateKey,
        NewTunnel,
        ReloadVault,
        LockVault,
        LockAllVaults,
        CloseTab,
        CloseWindow,
        ShowHosts,
        ShowKeychain,
        ShowTunnels,
        ToggleSidebar,
        ShowAllVaults,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        ToggleFullScreen,
        Minimize,
        NextTab,
        PreviousTab,
        ReportBug,
        ReleaseNotes,
        OpenLicenses,
    ]
);

const ISSUES: &str = "https://github.com/rixu-industries/vaultty/issues/new/choose";
const RELEASES: &str = "https://github.com/rixu-industries/vaultty/releases";

pub fn init(cx: &mut App) {
    actions(cx);
    bindings(cx);
    cx.set_menus(menus());
}

fn menus() -> Vec<Menu> {
    let mac = cfg!(target_os = "macos");

    let mut app = vec![
        MenuItem::action("Check for Updates", CheckForUpdates),
        MenuItem::separator(),
        MenuItem::action("Settings…", OpenSettings),
        MenuItem::action("Vault Settings", OpenVaultSettings),
        MenuItem::separator(),
    ];

    if mac {
        app.extend([
            MenuItem::action("Hide Vaultty", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
        ]);
    }

    app.push(MenuItem::action("Quit Vaultty", Quit));

    let mut window = vec![MenuItem::action("Minimize", Minimize)];

    if mac {
        window.push(MenuItem::action("Enter Full Screen", ToggleFullScreen));
    }

    window.extend([
        MenuItem::separator(),
        MenuItem::action("Next Tab", NextTab),
        MenuItem::action("Previous Tab", PreviousTab),
    ]);

    let mut view = vec![
        MenuItem::action("Hosts", ShowHosts),
        MenuItem::action("Keychain", ShowKeychain),
        MenuItem::action("Tunnels", ShowTunnels),
        MenuItem::action("Known Hosts", OpenKnownHosts),
        MenuItem::action("Toggle Side Panel", ToggleSidebar),
        MenuItem::separator(),
        MenuItem::action("All Vaults", ShowAllVaults),
        MenuItem::separator(),
        MenuItem::action("Zoom In", ZoomIn),
        MenuItem::action("Zoom Out", ZoomOut),
        MenuItem::action("Reset Zoom", ResetZoom),
    ];

    // On macOS, full screen goes under Window, as in other apps.
    if !mac {
        view.extend([
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]);
    }

    vec![
        Menu::new("Vaultty").items(app),
        Menu::new("File").items([
            MenuItem::action("New Vault…", NewVault),
            MenuItem::action("Open Vault Folder…", OpenVaultFolder),
            MenuItem::separator(),
            MenuItem::action("New Host…", NewHost),
            MenuItem::action("New Group…", NewGroup),
            MenuItem::action("Import Key…", NewKey),
            MenuItem::action("Generate Key…", GenerateKey),
            MenuItem::action("New Tunnel…", NewTunnel),
            MenuItem::separator(),
            MenuItem::action("Reload Vault", ReloadVault),
            MenuItem::action("Lock Vault", LockVault),
            MenuItem::action("Lock All Vaults", LockAllVaults),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Close Window", CloseWindow),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items(view),
        Menu::new("Window").items(window),
        Menu::new("Help").items([
            MenuItem::action("About Vaultty", OpenLicenses),
            MenuItem::action("Release Notes", ReleaseNotes),
            MenuItem::action("Report a Bug…", ReportBug),
            MenuItem::separator(),
            MenuItem::action("Open Source Licenses", OpenLicenses),
        ]),
    ]
}

/// Shortcuts. On Windows and Linux a bare Ctrl+letter is avoided: it belongs to the terminal (Ctrl+W deletes
/// a word, Ctrl+Q is XON…). The Edit ones are already set by the text field.
fn bindings(cx: &mut App) {
    // A bare Ctrl+B belongs to the terminal (back one character in bash): outside macOS, Shift is added.
    let (settings, zoom_in, zoom_out, reset, sidebar) = if cfg!(target_os = "macos") {
        ("cmd-,", "cmd-=", "cmd--", "cmd-0", "cmd-b")
    } else {
        ("ctrl-,", "ctrl-=", "ctrl--", "ctrl-0", "ctrl-shift-b")
    };

    let mut keys = vec![
        KeyBinding::new(settings, OpenSettings, None),
        KeyBinding::new(sidebar, ToggleSidebar, None),
        KeyBinding::new(zoom_in, ZoomIn, None),
        KeyBinding::new(zoom_out, ZoomOut, None),
        KeyBinding::new(reset, ResetZoom, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
    ];

    if cfg!(target_os = "macos") {
        keys.extend([
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-shift-w", CloseWindow, None),
            KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        ]);
    } else {
        keys.extend([
            KeyBinding::new("ctrl-shift-w", CloseTab, None),
            KeyBinding::new("f11", ToggleFullScreen, None),
        ]);

        // In the terminal, a bare Ctrl+key goes to the server (Ctrl+- is undo in bash).
        keys.extend(
            [settings, zoom_in, zoom_out, reset]
                .map(|k| KeyBinding::new(k, gpui_kit::NoAction, Some("Terminal"))),
        );
    }

    cx.bind_keys(keys);
}

/// The vault open at the current route, if it is unlocked.
fn open_vault(state: &Entity<AppState>, cx: &App) -> Option<Uuid> {
    let s = state.read(cx);
    s.current().filter(|v| s.views.contains_key(v))
}

/// For actions that need an open vault: if there is none, it does nothing.
fn in_vault(cx: &mut App, f: impl FnOnce(Entity<AppState>, Uuid, &mut Window, &mut App) + 'static) {
    shell::with_window(cx, move |state, window, cx| {
        if let Some(vault) = open_vault(&state, cx) {
            f(state, vault, window, cx);
        }
    });
}

/// Changes the side panel list and expands it if it was collapsed.
fn show_section(section: Section, cx: &mut App) {
    shell::show_window(cx);
    shell::state(cx).update(cx, |s, cx| s.show_section(section, false, cx));
}

/// A shortcut written the way each system writes it: "Ctrl+Shift+W" or "⌘W".
pub fn shortcut(binding: Option<KeyBinding>) -> String {
    let Some(binding) = binding else {
        return String::new();
    };

    let mac = cfg!(target_os = "macos");

    let chords = binding.keystrokes().iter().map(|k| {
        let m = k.modifiers();
        let mut parts: Vec<String> = Vec::new();

        let names: [(bool, &str); 4] = if mac {
            [
                (m.control, "⌃"),
                (m.alt, "⌥"),
                (m.shift, "⇧"),
                (m.platform, "⌘"),
            ]
        } else {
            [
                (m.control, "Ctrl"),
                (m.alt, "Alt"),
                (m.shift, "Shift"),
                (m.platform, "Super"),
            ]
        };

        for (on, name) in names {
            if on {
                parts.push(name.into());
            }
        }

        let mut key = k.key().to_string();

        if let Some(first) = key.get_mut(0..1) {
            first.make_ascii_uppercase();
        }

        parts.push(key);
        parts.join(if mac { "" } else { "+" })
    });

    chords.collect::<Vec<_>>().join(" ")
}

fn zoom(step: isize, cx: &mut App) {
    let mut appearance = Prefs::get(cx).appearance.clone();

    let size = if step == 0 {
        themes::AppearanceSettings::default().ui_font_size
    } else {
        // The next size in the Settings list, without going past its ends.
        let at = UI_SIZES
            .iter()
            .position(|s| *s >= appearance.ui_font_size)
            .unwrap_or(UI_SIZES.len() - 1);
        let next = (at as isize + step).clamp(0, UI_SIZES.len() as isize - 1);
        UI_SIZES[next as usize]
    };

    appearance.ui_font_size = size;

    if let Err(e) = Prefs::set_appearance(appearance, cx) {
        let state = shell::state(cx);
        state.update(cx, |s, cx| s.set_error(Some(e), cx));
    }
}

/// Moves to the adjacent tab in the focused pane, wrapping around.
fn cycle_tab(step: isize, cx: &mut App) {
    let state = shell::state(cx);

    state.update(cx, |s, cx| {
        let pane = s.tabs.active.and_then(|id| s.pane_of(id));
        let tabs: Vec<Uuid> = s
            .visible()
            .filter(|t| pane.is_none_or(|p| t.pane == p))
            .map(|t| t.id)
            .collect();
        let n = tabs.len() as isize;

        if n == 0 {
            return;
        }

        let at = s
            .tabs
            .active
            .and_then(|id| tabs.iter().position(|t| *t == id))
            .map_or(if step > 0 { -1 } else { 0 }, |i| i as isize);
        let id = tabs[(at + step).rem_euclid(n) as usize];

        s.show_tab(id, cx);
    });
}

fn actions(cx: &mut App) {
    cx.on_action(|_: &CheckForUpdates, cx| {
        shell::state(cx).update(cx, |s, cx| s.check_update(cx));
    });
    // Inside a vault, settings are one more tab; from the vault list, a full screen.
    cx.on_action(|_: &OpenSettings, cx| {
        shell::show_window(cx);
        shell::state(cx).update(cx, |s, cx| {
            if s.current().is_some_and(|v| s.views.contains_key(&v)) {
                s.open_single(Tab::Settings, cx);
            } else {
                s.go(Route::Settings, cx);
            }
        });
    });
    cx.on_action(|_: &OpenVaultSettings, cx| {
        shell::show_window(cx);
        let state = shell::state(cx);

        if let Some(vault) = open_vault(&state, cx) {
            state.update(cx, |s, cx| s.open_single(Tab::VaultSettings(vault), cx));
        }
    });
    cx.on_action(|_: &OpenKnownHosts, cx| {
        shell::show_window(cx);
        let state = shell::state(cx);

        if let Some(vault) = open_vault(&state, cx) {
            state.update(cx, |s, cx| s.open_single(Tab::KnownHosts(vault), cx));
        }
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());

    cx.on_action(|_: &NewVault, cx| {
        shell::with_window(cx, open_create);
    });
    cx.on_action(|_: &OpenVaultFolder, cx| {
        shell::with_window(cx, |state, window, cx| {
            pick(
                "Choose the vault folder",
                false,
                window,
                cx,
                move |dir, _, cx| {
                    state.update(cx, |s, cx| match s.add_existing(dir, cx) {
                        Ok(vault) => s.go(Route::Vault(vault, Section::Hosts), cx),
                        Err(e) => s.set_error(Some(e), cx),
                    });
                },
            );
        });
    });
    cx.on_action(|_: &NewHost, cx| {
        in_vault(cx, |state, vault, window, cx| {
            open_host_form(state, vault, None, None, window, cx)
        });
    });
    cx.on_action(|_: &NewGroup, cx| {
        in_vault(cx, |state, vault, window, cx| {
            open_group_form(state, vault, None, window, cx)
        });
    });
    cx.on_action(|_: &NewKey, cx| {
        in_vault(cx, |state, vault, window, cx| {
            open_key_form(state, vault, None, window, cx)
        });
    });
    cx.on_action(|_: &GenerateKey, cx| {
        in_vault(cx, |state, vault, window, cx| {
            open_generate(state, vault, window, cx)
        });
    });
    cx.on_action(|_: &NewTunnel, cx| {
        in_vault(cx, |state, vault, window, cx| {
            open_tunnel_form(state, vault, None, window, cx)
        });
    });
    cx.on_action(|_: &ReloadVault, cx| {
        let state = shell::state(cx);

        if let Some(vault) = open_vault(&state, cx) {
            state.update(cx, |s, cx| s.reload(vault, cx));
        }
    });
    cx.on_action(|_: &LockVault, cx| {
        let state = shell::state(cx);

        if let Some(vault) = open_vault(&state, cx) {
            state.update(cx, |s, cx| s.lock(vault, cx));
        }
    });
    cx.on_action(|_: &LockAllVaults, cx| {
        shell::state(cx).update(cx, |s, cx| {
            let open: Vec<_> = s.views.keys().copied().collect();

            for vault in open {
                s.lock(vault, cx);
            }
        });
    });
    cx.on_action(|_: &CloseTab, cx| {
        shell::state(cx).update(cx, |s, cx| {
            if let Some(id) = s.tabs.active {
                s.close_tab(id, cx);
            }
        });
    });
    cx.on_action(|_: &CloseWindow, cx| {
        shell::with_window(cx, |_, window, _| window.remove_window());
    });

    cx.on_action(|_: &ShowHosts, cx| show_section(Section::Hosts, cx));
    cx.on_action(|_: &ShowKeychain, cx| show_section(Section::Keys, cx));
    cx.on_action(|_: &ShowTunnels, cx| show_section(Section::Tunnels, cx));
    cx.on_action(|_: &ToggleSidebar, cx| {
        let state = shell::state(cx);

        if open_vault(&state, cx).is_some() {
            state.update(cx, |s, cx| s.toggle_sidebar(cx));
        }
    });
    cx.on_action(|_: &ShowAllVaults, cx| {
        shell::show_window(cx);
        shell::state(cx).update(cx, |s, cx| s.go(Route::Picker, cx));
    });
    cx.on_action(|_: &ZoomIn, cx| zoom(1, cx));
    cx.on_action(|_: &ZoomOut, cx| zoom(-1, cx));
    cx.on_action(|_: &ResetZoom, cx| zoom(0, cx));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        shell::with_window(cx, |_, window, _| window.toggle_fullscreen());
    });

    cx.on_action(|_: &Minimize, cx| {
        shell::with_window(cx, |_, window, _| window.minimize_window());
    });
    cx.on_action(|_: &NextTab, cx| cycle_tab(1, cx));
    cx.on_action(|_: &PreviousTab, cx| cycle_tab(-1, cx));

    cx.on_action(|_: &ReleaseNotes, cx| cx.open_url(RELEASES));
    cx.on_action(|_: &ReportBug, cx| cx.open_url(ISSUES));
    // Like settings: a tab inside a vault, a full screen outside.
    cx.on_action(|_: &OpenLicenses, cx| {
        shell::show_window(cx);
        shell::state(cx).update(cx, |s, cx| {
            if s.current().is_some_and(|v| s.views.contains_key(&v)) {
                s.open_single(Tab::Licenses, cx);
            } else {
                s.go(Route::Licenses, cx);
            }
        });
    });
}
