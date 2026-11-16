// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Desktop app of Vaultty, built with GPUI (gpui-kit). All the logic lives in the domain crates
//! (`vaults`, `sessions`, `sftp`, `terminal`); here there is only UI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod components;
mod features;
mod layout;

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::{AppContext, AssetSource, Result, SharedString};
use sessions::Sessions;
use vaults::Vaults;

use crate::app::state::AppState;

/// How often to check that the vaults are still in their folder.
const CHECK_EVERY: Duration = Duration::from_secs(3);

/// Operating system logos, specific to the app.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "os/*.svg"]
struct OwnAssets;

/// The Lucide icons from gpui-kit plus the app's own logos.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = OwnAssets::get(path) {
            return Ok(Some(file.data));
        }

        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = gpui_kit::assets::AllAssets.list(path)?;
        list.extend(
            OwnAssets::iter()
                .filter(|name| name.starts_with(path))
                .map(|name| SharedString::from(name.to_string())),
        );
        Ok(list)
    }
}

fn main() {
    // SSH connections run on tokio (russh needs it); the UI runs on the GPUI executor.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("could not start the tokio runtime");

    let (events, mut received) = futures::channel::mpsc::unbounded();
    let notify: sessions::Notify = Arc::new(move |event| {
        let _ = events.unbounded_send(event);
    });

    let vaults = Arc::new(Vaults::load());
    let sessions = Sessions::new(vaults.clone(), runtime.handle().clone(), notify);

    let application = gpui_kit::application().with_assets(Assets);

    // macOS: click on the Dock icon with the window closed.
    application.on_reopen(app::shell::show_window);

    application.run(move |cx| {
        gpui_kit::init(cx);
        app::prefs::Prefs::init(cx);

        // Root uses Tab to move focus and, outside macOS, Ctrl+C to copy; in the terminal those
        // keys must reach the server (there, copying is done with Ctrl+Shift+C).
        cx.bind_keys([
            gpui_kit::KeyBinding::new("tab", gpui_kit::NoAction, Some("Terminal")),
            gpui_kit::KeyBinding::new("shift-tab", gpui_kit::NoAction, Some("Terminal")),
            #[cfg(not(target_os = "macos"))]
            gpui_kit::KeyBinding::new("ctrl-c", gpui_kit::NoAction, Some("Terminal")),
        ]);

        let watched = vaults.clone();
        let state = cx.new(|_| AppState::new(vaults, sessions));

        // Session notices arrive from the tokio threads and are applied here, on the UI thread.
        cx.spawn({
            let state = state.clone();
            async move |cx| {
                while let Some(event) = received.next().await {
                    state.update(cx, |s, cx| s.on_event(event, cx));
                }
            }
        })
        .detach();

        // Watches that the vaults are still in their folder. Looking at the disk happens off the UI
        // thread: a network drive that stops answering would freeze the window.
        cx.spawn({
            let state = state.clone();
            async move |cx| {
                loop {
                    let vaults = watched.clone();
                    cx.background_executor()
                        .spawn(async move { vaults.check_missing() })
                        .await;

                    state.update(cx, |s, cx| s.check_vaults(cx));

                    cx.background_executor().timer(CHECK_EVERY).await;
                }
            }
        })
        .detach();

        // The anonymous "this installation is running" ping, now and once a day while open.
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .spawn(async { telemetry::ping() })
                    .await;

                cx.background_executor().timer(telemetry::EVERY).await;
            }
        })
        .detach();

        state.update(cx, |s, cx| {
            s.auto_unlock_all(cx);
            s.check_update(cx);
        });

        app::shell::init(state, cx);
    });

    // The runtime lives until the app is closed.
    drop(runtime);
}
