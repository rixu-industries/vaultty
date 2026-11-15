// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Import VS Code themes: an extension (`.vsix`, as downloaded from the Marketplace or Open
//! VSX), its `package.json` already extracted, or a loose theme file (`.json`). Each theme is converted
//! to the app's format and installed as one more package, so it can be uninstalled and imported again
//! (same `id`: it replaces it).
//!
//! Only the window colors (`colors`) are taken from VS Code; the syntax ones (`tokenColors`)
//! have no place to go. Anything the theme does not define is computed from its background and text,
//! so Vaultty's grays don't show up in the middle of a blue theme.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Map, Value, json};

use crate::package::{Manifest, install_into, themes_dir};
use crate::{Appearance, Color, Library, SCHEMA_VERSION};

type Reader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

/// Files of the extension, with the path relative to its root (`themes/x.json`).
struct Files {
    read: Reader,
}

/// A theme declared by the extension.
struct Entry {
    label: String,
    appearance: Option<Appearance>,
    path: String,
}

/// What is known about the extension, for the `manifest.json`.
#[derive(Default)]
struct About {
    id: String,
    name: String,
    version: String,
    description: String,
    author: Option<String>,
    repository: Option<String>,
    license: Option<String>,
}

/// What was imported.
#[derive(Debug)]
pub struct Imported {
    pub manifest: Manifest,
    /// Themes not imported because one with the same name already exists (for example, a built-in one).
    pub skipped: Vec<String>,
}

pub fn import_vscode(from: &Path) -> Result<Imported, String> {
    let dir = themes_dir().ok_or("System configuration folder not found")?;
    import_vscode_into(&dir, from)
}

pub fn import_vscode_into(dir: &Path, from: &Path) -> Result<Imported, String> {
    let (mut about, entries, files) = open(from)?;

    if entries.is_empty() {
        return Err("This extension has no color themes".into());
    }

    // Themes already present (built in or from another package) are skipped: this way the rest of the
    // extension is imported, like Frappé and Macchiato from Catppuccin, which ships Mocha built in.
    let library = Library::load_from(Some(dir));

    // A built-in `id` cannot be used (a loose theme named "Dracula" would give `dracula`).
    if library
        .packages
        .iter()
        .any(|p| p.dir.is_none() && p.manifest.id == about.id)
    {
        about.id = format!("vscode-{}", about.id);
    }
    let taken = |name: &str| {
        library
            .themes
            .iter()
            .any(|t| t.name == name && t.package != about.id)
    };
    let (entries, skipped): (Vec<Entry>, Vec<Entry>) =
        entries.into_iter().partition(|e| !taken(&e.label));
    let skipped: Vec<String> = skipped.into_iter().map(|e| e.label).collect();

    if entries.is_empty() {
        return Err(format!("Already installed: {}", skipped.join(", ")));
    }

    let mut themes = Vec::new();

    for entry in &entries {
        let colors = colors(&files, &entry.path, 0)?;
        let appearance = entry.appearance.unwrap_or_else(|| guess(&colors));

        themes.push(json!({
            "name": entry.label,
            "appearance": appearance,
            "style": style(&colors, appearance),
        }));
    }

    let file = format!("themes/{}.json", about.id);
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        id: about.id.clone(),
        name: about.name.clone(),
        version: about.version.clone(),
        description: if about.description.is_empty() {
            "Imported from VS Code.".into()
        } else {
            about.description.clone()
        },
        authors: about.author.clone().into_iter().collect(),
        repository: about.repository.clone(),
        license: about.license.clone(),
        themes: vec![file.clone()],
    };
    let family = json!({
        "$schema": "https://raw.githubusercontent.com/rixu-industries/vaultty/main/crates/themes/schema/theme-v1.json",
        "name": about.name,
        "author": about.author.unwrap_or_default(),
        "themes": themes,
    });

    // The package is assembled outside and installed with the same checks as a hand-made one.
    // A unique name per import: two at once do not clash.
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let stage = std::env::temp_dir().join(format!(
        "vaultty-import-{}-{n}-{}",
        std::process::id(),
        about.id
    ));
    let error = |e: std::io::Error| format!("Could not import the theme: {e}");
    let _ = fs::remove_dir_all(&stage);
    // The first time, the themes folder does not exist yet.
    fs::create_dir_all(dir).map_err(error)?;
    fs::create_dir_all(stage.join("themes")).map_err(error)?;

    let manifest_text = serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n";
    let family_text = serde_json::to_string_pretty(&family).unwrap_or_default() + "\n";
    fs::write(stage.join("manifest.json"), manifest_text).map_err(error)?;
    fs::write(stage.join(&file), family_text).map_err(error)?;

    let result = install_into(dir, &stage);
    let _ = fs::remove_dir_all(&stage);

    Ok(Imported {
        manifest: result?,
        skipped,
    })
}

/// Opens the chosen file: a `.vsix`, an extension's `package.json` or a loose theme.
fn open(from: &Path) -> Result<(About, Vec<Entry>, Files), String> {
    let file = from
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let name = file.to_lowercase();

    if name.ends_with(".vsix") {
        let files = vsix(from)?;
        let (about, entries) = extension(&files)?;
        return Ok((about, entries, files));
    }

    let folder = from.parent().unwrap_or(Path::new(".")).to_path_buf();
    let files = Files {
        read: Box::new(move |path| fs::read(folder.join(path)).ok()),
    };

    if name == "package.json" {
        let (about, entries) = extension(&files)?;
        return Ok((about, entries, files));
    }

    // A loose theme: its name comes from the JSON itself or, if it has none, from the file name.
    let bytes = fs::read(from).map_err(|e| format!("Could not read {}: {e}", from.display()))?;
    let theme = jsonc(&bytes).map_err(|e| format!("{name} is not a VS Code theme: {e}"))?;
    let stem = from
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("theme")
        .trim_end_matches("-color-theme")
        .to_string();
    let label = theme["name"].as_str().map(String::from).unwrap_or(stem);
    let appearance = match theme["type"].as_str() {
        Some("light" | "hcLight") => Some(Appearance::Light),
        Some("dark" | "hc" | "hcDark") => Some(Appearance::Dark),
        _ => None,
    };
    let about = About {
        id: slug(&label),
        name: label.clone(),
        version: "1.0.0".into(),
        ..About::default()
    };

    let entries = vec![Entry {
        label,
        appearance,
        path: file,
    }];

    Ok((about, entries, files))
}

/// Cap on each JSON file read from a `.vsix`: theme files are far smaller.
const MAX_JSON: u64 = 4 * 1024 * 1024;

/// Reads the JSON files of a `.vsix` (a zip with the extension inside `extension/`). There are few, and they are small.
fn vsix(path: &Path) -> Result<Files, String> {
    let bad = |e: &dyn std::fmt::Display| format!("{} is not a valid .vsix: {e}", path.display());
    let file = fs::File::open(path).map_err(|e| bad(&e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(&e))?;
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| bad(&e))?;
        let full = entry.name().map_err(|e| bad(&e))?.to_string();
        let Some(name) = full.strip_prefix("extension/").map(String::from) else {
            continue;
        };

        if !name.ends_with(".json") || entry.size() > MAX_JSON {
            continue;
        }

        // The declared size is not trusted: a crafted zip can expand to far more.
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_JSON + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| bad(&e))?;

        if bytes.len() as u64 > MAX_JSON {
            continue;
        }

        files.insert(name, bytes);
    }

    Ok(Files {
        read: Box::new(move |path| files.get(path).cloned()),
    })
}

/// The data and themes of an extension, from its `package.json`.
fn extension(files: &Files) -> Result<(About, Vec<Entry>), String> {
    let bytes = (files.read)("package.json").ok_or("The extension has no package.json")?;
    let package = jsonc(&bytes).map_err(|e| format!("package.json is not valid: {e}"))?;
    // Texts like `%key%` are translated in `package.nls.json`.
    let nls = (files.read)("package.nls.json")
        .and_then(|b| jsonc(&b).ok())
        .unwrap_or(Value::Null);
    let text = |v: &Value| -> Option<String> {
        let s = v.as_str()?;

        match s.strip_prefix('%').and_then(|k| k.strip_suffix('%')) {
            Some(key) => nls[key].as_str().map(String::from),
            None => Some(s.to_string()),
        }
    };

    let name = text(&package["name"]).unwrap_or_else(|| "vscode-theme".into());
    let display = text(&package["displayName"]).unwrap_or_else(|| name.clone());
    let publisher = text(&package["publisher"]);
    let id = match &publisher {
        Some(p) => slug(&format!("{p}-{name}")),
        None => slug(&name),
    };
    let repository = match &package["repository"] {
        Value::String(url) => Some(url.clone()),
        other => other["url"].as_str().map(String::from),
    };

    let about = About {
        id,
        name: display,
        version: text(&package["version"]).unwrap_or_else(|| "1.0.0".into()),
        description: text(&package["description"]).unwrap_or_default(),
        author: publisher,
        repository,
        license: text(&package["license"]),
    };

    let list = package["contributes"]["themes"].as_array().cloned();
    let entries = list
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            let path = normalize("", t["path"].as_str()?)?;
            let label = text(&t["label"])
                .or_else(|| text(&t["id"]))
                .unwrap_or_else(|| path.clone());
            let appearance = match t["uiTheme"].as_str() {
                Some("vs" | "hc-light") => Some(Appearance::Light),
                Some("vs-dark" | "hc-black") => Some(Appearance::Dark),
                _ => None,
            };

            Some(Entry {
                label,
                appearance,
                path,
            })
        })
        .collect();

    Ok((about, entries))
}

/// The window colors of a theme, with those of its `include` underneath (a theme can inherit from
/// another, such as Dark Modern from Dark+).
fn colors(files: &Files, path: &str, depth: usize) -> Result<BTreeMap<String, Color>, String> {
    if depth > 8 {
        return Err(format!("{path}: too many nested includes"));
    }

    let bytes = (files.read)(path).ok_or(format!("The extension has no {path}"))?;
    let theme = jsonc(&bytes).map_err(|e| format!("{path} is not valid: {e}"))?;

    let mut colors = match theme["include"].as_str() {
        Some(include) => {
            let base = Path::new(path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            let inner = normalize(base, include).ok_or(format!("{path}: bad include"))?;
            colors(files, &inner, depth + 1)?
        }
        None => BTreeMap::new(),
    };

    if let Some(own) = theme["colors"].as_object() {
        for (key, value) in own {
            if let Some(color) = value.as_str().and_then(parse) {
                colors.insert(key.clone(), color);
            }
        }
    }

    Ok(colors)
}

/// A VS Code color: besides ours, it accepts `#rgba`.
fn parse(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;

    if hex.len() == 4 && hex.is_ascii() {
        let long: String = hex.chars().flat_map(|c| [c, c]).collect();
        return Color::parse(&format!("#{long}"));
    }

    Color::parse(text)
}

/// Light or dark based on the background, for themes that don't say.
fn guess(colors: &BTreeMap<String, Color>) -> Appearance {
    match colors.get("editor.background") {
        Some(c) if luma(*c) > 0.5 => Appearance::Light,
        _ => Appearance::Dark,
    }
}

fn luma(c: Color) -> f32 {
    (0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32) / 255.
}

/// `a` moved toward `b` by the ratio `t` (0 = `a`, 1 = `b`), opaque.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;

    Color {
        r: ch(a.r, b.r),
        g: ch(a.g, b.g),
        b: ch(a.b, b.b),
        a: 255,
    }
}

/// Our keys ← VS Code's, in order of preference. Those with nothing are computed
/// afterwards (the base ones) or come from their source (the tokens).
const MAP: &[(&str, &[&str])] = &[
    ("background", &["editor.background"]),
    ("foreground", &["editor.foreground", "foreground"]),
    (
        "card",
        &["sideBar.background", "editorGroupHeader.tabsBackground"],
    ),
    (
        "popover",
        &[
            "menu.background",
            "editorWidget.background",
            "dropdown.background",
        ],
    ),
    ("primary", &["button.background", "focusBorder"]),
    ("primary.foreground", &["button.foreground"]),
    (
        "secondary",
        &["button.secondaryBackground", "input.background"],
    ),
    (
        "muted.foreground",
        &["descriptionForeground", "tab.inactiveForeground"],
    ),
    ("accent", &["list.hoverBackground"]),
    (
        "destructive",
        &["errorForeground", "editorError.foreground"],
    ),
    (
        "border",
        &[
            "sideBar.border",
            "panel.border",
            "editorGroup.border",
            "contrastBorder",
        ],
    ),
    ("sidebar", &["sideBar.background"]),
    (
        "sidebar.accent",
        &[
            "list.inactiveSelectionBackground",
            "list.activeSelectionBackground",
        ],
    ),
    (
        "success",
        &[
            "gitDecoration.addedResourceForeground",
            "terminal.ansiGreen",
        ],
    ),
    (
        "warning",
        &["editorWarning.foreground", "list.warningForeground"],
    ),
    ("titlebar.background", &["titleBar.activeBackground"]),
    ("titlebar.border", &["titleBar.border"]),
    ("sidebar.foreground", &["sideBar.foreground"]),
    ("sidebar.border", &["sideBar.border"]),
    ("list.hover", &["list.hoverBackground"]),
    ("tab.background", &["editorGroupHeader.tabsBackground"]),
    ("tab.foreground", &["tab.inactiveForeground"]),
    ("tab.hover.foreground", &["tab.hoverForeground"]),
    ("tab.active.background", &["tab.activeBackground"]),
    ("tab.active.foreground", &["tab.activeForeground"]),
    ("tab.border", &["tab.border"]),
    ("statusbar.background", &["statusBar.background"]),
    ("statusbar.foreground", &["statusBar.foreground"]),
    ("statusbar.border", &["statusBar.border"]),
    ("input.background", &["input.background"]),
    ("input", &["input.border"]),
    ("selection", &["selection.background"]),
    ("scrollbar.thumb", &["scrollbarSlider.background"]),
    (
        "scrollbar.thumb.hover",
        &["scrollbarSlider.hoverBackground"],
    ),
    ("terminal.background", &["terminal.background"]),
    ("terminal.foreground", &["terminal.foreground"]),
    ("terminal.cursor", &["terminalCursor.foreground"]),
    ("terminal.selection", &["terminal.selectionBackground"]),
];

/// The base keys: they cannot be transparent.
const BASE: [&str; 15] = [
    "background",
    "foreground",
    "card",
    "popover",
    "primary",
    "primary.foreground",
    "secondary",
    "muted.foreground",
    "accent",
    "destructive",
    "border",
    "sidebar",
    "sidebar.accent",
    "success",
    "warning",
];

const ANSI: [&str; 8] = [
    "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
];

/// Our theme's `style`, built from the VS Code colors.
fn style(colors: &BTreeMap<String, Color>, appearance: Appearance) -> Map<String, Value> {
    let mut out: BTreeMap<&str, Color> = BTreeMap::new();

    for (ours, theirs) in MAP {
        // A transparent border or base background (Catppuccin paints no borders) would leave buttons and
        // cards without an outline: the next value is looked up or computed. For tokens it is fine.
        let base = BASE.contains(ours);
        let found = theirs
            .iter()
            .filter_map(|k| colors.get(*k))
            .find(|c| !(base && c.a == 0));

        if let Some(c) = found {
            out.insert(ours, *c);
        }
    }

    // What does not fit this design: a button's hover
    // that is not our color, or a row's that equals the panel.
    let same = |a: Option<&Color>, b: Option<&Color>| a.is_some() && a == b;

    if let Some(hover) = colors.get("button.hoverBackground")
        && same(colors.get("button.background"), out.get("primary"))
    {
        out.insert("primary.hover", *hover);
    }

    if let Some(hover) = colors.get("button.secondaryHoverBackground")
        && same(
            colors.get("button.secondaryBackground"),
            out.get("secondary"),
        )
    {
        out.insert("secondary.hover", *hover);
    }

    if out.get("list.hover").map(|c| Color { a: 255, ..*c }) == out.get("sidebar").copied() {
        out.remove("list.hover");
    }

    // Missing base keys come from the theme's background and text, not from Vaultty.
    let dark = appearance == Appearance::Dark;
    let bg = *out.entry("background").or_insert(if dark {
        Color {
            r: 30,
            g: 30,
            b: 30,
            a: 255,
        }
    } else {
        Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    });
    let fg = *out.entry("foreground").or_insert(if dark {
        Color {
            r: 204,
            g: 204,
            b: 204,
            a: 255,
        }
    } else {
        Color {
            r: 51,
            g: 51,
            b: 51,
            a: 255,
        }
    });

    let derived = [
        ("card", mix(bg, fg, 0.03)),
        ("sidebar", mix(bg, fg, 0.03)),
        ("popover", mix(bg, fg, 0.05)),
        ("secondary", mix(bg, fg, 0.1)),
        ("accent", mix(bg, fg, 0.08)),
        ("sidebar.accent", mix(bg, fg, 0.15)),
        ("border", mix(bg, fg, 0.15)),
        ("muted.foreground", mix(fg, bg, 0.4)),
    ];

    for (key, color) in derived {
        out.entry(key).or_insert(color);
    }

    let mut style = Map::new();

    for (key, color) in out {
        style.insert(key.into(), color.to_string().into());
    }

    for (i, name) in ANSI.iter().enumerate() {
        for (prefix, ours) in [("", ""), ("Bright", "bright_")] {
            if let Some(c) = colors.get(&format!("terminal.ansi{prefix}{name}")) {
                let key = format!("terminal.ansi.{ours}{}", ANSI[i].to_lowercase());
                style.insert(key, c.to_string().into());
            }
        }
    }

    style
}

/// JSON with comments and trailing commas, like VS Code's.
fn jsonc(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut string = false;

    while let Some(c) = chars.next() {
        if string {
            out.push(c);

            match c {
                '\\' => out.extend(chars.next()),
                '"' => string = false,
                _ => {}
            }

            continue;
        }

        match (c, chars.peek()) {
            ('"', _) => {
                string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';

                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }

                    last = c;
                }
            }
            _ => out.push(c),
        }
    }

    // Commas before `}` or `]`, outside strings (there are no comments left).
    let mut clean = String::with_capacity(out.len());
    let mut string = false;
    let mut escaped = false;
    let bytes: Vec<char> = out.chars().collect();

    for (i, &c) in bytes.iter().enumerate() {
        if string {
            clean.push(c);
            string = escaped || c != '"';
            escaped = !escaped && c == '\\';
            continue;
        }

        if c == '"' {
            string = true;
        }

        if c == ',' {
            let next = bytes[i + 1..].iter().find(|c| !c.is_whitespace());

            if matches!(next, Some('}' | ']')) {
                continue;
            }
        }

        clean.push(c);
    }

    serde_json::from_str(&clean)
}

/// Joins `rel` to the folder `base` inside the extension, without being able to leave it.
fn normalize(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let joined = PathBuf::from(base).join(rel);

    for part in joined.components() {
        match part {
            Component::Normal(p) => parts.push(p.to_str()?.to_string()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }

    Some(parts.join("/"))
}

/// A valid package `id`: lowercase letters, digits and hyphens.
fn slug(text: &str) -> String {
    let mut out = String::new();

    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }

    let out = out.trim_matches('-').to_string();

    if out.is_empty() {
        "vscode-theme".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const DARK: &str = r##"{
        // A test theme, with comments and trailing commas like VS Code's.
        "name": "Ocean Dark",
        "type": "dark",
        "include": "./base.json",
        "colors": {
            "editor.background": "#0a1a2a",
            "editor.foreground": "#ccddee",
            "sideBar.background": "#081522",
            "titleBar.activeBackground": "#05101a", /* the title bar */
            "button.background": "#1e90ff",
            "button.hoverBackground": "#4aa8ff",
            "terminal.ansiRed": "#f00a",
        },
        "tokenColors": []
    }"##;

    const BASE: &str =
        r##"{ "colors": { "statusBar.background": "#123456", "editor.background": "#ffffff" } }"##;

    fn vsix(dir: &Path) -> PathBuf {
        let path = dir.join("ocean.vsix");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let package = r#"{
            "name": "ocean", "displayName": "%name%", "publisher": "Someone", "version": "2.1.0",
            "repository": { "url": "https://example.com/ocean" },
            "contributes": { "themes": [
                { "label": "Ocean Dark", "uiTheme": "vs-dark", "path": "./themes/ocean.json" },
            ] }
        }"#;

        for (name, text) in [
            ("extension/package.json", package),
            ("extension/package.nls.json", r#"{ "name": "Ocean" }"#),
            ("extension/themes/ocean.json", DARK),
            ("extension/themes/base.json", BASE),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }

        zip.finish().unwrap();
        path
    }

    #[test]
    fn imports_a_vsix() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");
        let manifest = import_vscode_into(&installed, &vsix(tmp.path()))
            .unwrap()
            .manifest;

        assert_eq!(manifest.id, "someone-ocean");
        assert_eq!(manifest.name, "Ocean");
        assert_eq!(manifest.version, "2.1.0");

        let library = Library::load_from(Some(&installed));
        let theme = library.find("Ocean Dark").unwrap();
        let hex = |s: &str| Color::parse(s).unwrap();

        assert!(library.problems.is_empty(), "{:?}", library.problems);
        assert_eq!(theme.appearance, Appearance::Dark);
        // Own values win over the `include`; what is only in the `include` comes through too.
        assert_eq!(theme.ui.background, hex("#0a1a2a"));
        assert_eq!(theme.ui.statusbar_background, hex("#123456"));
        assert_eq!(theme.ui.titlebar_background, hex("#05101a"));
        assert_eq!(theme.ui.primary_hover, hex("#4aa8ff"));
        // VS Code's `#rgba`.
        assert_eq!(theme.terminal.ansi[1], hex("#ff0000aa"));
        // Without `tab.activeBackground`: the active tab is the theme's background, not Vaultty's.
        assert_eq!(theme.ui.tab_active_background, hex("#0a1a2a"));
    }

    #[test]
    fn imports_a_loose_theme() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");
        fs::write(tmp.path().join("ocean-color-theme.json"), DARK).unwrap();
        fs::write(tmp.path().join("base.json"), BASE).unwrap();

        let manifest = import_vscode_into(&installed, &tmp.path().join("ocean-color-theme.json"))
            .unwrap()
            .manifest;
        let library = Library::load_from(Some(&installed));

        assert_eq!(manifest.id, "ocean-dark");
        assert!(library.find("Ocean Dark").is_some());
    }

    /// What the theme does not define comes from its background and text: none of Vaultty's grays.
    #[test]
    fn missing_keys_come_from_the_theme() {
        let colors = BTreeMap::from([
            (
                "editor.background".to_string(),
                Color::parse("#000080").unwrap(),
            ),
            (
                "editor.foreground".to_string(),
                Color::parse("#ffffff").unwrap(),
            ),
        ]);
        let style = style(&colors, Appearance::Dark);

        assert_eq!(style["border"], "#262693");
        assert_eq!(style["sidebar"], "#080884");
    }

    /// A theme that already exists is skipped and the others are imported; if all exist, it is an error.
    #[test]
    fn skips_existing_themes() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");
        let theme =
            |name: &str| format!(r#"{{ "name": "{name}", "type": "dark", "colors": {{}} }}"#);
        fs::write(tmp.path().join("a.json"), theme("Dracula")).unwrap();
        fs::write(tmp.path().join("b.json"), theme("Dracula Night")).unwrap();

        let error = import_vscode_into(&installed, &tmp.path().join("a.json")).unwrap_err();
        assert!(error.contains("Already installed: Dracula"), "{error}");

        let ok = import_vscode_into(&installed, &tmp.path().join("b.json")).unwrap();
        assert!(ok.skipped.is_empty());
    }

    #[test]
    fn a_transparent_border_is_not_a_base_value() {
        let colors = BTreeMap::from([
            (
                "editor.background".to_string(),
                Color::parse("#000080").unwrap(),
            ),
            (
                "editor.foreground".to_string(),
                Color::parse("#ffffff").unwrap(),
            ),
            (
                "sideBar.border".to_string(),
                Color::parse("#00000000").unwrap(),
            ),
        ]);
        let style = style(&colors, Appearance::Dark);

        assert_eq!(style["border"], "#262693");
        assert_eq!(style["sidebar.border"], "#00000000");
    }

    #[test]
    fn an_include_cannot_leave_the_extension() {
        assert_eq!(
            normalize("themes", "../base.json").as_deref(),
            Some("base.json")
        );
        assert_eq!(normalize("", "../../etc/passwd"), None);
        assert_eq!(slug("Some One / Ocean++"), "some-one-ocean");
    }
}
