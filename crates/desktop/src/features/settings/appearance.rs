// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! "Appearance" category: light, dark or system mode, the theme for each appearance, the text size
//! and the installed theme packages. Themes and their installation live in the `themes` crate.

use crate::app::prefs::Prefs;
use crate::app::state::pick;
use crate::app::theme::px;
use crate::app::theme::{colors, hsla};
use crate::components::{
    Button, ButtonSize, ButtonVariant, choice, confirm, glyph, icon, note, section_title,
};
use crate::features::settings::{header, setting};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div,
};
use themes::{Appearance, Mode, Theme};

const MODES: [(Mode, &str); 3] = [
    (Mode::System, "Match system"),
    (Mode::Light, "Light"),
    (Mode::Dark, "Dark"),
];

pub const UI_SIZES: [f32; 6] = [12., 13., 14., 15., 16., 18.];

#[derive(Default)]
pub struct AppearanceSettings {
    /// Error when saving, installing or uninstalling.
    error: Option<String>,
    /// What was imported from VS Code (and what was skipped), until the next action.
    imported: Option<String>,
}

impl AppearanceSettings {
    fn update(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut themes::AppearanceSettings),
    ) {
        let mut settings = Prefs::get(cx).appearance.clone();
        edit(&mut settings);

        self.error = Prefs::set_appearance(settings, cx).err();
        cx.notify();
    }

    /// Choosing a theme sets it for its appearance. If the mode is fixed to the other one, it is changed:
    /// whoever picks a light theme wants to see it right away.
    fn select(&mut self, theme: &Theme, cx: &mut Context<Self>) {
        let (name, appearance) = (theme.name.clone(), theme.appearance);

        self.update(cx, |s| {
            match appearance {
                Appearance::Light => s.light = name,
                Appearance::Dark => s.dark = name,
            }

            if s.mode != Mode::System {
                s.mode = match appearance {
                    Appearance::Light => Mode::Light,
                    Appearance::Dark => Mode::Dark,
                };
            }
        });
    }

    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();

        pick("Install theme", false, window, cx, move |dir, _, cx| {
            let result = themes::install(&dir);

            if result.is_ok() {
                Prefs::reload_themes(cx);
            }

            this.update(cx, |t, cx| {
                t.error = result.err();
                cx.notify();
            });
        });
    }

    /// Imports a VS Code theme (`.vsix`, `package.json` or a standalone theme file) and applies it.
    fn import_vscode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();

        pick(
            "Import VS Code theme",
            true,
            window,
            cx,
            move |file, _, cx| {
                let result = themes::import_vscode(&file);

                if result.is_ok() {
                    Prefs::reload_themes(cx);
                }

                this.update(cx, |t, cx| {
                    t.imported = None;

                    match result {
                        Ok(imported) => {
                            let id = imported.manifest.id;
                            let names: Vec<Theme> = Prefs::get(cx)
                                .library
                                .themes
                                .iter()
                                .filter(|th| th.package == id)
                                .cloned()
                                .collect();
                            let list: Vec<&str> = names.iter().map(|th| th.name.as_str()).collect();
                            let mut text = format!("Imported {}.", list.join(", "));

                            if !imported.skipped.is_empty() {
                                text.push_str(&format!(
                                    " Already installed, so skipped: {}.",
                                    imported.skipped.join(", ")
                                ));
                            }

                            t.imported = Some(text);

                            // Whoever imports a theme wants to see it right away.
                            if let Some(first) = names.first() {
                                t.select(first, cx);
                            }
                        }
                        Err(e) => t.error = Some(e),
                    }

                    cx.notify();
                });
            },
        );
    }

    fn uninstall(&mut self, id: &str, cx: &mut Context<Self>) {
        self.error = themes::uninstall(id).err();
        Prefs::reload_themes(cx);
        cx.notify();
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = themes::themes_dir() else {
            return;
        };

        // It is created so the system has something to open the first time.
        match std::fs::create_dir_all(&dir) {
            Ok(()) => cx.open_with_system(&dir),
            Err(e) => self.error = Some(format!("Could not open {}: {e}", dir.display())),
        }

        cx.notify();
    }

    fn themes(&self, appearance: Appearance, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs = Prefs::get(cx);
        let selected = prefs.appearance.theme_for(appearance).to_string();
        let active = prefs
            .library
            .pick(&prefs.appearance, prefs.system)
            .name
            .clone();
        let themes: Vec<Theme> = prefs.library.of(appearance).cloned().collect();

        div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .children(themes.into_iter().map(|theme| {
                let this = cx.entity();
                let chosen = theme.name == selected;
                let on = theme.name == active;

                theme_card(&theme, chosen, on).on_click(move |_, _, cx| {
                    this.update(cx, |t, cx| t.select(&theme, cx));
                })
            }))
    }

    fn packages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let prefs = Prefs::get(cx);
        let packages = prefs.library.packages.clone();
        let count = |id: &str| {
            prefs
                .library
                .themes
                .iter()
                .filter(|t| t.package == id)
                .count()
        };
        // Built-in themes are not listed: they already show up as cards and cannot be uninstalled.
        let rows: Vec<_> = packages
            .into_iter()
            .filter(|p| p.dir.is_some())
            .map(|p| (count(&p.manifest.id), p))
            .collect();

        if rows.is_empty() {
            return div()
                .px(px(12.))
                .py(px(10.))
                .border_1()
                .border_dashed()
                .border_color(c.border)
                .rounded(px(6.))
                .child(note(
                    "No themes installed yet. The ones above come with the app.",
                    c.muted_foreground,
                ));
        }

        div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(c.border)
            .rounded(px(6.))
            .children(rows.into_iter().enumerate().map(|(i, (count, package))| {
                let m = package.manifest;
                let this = cx.entity();
                let id = m.id.clone();
                let mut details = vec![format!("v{}", m.version)];

                if !m.authors.is_empty() {
                    details.push(m.authors.join(", "));
                }

                details.push(match count {
                    1 => "1 theme".into(),
                    n => format!("{n} themes"),
                });

                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(8.))
                    .when(i > 0, |d| d.border_t_1().border_color(c.border))
                    .child(icon(IconName::Palette, 14., c.muted_foreground))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(m.name.clone())
                            .child(note(details.join(" · "), c.muted_foreground)),
                    )
                    .child(
                        Button::new(gpui_kit::ElementId::Name(
                            format!("theme-uninstall-{id}").into(),
                        ))
                        .variant(ButtonVariant::Outline)
                        .size(ButtonSize::Small)
                        .label("Uninstall")
                        .on_click(move |_, window, cx| {
                            let (this, id) = (this.clone(), id.clone());

                            confirm(
                                format!("Uninstall {}", m.name),
                                "Its themes will no longer be available. If one of them is in use, the app goes back to the built-in theme.",
                                "Uninstall",
                                window,
                                cx,
                                move |_, cx| this.update(cx, |t, cx| t.uninstall(&id, cx)),
                            );
                        }),
                    )
            }))
    }
}

/// Card for a theme, with a thumbnail of the window painted in its colors.
fn theme_card(theme: &Theme, chosen: bool, active: bool) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors();
    let (u, t) = (&theme.ui, &theme.terminal);

    // Red, green, yellow, blue, magenta and cyan: the colors that show most in a terminal.
    let swatches = (1..7).map(|i| div().size(px(8.)).rounded_full().bg(hsla(t.ansi[i])));

    // The window in miniature, with the tokens for each area: title, panel, tabs, terminal and status bar.
    let body = div()
        .flex()
        .flex_1()
        .min_h_0()
        .child(
            div()
                .w(px(40.))
                .h_full()
                .bg(hsla(u.sidebar))
                .border_r_1()
                .border_color(hsla(u.sidebar_border))
                .p(px(6.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(div().h(px(6.)).rounded(px(2.)).bg(hsla(u.list_active)))
                .child(
                    div()
                        .h(px(6.))
                        .w(px(20.))
                        .rounded(px(2.))
                        .bg(hsla(u.muted_foreground)),
                ),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .h(px(12.))
                        .bg(hsla(u.tab_background))
                        .child(
                            div()
                                .w(px(28.))
                                .h_full()
                                .bg(hsla(u.tab_active_background))
                                .border_r_1()
                                .border_color(hsla(u.tab_border)),
                        )
                        .child(div().flex_1().border_b_1().border_color(hsla(u.tab_border))),
                )
                .child(
                    div()
                        .flex_1()
                        .m(px(6.))
                        .p(px(6.))
                        .rounded(px(3.))
                        .bg(hsla(t.background))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .h(px(4.))
                                .w(px(48.))
                                .rounded(px(2.))
                                .bg(hsla(t.foreground)),
                        )
                        .child(div().flex().gap(px(4.)).children(swatches)),
                ),
        );

    let preview = div()
        .flex()
        .flex_col()
        .h(px(92.))
        .bg(hsla(u.background))
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .px(px(6.))
                .h(px(12.))
                .bg(hsla(u.titlebar_background))
                .border_b_1()
                .border_color(hsla(u.titlebar_border))
                .child(
                    div()
                        .h(px(6.))
                        .w(px(18.))
                        .rounded(px(2.))
                        .bg(hsla(u.primary)),
                ),
        )
        .child(body)
        .child(
            div()
                .h(px(8.))
                .bg(hsla(u.statusbar_background))
                .border_t_1()
                .border_color(hsla(u.statusbar_border)),
        );

    div()
        .id(gpui_kit::ElementId::Name(
            format!("theme-{}", theme.name).into(),
        ))
        // Three per row in the width of the settings content (616).
        .w(px(192.))
        .flex()
        .flex_col()
        .rounded(px(8.))
        .overflow_hidden()
        .border_1()
        .border_color(if chosen { c.primary } else { c.border })
        .cursor_pointer()
        .hover(|s| s.border_color(c.muted_foreground))
        .child(preview)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(6.))
                .bg(c.card)
                .border_t_1()
                .border_color(c.border)
                .child(div().flex_1().truncate().child(theme.name.clone()))
                .when(active, |d| {
                    d.child(icon(IconName::Check, 13., c.foreground))
                }),
        )
}

impl Render for AppearanceSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let prefs = Prefs::get(cx);
        let (mode, size) = (prefs.appearance.mode, prefs.appearance.ui_font_size);
        let problems = prefs.library.problems.clone();
        let (on_mode, on_size, install, open) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let import = cx.entity();

        let modes = MODES.iter().map(|(m, l)| (*m, l.to_string())).collect();
        let sizes = UI_SIZES.iter().map(|s| (*s, format!("{s} px"))).collect();

        div()
            .flex()
            .flex_col()
            .child(header(
                "Appearance",
                "Choose how Vaultty looks. Each theme colors both the app and the terminal.",
            ))
            .child(setting(
                "Mode",
                "With \"Match system\", the app switches between your light and dark theme when the system does.",
                div().w(px(180.)).child(choice("appearance-mode", modes, Some(mode), move |m, _, cx| {
                    on_mode.update(cx, |t, cx| t.update(cx, |s| s.mode = m));
                })),
            ))
            .when_some(self.error.clone(), |this, e| {
                this.child(div().mt(px(8.)).child(note(e, c.destructive)))
            })
            .child(section_title("Dark theme"))
            .child(self.themes(Appearance::Dark, cx))
            .child(section_title("Light theme"))
            .child(self.themes(Appearance::Light, cx))
            .child(section_title("Text"))
            .child(setting(
                "Interface size",
                "Scales the whole interface: text, lists and controls. The terminal has its own size.",
                div().w(px(120.)).child(choice("ui-font-size", sizes, Some(size), move |s, _, cx| {
                    on_size.update(cx, |t, cx| t.update(cx, |a| a.ui_font_size = s));
                })),
            ))
            .child(section_title("Installed themes"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mb(px(12.))
                    .child(
                        Button::new("theme-install")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Small)
                            .icon(IconName::Folder)
                            .label("Install from folder…")
                            .on_click(move |_, window, cx| {
                                install.update(cx, |t, cx| t.install(window, cx))
                            }),
                    )
                    .child(
                        Button::new("theme-import-vscode")
                            .variant(ButtonVariant::Outline)
                            .size(ButtonSize::Small)
                            .icon(IconName::Download)
                            .label("Import VS Code theme…")
                            .tooltip("A .vsix extension, its package.json or a theme .json file")
                            .on_click(move |_, window, cx| {
                                import.update(cx, |t, cx| t.import_vscode(window, cx))
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("theme-folder")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSmall)
                            .tooltip("Open themes folder")
                            .child(glyph(IconName::FolderOpen, 13.))
                            .on_click(move |_, _, cx| open.update(cx, |t, cx| t.open_folder(cx))),
                    )
                    .child(
                        Button::new("theme-reload")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSmall)
                            .tooltip("Reload themes")
                            .child(glyph(IconName::RefreshCw, 13.))
                            .on_click(|_, _, cx: &mut App| Prefs::reload_themes(cx)),
                    ),
            )
            .when_some(self.imported.clone(), |this, text| {
                this.child(div().mb(px(8.)).child(note(text, c.success)))
            })
            .child(self.packages(cx))
            .child(div().mt(px(8.)).child(note(
                "A theme is a folder with a manifest.json and its theme files. Colors a theme leaves out come from the built-in theme of the same appearance. VS Code themes are converted on import: their window colors carry over, syntax colors do not.",
                c.muted_foreground,
            )))
            .children(problems.into_iter().map(move |p| div().mt(px(4.)).child(note(p, c.warning))))
    }
}
