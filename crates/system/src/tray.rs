// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Tray icon with "Open" and "Quit". On Windows and macOS it uses `tray-icon`; on Linux, with
//! `ksni` (StatusNotifierItem over D-Bus), which does not need GTK. When the `Tray` is dropped, the icon is removed.

/// What the user requests from the icon. Arrives through the `notify` of `Tray::new`, from any thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Open,
    Quit,
}

type Notify = std::sync::Arc<dyn Fn(TrayEvent) + Send + Sync>;

/// Converts a PNG to 8-bit RGBA.
fn decode(png: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => return Err(format!("Unsupported tray icon format: {other:?}")),
    };

    Ok((rgba, info.width, info.height))
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use super::{Notify, TrayEvent};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct Inner(#[allow(dead_code)] TrayIcon);

    pub fn spawn(rgba: Vec<u8>, w: u32, h: u32, notify: Notify) -> Result<Inner, String> {
        let open = MenuItem::with_id("open", "Open Vaultty", true, None);
        let quit = MenuItem::with_id("quit", "Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;

        let icon = Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string())?;

        // On Windows the left click opens the window and the right one the menu. On macOS, as is
        // customary in the menu bar, any click opens the menu.
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("Vaultty")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()
            .map_err(|e| e.to_string())?;

        let on_menu = notify.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.as_ref() {
            "open" => on_menu(TrayEvent::Open),
            "quit" => on_menu(TrayEvent::Quit),
            _ => {}
        }));

        TrayIconEvent::set_event_handler(Some(move |event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                notify(TrayEvent::Open);
            }
        }));

        Ok(Inner(tray))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{Notify, TrayEvent};
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::StandardItem;
    use ksni::{Icon, MenuItem};

    struct Item {
        icon: Icon,
        notify: Notify,
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "vaultty".into()
        }

        fn title(&self) -> String {
            "Vaultty".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            vec![self.icon.clone()]
        }

        fn activate(&mut self, _: i32, _: i32) {
            (self.notify)(TrayEvent::Open);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Open Vaultty".into(),
                    activate: Box::new(|t: &mut Self| (t.notify)(TrayEvent::Open)),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|t: &mut Self| (t.notify)(TrayEvent::Quit)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub struct Inner(Handle<Item>);

    impl Drop for Inner {
        fn drop(&mut self) {
            // Without waiting: the icon is removed on its own thread.
            let _ = self.0.shutdown();
        }
    }

    pub fn spawn(rgba: Vec<u8>, w: u32, h: u32, notify: Notify) -> Result<Inner, String> {
        // SNI wants ARGB.
        let data = rgba
            .chunks(4)
            .flat_map(|p| [p[3], p[0], p[1], p[2]])
            .collect();
        let icon = Icon {
            width: w as i32,
            height: h as i32,
            data,
        };

        Item { icon, notify }
            .spawn()
            .map(Inner)
            .map_err(|e| format!("No system tray available: {e}"))
    }
}

pub struct Tray(#[allow(dead_code)] imp::Inner);

impl Tray {
    /// Sets the icon. `png` is the icon image (32×32 works well on all systems).
    pub fn new(
        png: &[u8],
        notify: impl Fn(TrayEvent) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let (rgba, w, h) = decode(png)?;
        imp::spawn(rgba, w, h, std::sync::Arc::new(notify)).map(Tray)
    }
}
