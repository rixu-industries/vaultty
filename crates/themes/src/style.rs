// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! The colors of a theme and how its JSON keys are read.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Appearance, Color};

/// Where a token comes from when the theme does not provide it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Source {
    /// Base key: if missing, it is inherited from the built-in theme of the same appearance.
    Base,
    /// Another key of the theme itself, with its alpha multiplied by the number (1 = same).
    Key(&'static str, f32),
    /// A fixed color (the black veil behind dialogs).
    Fixed(Color),
}

/// A JSON key: its name, where it comes from if missing, and what shows in the schema.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    pub key: &'static str,
    pub source: Source,
    pub doc: &'static str,
}

const fn from(key: &'static str) -> Source {
    Source::Key(key, 1.)
}

const fn faded(key: &'static str, alpha: f32) -> Source {
    Source::Key(key, alpha)
}

/// A single list for the `struct`, the keys, each one's slot and source: adding a color is
/// one line here (and its entry in the schema, which a test checks).
macro_rules! tokens {
    ($($field:ident $key:literal $source:expr, $doc:literal;)*) => {
        /// Interface colors, in the style of shadcn: one name per place that is painted, even if
        /// many equal another one. Generic so the app can store its own, already converted.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct Ui<C = Color> {
            $(#[doc = $doc] pub $field: C,)*
        }

        impl<C> Ui<C> {
            pub fn map<D>(self, f: impl Fn(C) -> D) -> Ui<D> {
                Ui { $($field: f(self.$field),)* }
            }
        }

        impl Ui {
            fn slot(&mut self, key: &str) -> Option<&mut Color> {
                match key {
                    $($key => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }

        pub(crate) const UI: &[Token] = &[$(Token { key: $key, source: $source, doc: $doc },)*];
    };
}

tokens! {
    // Base: the usual 16.
    background "background" Source::Base, "Main window background.";
    foreground "foreground" Source::Base, "Main text color.";
    card "card" Source::Base, "Cards and raised surfaces. Also the default for the title, tab and status bars.";
    popover "popover" Source::Base, "Menus, dropdowns and dialogs.";
    primary "primary" Source::Base, "Primary buttons, focus outline and the selected item marker.";
    primary_foreground "primary.foreground" Source::Base, "Text on primary buttons.";
    secondary "secondary" Source::Base, "Secondary buttons and muted surfaces.";
    muted_foreground "muted.foreground" Source::Base, "Secondary text, labels and hints.";
    accent "accent" Source::Base, "Hover background.";
    destructive "destructive" Source::Base, "Delete actions and errors.";
    destructive_foreground "destructive.foreground" Source::Base, "Text on destructive buttons.";
    border "border" Source::Base, "Borders and separators.";
    sidebar "sidebar" Source::Base, "Side panel background.";
    sidebar_accent "sidebar.accent" Source::Base, "Selected item in the side panel and lists.";
    success "success" Source::Base, "Connected states and confirmations.";
    warning "warning" Source::Base, "Warnings.";

    // What is missing compared to shadcn.
    card_foreground "card.foreground" from("foreground"), "Text on cards. Defaults to foreground.";
    popover_foreground "popover.foreground" from("foreground"), "Text in menus and dialogs. Defaults to foreground.";
    secondary_foreground "secondary.foreground" from("foreground"), "Text on secondary buttons. Defaults to foreground.";
    muted "muted" from("secondary"), "Muted surfaces such as keyboard hints. Defaults to secondary.";
    accent_foreground "accent.foreground" from("foreground"), "Text on hovered items. Defaults to foreground.";
    input "input" from("border"), "Text field border. Defaults to border.";
    input_background "input.background" from("background"), "Text field background. Defaults to background.";
    ring "ring" from("primary"), "Focus outline of text fields. Defaults to primary.";
    selection "selection" faded("primary", 0.25), "Selected text in fields. Defaults to primary at 25%.";
    overlay "overlay" Source::Fixed(Color { r: 0, g: 0, b: 0, a: 102 }), "Backdrop behind dialogs. Defaults to black at 40%.";
    sidebar_foreground "sidebar.foreground" from("foreground"), "Text in the side panel. Defaults to foreground.";
    sidebar_border "sidebar.border" from("border"), "Side panel border. Defaults to border.";
    sidebar_resize "sidebar.resize" from("ring"), "Side panel edge while hovering to resize it. Defaults to ring.";

    // States.
    primary_hover "primary.hover" faded("primary", 0.88), "Hovered primary button. Defaults to primary at 88%.";
    primary_active "primary.active" faded("primary", 0.8), "Pressed primary button. Defaults to primary at 80%.";
    secondary_hover "secondary.hover" from("accent"), "Hovered secondary, outline and ghost buttons. Defaults to accent.";
    destructive_hover "destructive.hover" faded("destructive", 0.88), "Hovered destructive button. Defaults to destructive at 88%.";
    destructive_active "destructive.active" faded("destructive", 0.8), "Pressed destructive button. Defaults to destructive at 80%.";
    destructive_subtle "destructive.subtle" faded("destructive", 0.12), "Hovered delete action in rows and toolbars. Defaults to destructive at 12%.";
    highlight "highlight" faded("primary", 0.1), "Drop targets and selected files. Defaults to primary at 10%.";
    muted_icon "muted.icon" faded("muted.foreground", 0.6), "Dimmed icons. Defaults to muted.foreground at 60%.";
    disabled_foreground "disabled.foreground" faded("muted.foreground", 0.5), "Disabled items. Defaults to muted.foreground at 50%.";
    border_subtle "border.subtle" faded("border", 0.5), "Soft separators. Defaults to border at 50%.";
    scrollbar_thumb "scrollbar.thumb" faded("muted.foreground", 0.3), "Scrollbar thumb. Defaults to muted.foreground at 30%.";
    scrollbar_thumb_hover "scrollbar.thumb.hover" faded("muted.foreground", 0.5), "Hovered scrollbar thumb. Defaults to muted.foreground at 50%.";

    // Window surfaces.
    titlebar_background "titlebar.background" from("card"), "Title bar. Defaults to card.";
    titlebar_border "titlebar.border" from("border"), "Title bar bottom border. Defaults to border.";
    panel_header_foreground "panel.header.foreground" from("muted.foreground"), "Side panel title. Defaults to muted.foreground.";
    list_foreground "list.foreground" faded("foreground", 0.9), "List and tree rows. Defaults to foreground at 90%.";
    list_hover "list.hover" from("accent"), "Hovered row. Defaults to accent.";
    list_active "list.active" from("sidebar.accent"), "Selected row. Defaults to sidebar.accent.";
    list_active_foreground "list.active.foreground" from("foreground"), "Selected row text. Defaults to foreground.";
    list_active_indicator "list.active.indicator" from("primary"), "Mark on the left of the selected row. Defaults to primary.";
    list_drop "list.drop" from("primary"), "Group outline while dropping a host on it. Defaults to primary.";
    tab_background "tab.background" from("card"), "Tab bar and inactive tabs. Defaults to card.";
    tab_foreground "tab.foreground" from("muted.foreground"), "Inactive tab title. Defaults to muted.foreground.";
    tab_hover_foreground "tab.hover.foreground" from("foreground"), "Hovered inactive tab title. Defaults to foreground.";
    tab_active_background "tab.active.background" from("background"), "Active tab. Defaults to background.";
    tab_active_foreground "tab.active.foreground" from("foreground"), "Active tab title. Defaults to foreground.";
    tab_border "tab.border" from("border"), "Tab separators. Defaults to border.";
    tab_close_hover "tab.close.hover" from("accent"), "Hovered close button of a tab. Defaults to accent.";
    tab_drop "tab.drop" from("primary"), "Insertion mark while dragging a tab. Defaults to primary.";
    statusbar_background "statusbar.background" from("card"), "Status bar. Defaults to card.";
    statusbar_foreground "statusbar.foreground" from("muted.foreground"), "Status bar text and icons. Defaults to muted.foreground.";
    statusbar_border "statusbar.border" from("border"), "Status bar top border. Defaults to border.";
    statusbar_item_active "statusbar.item.active" from("sidebar.accent"), "Status bar button of the open panel. Defaults to sidebar.accent.";
    statusbar_item_active_foreground "statusbar.item.active.foreground" from("foreground"), "Icon of the open panel. Defaults to foreground.";
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terminal {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    pub selection: Color,
    /// The 16 ANSI colors: the 8 normal ones, then their bright versions.
    pub ansi: [Color; 16],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub appearance: Appearance,
    /// `id` of the package it comes from.
    pub package: String,
    pub ui: Ui,
    pub terminal: Terminal,
}

/// A themes file: a family with one or more themes (usually its light and dark versions).
#[derive(Deserialize)]
pub(crate) struct Family {
    pub themes: Vec<Definition>,
}

#[derive(Deserialize)]
pub(crate) struct Definition {
    pub name: String,
    pub appearance: Appearance,
    #[serde(default)]
    pub style: BTreeMap<String, String>,
}

const ANSI: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// The first four come from the theme's own interface, not the built-in one: a blue theme without
/// `terminal.background` wants a blue terminal, not a gray one.
const TERMINAL: [Token; 4] = [
    Token {
        key: "terminal.background",
        source: from("background"),
        doc: "Terminal background. Defaults to the theme's background.",
    },
    Token {
        key: "terminal.foreground",
        source: from("foreground"),
        doc: "Terminal text. Defaults to the theme's foreground.",
    },
    Token {
        key: "terminal.cursor",
        source: from("terminal.foreground"),
        doc: "Terminal cursor. Defaults to terminal.foreground.",
    },
    Token {
        key: "terminal.selection",
        source: from("accent"),
        doc: "Selected text in the terminal. Defaults to the theme's accent.",
    },
];

/// All keys, in schema and resolution order: each one's source comes before it.
pub(crate) fn tokens() -> Vec<(String, Source, String)> {
    let mut all: Vec<(String, Source, String)> = UI
        .iter()
        .chain(&TERMINAL)
        .map(|t| (t.key.to_string(), t.source, t.doc.to_string()))
        .collect();

    for (prefix, label) in [("", ""), ("bright_", "bright ")] {
        all.extend(ANSI.iter().map(|n| {
            (
                format!("terminal.ansi.{prefix}{n}"),
                Source::Base,
                format!("ANSI {label}{n}."),
            )
        }));
    }

    all
}

/// All keys the app understands, in schema order.
pub fn keys() -> Vec<String> {
    tokens().into_iter().map(|(key, _, _)| key).collect()
}

impl Theme {
    fn slot(&mut self, key: &str) -> Option<&mut Color> {
        let t = &mut self.terminal;

        let slot = match key {
            "terminal.background" => &mut t.background,
            "terminal.foreground" => &mut t.foreground,
            "terminal.cursor" => &mut t.cursor,
            "terminal.selection" => &mut t.selection,
            _ => {
                let Some(name) = key.strip_prefix("terminal.ansi.") else {
                    return self.ui.slot(key);
                };
                let (offset, name) = match name.strip_prefix("bright_") {
                    Some(name) => (8, name),
                    None => (0, name),
                };
                let index = ANSI.iter().position(|n| *n == name)?;

                &mut t.ansi[offset + index]
            }
        };

        Some(slot)
    }

    /// Builds a theme on top of `base` (the built-in one of its appearance). Without `base` (Vaultty), all
    /// the base keys must be present. Unknown or misspelled keys do not stop it from loading: they are reported.
    ///
    /// Tokens whose source the theme does not provide are computed from that source **in the theme itself**, never
    /// inherited from the built-in one: a theme that changes `card` wants a matching title bar.
    pub(crate) fn build(
        def: Definition,
        package: &str,
        base: Option<&Theme>,
    ) -> Result<(Theme, Vec<String>), String> {
        let mut theme = match base {
            Some(base) => base.clone(),
            None => Theme {
                name: String::new(),
                appearance: def.appearance,
                package: String::new(),
                ui: Ui::default(),
                terminal: Terminal::default(),
            },
        };

        theme.name = def.name;
        theme.appearance = def.appearance;
        theme.package = package.into();

        let mut warnings = Vec::new();
        let mut set = BTreeSet::new();

        for (key, value) in &def.style {
            let Some(slot) = theme.slot(key) else {
                warnings.push(format!("{}: unknown key \"{key}\"", theme.name));
                continue;
            };

            match Color::parse(value) {
                Some(color) => {
                    *slot = color;
                    set.insert(key.as_str());
                }
                None => warnings.push(format!(
                    "{}: \"{key}\" is not a color (\"{value}\")",
                    theme.name
                )),
            }
        }

        let all = tokens();

        if base.is_none() {
            let missing: Vec<&str> = all
                .iter()
                .filter(|(k, source, _)| {
                    matches!(source, Source::Base) && !set.contains(k.as_str())
                })
                .map(|(k, _, _)| k.as_str())
                .collect();

            if !missing.is_empty() {
                return Err(format!("{}: missing {}", theme.name, missing.join(", ")));
            }
        }

        // In order: each token's source already has its final value by the time it is reached.
        for (key, source, _) in &all {
            if set.contains(key.as_str()) {
                continue;
            }

            let value = match *source {
                Source::Base => continue,
                Source::Fixed(color) => color,
                Source::Key(from, alpha) => {
                    let color = *theme.slot(from).expect("the source is a known key");
                    color.with_alpha(alpha)
                }
            };

            *theme.slot(key).expect("the key is known") = value;
        }

        Ok((theme, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema is what authors see in their editor: it must list the same keys.
    #[test]
    fn schema_has_the_same_keys() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schema/theme-v1.json")).unwrap();
        let style = &schema["$defs"]["theme"]["properties"]["style"]["properties"];
        let in_schema: Vec<&String> = style.as_object().unwrap().keys().collect();
        let mut in_code = keys();
        let mut in_schema: Vec<String> = in_schema.into_iter().cloned().collect();

        in_code.sort();
        in_schema.sort();
        assert_eq!(in_code, in_schema);
    }

    /// A token's source must come before it in the list: that way they resolve in a single pass.
    #[test]
    fn each_source_comes_before_its_token() {
        let all = tokens();

        for (i, (key, source, _)) in all.iter().enumerate() {
            if let Source::Key(from, _) = source {
                let at = all.iter().position(|(k, _, _)| k == from);
                assert!(
                    at.is_some_and(|at| at < i),
                    "{key} comes from {from}, which does not come before it"
                );
            }
        }
    }

    fn build(style: &[(&str, &str)]) -> Theme {
        let library = crate::Library::load_from(None);
        let base = library.find(crate::DEFAULT_DARK).unwrap();
        let def = Definition {
            name: "Test".into(),
            appearance: Appearance::Dark,
            style: style
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };

        Theme::build(def, "test", Some(base)).unwrap().0
    }

    /// Without `titlebar.background`, the bar comes from the theme's own `card`, not Vaultty's.
    #[test]
    fn tokens_come_from_the_theme_itself() {
        let card = Color::parse("#102030").unwrap();
        let theme = build(&[("card", "#102030")]);

        assert_eq!(theme.ui.titlebar_background, card);
        assert_eq!(theme.ui.tab_background, card);
        assert_eq!(theme.ui.statusbar_background, card);
    }

    #[test]
    fn a_written_token_beats_its_source() {
        let red = Color::parse("#ff0000").unwrap();
        let theme = build(&[("tab.active.background", "#ff0000")]);

        assert_eq!(theme.ui.tab_active_background, red);
        assert_ne!(theme.ui.background, red);
    }

    #[test]
    fn tints_lower_the_alpha() {
        let theme = build(&[("primary", "#336699")]);

        assert_eq!(theme.ui.primary_hover, Color::parse("#336699e0").unwrap());
        assert_eq!(theme.ui.highlight, Color::parse("#3366991a").unwrap());
    }

    /// A theme that does not include the tokens looks as it did before they existed: Vaultty does not include them.
    #[test]
    fn without_tokens_it_looks_as_before() {
        let library = crate::Library::load_from(None);
        let all = library
            .of(Appearance::Dark)
            .chain(library.of(Appearance::Light));

        for theme in all.filter(|t| t.package == "vaultty") {
            let u = &theme.ui;

            assert_eq!(u.titlebar_background, u.card, "{}", theme.name);
            assert_eq!(u.tab_active_background, u.background, "{}", theme.name);
            assert_eq!(u.list_active, u.sidebar_accent, "{}", theme.name);
            assert_eq!(u.input, u.border, "{}", theme.name);
            assert_eq!(
                u.muted_icon,
                u.muted_foreground.with_alpha(0.6),
                "{}",
                theme.name
            );
        }
    }

    /// Rewrites the schema keys from those of `tokens!`, descriptions included:
    /// `cargo test -p themes -- --ignored write_schema`.
    #[test]
    #[ignore]
    fn write_schema() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/theme-v1.json");
        let mut schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut style = serde_json::Map::new();

        for (key, _, doc) in tokens() {
            style.insert(
                key,
                serde_json::json!({ "$ref": "#/$defs/color", "description": doc }),
            );
        }

        schema["$defs"]["theme"]["properties"]["style"]["properties"] = style.into();
        let text = serde_json::to_string_pretty(&schema).unwrap();
        std::fs::write(path, text + "\n").unwrap();
    }

    /// The built-in themes ship the colors of their originals (title bar, tabs, status bar): they do not come from
    /// `card` as in Vaultty.
    #[test]
    fn builtin_themes_ship_their_bars() {
        let library = crate::Library::load_from(None);
        let dark_modern = library.find("Dark Modern").unwrap();
        let mocha = library.find("Catppuccin Mocha").unwrap();

        assert_eq!(
            dark_modern.ui.tab_active_foreground,
            Color::parse("#ffffff").unwrap()
        );
        assert_eq!(
            mocha.ui.titlebar_background,
            Color::parse("#11111b").unwrap()
        );
        assert_ne!(mocha.ui.titlebar_background, mocha.ui.card);
    }
}
