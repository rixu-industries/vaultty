// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Theme packages: a folder with a `manifest.json` and the theme JSON files it lists. Installed ones
//! live in `<config>/themes/<id>/`; installing copies the folder there, and uninstalling deletes it.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::style::{Family, Theme};
use crate::{Appearance, AppearanceSettings, DEFAULT_DARK, DEFAULT_LIGHT, SCHEMA_VERSION};

const MANIFEST: &str = "manifest.json";

/// Built-in packages, compiled into the binary: (manifest, themes file). The first is Vaultty's, which
/// has every key and is the base for the others; the rest are the most used VS Code themes,
/// converted to this format.
macro_rules! builtin {
    ($($id:literal),+ $(,)?) => {
        [$((
            include_str!(concat!("../builtin/", $id, "/manifest.json")),
            include_str!(concat!("../builtin/", $id, "/themes/", $id, ".json")),
        )),+]
    };
}

const BUILTIN: [(&str, &str); 12] = builtin![
    "vaultty",
    "vscode",
    "one-dark-pro",
    "dracula",
    "github",
    "catppuccin",
    "tokyo-night",
    "nord",
    "gruvbox",
    "solarized",
    "monokai",
    "night-owl",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Name of the folder it is installed in: lowercase letters, digits and hyphens.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Theme files, relative to the package folder.
    pub themes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub manifest: Manifest,
    /// `None`: the built-in one, which is inside the binary and cannot be uninstalled.
    pub dir: Option<PathBuf>,
}

/// All the themes there are: the built-in ones first, then the installed ones.
#[derive(Clone, Debug)]
pub struct Library {
    pub themes: Vec<Theme>,
    pub packages: Vec<Package>,
    /// What could not be loaded, or was only partly loaded, to show in Settings.
    pub problems: Vec<String>,
}

/// Folder of the installed themes.
pub fn themes_dir() -> Option<PathBuf> {
    Some(settings::dir()?.join("themes"))
}

impl Library {
    pub fn load() -> Self {
        Self::load_from(themes_dir().as_deref())
    }

    pub fn load_from(dir: Option<&Path>) -> Self {
        let mut library = Self::builtin();

        let Some(dir) = dir else {
            return library;
        };

        let Ok(entries) = fs::read_dir(dir) else {
            return library;
        };

        let mut folders: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        folders.sort();

        for folder in folders {
            if let Err(e) = library.add(&folder) {
                library.problems.push(e);
            }
        }

        library
    }

    fn builtin() -> Self {
        let mut library = Self {
            themes: Vec::new(),
            packages: Vec::new(),
            problems: Vec::new(),
        };

        // Vaultty has no base: it must include every key (`build` checks this).
        let (manifest, themes) = BUILTIN[0];
        let manifest: Manifest = serde_json::from_str(manifest).expect("valid built-in manifest");
        let family: Family = serde_json::from_str(themes).expect("built-in themes are valid");

        for def in family.themes {
            let (theme, _) =
                Theme::build(def, &manifest.id, None).expect("Vaultty's themes include every key");
            library.themes.push(theme);
        }

        library.packages.push(Package {
            manifest,
            dir: None,
        });

        // The others go through the same steps as an installed one: validation and inheritance.
        for (manifest, themes) in &BUILTIN[1..] {
            let (manifest, themes, warnings) = library
                .parse(manifest.as_bytes(), "built-in", |_| {
                    Ok(themes.as_bytes().to_vec())
                })
                .expect("the built-in packages are valid");

            debug_assert!(warnings.is_empty(), "{warnings:?}");
            library.themes.extend(themes);
            library.packages.push(Package {
                manifest,
                dir: None,
            });
        }

        library
    }

    /// Loads a package from a folder. If it is not valid as a whole, nothing is added.
    fn add(&mut self, folder: &Path) -> Result<(), String> {
        let (manifest, themes, warnings) = self.read(folder)?;

        self.problems.extend(warnings);
        self.themes.extend(themes);
        self.packages.push(Package {
            manifest,
            dir: Some(folder.to_path_buf()),
        });

        Ok(())
    }

    /// Reads and validates a package from a folder without adding it.
    fn read(&self, folder: &Path) -> Result<(Manifest, Vec<Theme>, Vec<String>), String> {
        let shown = folder.display().to_string();
        let bytes = fs::read(folder.join(MANIFEST))
            .map_err(|_| format!("{shown}: there is no {MANIFEST}"))?;

        self.parse(&bytes, &shown, |file| fs::read(folder.join(file)))
    }

    /// Validates a package. `open` gives the content of each themes file in the manifest.
    fn parse(
        &self,
        manifest: &[u8],
        shown: &str,
        open: impl Fn(&str) -> std::io::Result<Vec<u8>>,
    ) -> Result<(Manifest, Vec<Theme>, Vec<String>), String> {
        let manifest: Manifest = serde_json::from_slice(manifest)
            .map_err(|e| format!("{shown}/{MANIFEST} is not valid: {e}"))?;

        if manifest.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "{}: schema_version {} is not supported (this app reads {SCHEMA_VERSION})",
                manifest.id, manifest.schema_version
            ));
        }

        if !valid_id(&manifest.id) {
            return Err(format!(
                "{shown}: id \"{}\" must use only a-z, 0-9 and -",
                manifest.id
            ));
        }

        if self.packages.iter().any(|p| p.manifest.id == manifest.id) {
            return Err(format!("{}: another package has this id", manifest.id));
        }

        let mut themes: Vec<Theme> = Vec::new();
        let mut warnings = Vec::new();

        for file in &manifest.themes {
            if !inside(file) {
                return Err(format!(
                    "{}: \"{file}\" must be inside the package",
                    manifest.id
                ));
            }

            let bytes = open(file).map_err(|e| format!("{}/{file}: {e}", manifest.id))?;
            let family: Family = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{}/{file} is not valid: {e}", manifest.id))?;

            for def in family.themes {
                let taken = self
                    .themes
                    .iter()
                    .chain(&themes)
                    .any(|t| t.name == def.name);

                if taken {
                    return Err(format!(
                        "{}: there is already a theme called \"{}\"",
                        manifest.id, def.name
                    ));
                }

                // Missing keys come from the built-in theme of the same appearance.
                let base = self.builtin_of(def.appearance);
                let (theme, w) = Theme::build(def, &manifest.id, Some(base))
                    .map_err(|e| format!("{}: {e}", manifest.id))?;

                themes.push(theme);
                warnings.extend(w.into_iter().map(|w| format!("{}: {w}", manifest.id)));
            }
        }

        if themes.is_empty() {
            return Err(format!("{}: the package has no themes", manifest.id));
        }

        Ok((manifest, themes, warnings))
    }

    pub fn find(&self, name: &str) -> Option<&Theme> {
        self.themes.iter().find(|t| t.name == name)
    }

    fn builtin_of(&self, appearance: Appearance) -> &Theme {
        let name = match appearance {
            Appearance::Light => DEFAULT_LIGHT,
            Appearance::Dark => DEFAULT_DARK,
        };

        self.find(name)
            .expect("the built-in themes are always present")
    }

    /// The theme that applies with these settings. If the chosen one is gone (it was uninstalled) or is of the other
    /// appearance, the built-in one.
    pub fn pick(&self, settings: &AppearanceSettings, system: Appearance) -> &Theme {
        let appearance = settings.appearance(system);

        self.find(settings.theme_for(appearance))
            .filter(|t| t.appearance == appearance)
            .unwrap_or_else(|| self.builtin_of(appearance))
    }

    pub fn of(&self, appearance: Appearance) -> impl Iterator<Item = &Theme> {
        self.themes
            .iter()
            .filter(move |t| t.appearance == appearance)
    }
}

/// Copies a package into the themes folder. If it is already there (same `id`), it is replaced: this is how it gets updated.
pub fn install(from: &Path) -> Result<Manifest, String> {
    let dir = themes_dir().ok_or("System configuration folder not found")?;
    install_into(&dir, from)
}

pub fn install_into(dir: &Path, from: &Path) -> Result<Manifest, String> {
    // It is validated against the built-in and the other installed themes, without the one being replaced.
    let probe = Library::builtin();
    let (manifest, themes, _) = probe.read(from)?;
    let others = Library::load_from(Some(dir));

    for theme in &themes {
        let clash = others
            .themes
            .iter()
            .any(|t| t.name == theme.name && t.package != manifest.id);

        if clash {
            return Err(format!(
                "There is already a theme called \"{}\" in another package",
                theme.name
            ));
        }
    }

    let target = dir.join(&manifest.id);
    let error = |e: std::io::Error| format!("Could not install the theme: {e}");

    if target.exists() {
        fs::remove_dir_all(&target).map_err(error)?;
    }

    copy_dir(from, &target).map_err(error)?;
    Ok(manifest)
}

pub fn uninstall(id: &str) -> Result<(), String> {
    let dir = themes_dir().ok_or("System configuration folder not found")?;
    uninstall_from(&dir, id)
}

pub fn uninstall_from(dir: &Path, id: &str) -> Result<(), String> {
    let builtin = Library::builtin()
        .packages
        .iter()
        .any(|p| p.manifest.id == id);

    if !valid_id(id) || builtin {
        return Err(format!("\"{id}\" cannot be uninstalled"));
    }

    fs::remove_dir_all(dir.join(id)).map_err(|e| format!("Could not uninstall the theme: {e}"))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The path must not leave the package (`../`, absolute paths).
fn inside(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
}

/// Copies the folder without hidden entries (`.git`, `.DS_Store`): they are not needed for themes.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;

    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();

        if name.to_string_lossy().starts_with('.') {
            continue;
        }

        let path = entry.path();

        if entry.file_type()?.is_dir() {
            copy_dir(&path, &to.join(&name))?;
        } else {
            fs::copy(&path, to.join(&name))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Mode};

    fn write_package(dir: &Path, id: &str, themes: &str) -> PathBuf {
        let folder = dir.join(format!("src-{id}"));
        fs::create_dir_all(folder.join("themes")).unwrap();
        fs::write(
            folder.join(MANIFEST),
            format!(
                r#"{{"schema_version":1,"id":"{id}","name":"Demo","version":"1.0.0","themes":["themes/demo.json"]}}"#
            ),
        )
        .unwrap();
        fs::write(folder.join("themes/demo.json"), themes).unwrap();
        folder
    }

    const OCEAN: &str = r##"{"name":"Ocean","themes":[
        {"name":"Ocean Light","appearance":"light","style":{"background":"#e0f0ff","terminal.ansi.red":"#ff0000","bogus":"#000"}}
    ]}"##;

    #[test]
    fn example_installs_without_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/ember");

        install_into(tmp.path(), &example).unwrap();

        let library = Library::load_from(Some(tmp.path()));
        assert_eq!(library.themes.len(), Library::builtin().themes.len() + 2);
        assert!(library.problems.is_empty(), "{:?}", library.problems);
    }

    #[test]
    fn builtin_has_dark_and_light() {
        let library = Library::load_from(None);

        assert_eq!(
            library.find(DEFAULT_DARK).unwrap().appearance,
            Appearance::Dark
        );
        assert_eq!(
            library.find(DEFAULT_LIGHT).unwrap().appearance,
            Appearance::Light
        );
        assert!(library.problems.is_empty());
    }

    #[test]
    fn missing_keys_come_from_builtin_of_same_appearance() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");
        let from = write_package(tmp.path(), "ocean", OCEAN);

        install_into(&installed, &from).unwrap();

        let library = Library::load_from(Some(&installed));
        let ocean = library.find("Ocean Light").unwrap();
        let light = library.find(DEFAULT_LIGHT).unwrap();
        let blue = Color::parse("#e0f0ff").unwrap();

        assert_eq!(ocean.package, "ocean");
        assert_eq!(ocean.ui.background, blue);
        assert_eq!(ocean.ui.border, light.ui.border);
        // The terminal follows the theme's background, not the built-in one's.
        assert_eq!(ocean.terminal.background, blue);
        assert_eq!(ocean.terminal.ansi[1], Color::parse("#ff0000").unwrap());
        assert_eq!(ocean.terminal.ansi[2], light.terminal.ansi[2]);
        assert!(library.problems.iter().any(|p| p.contains("bogus")));
    }

    #[test]
    fn reinstalling_updates_and_uninstalling_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");
        let from = write_package(tmp.path(), "ocean", OCEAN);

        install_into(&installed, &from).unwrap();
        install_into(&installed, &from).unwrap();
        assert_eq!(
            Library::load_from(Some(&installed)).packages.len(),
            BUILTIN.len() + 1
        );

        uninstall_from(&installed, "ocean").unwrap();
        assert!(
            Library::load_from(Some(&installed))
                .find("Ocean Light")
                .is_none()
        );
        assert!(uninstall_from(&installed, "vaultty").is_err());
        assert!(uninstall_from(&installed, "dracula").is_err());
        assert!(uninstall_from(&installed, "../x").is_err());
    }

    #[test]
    fn rejects_invalid_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let installed = tmp.path().join("themes");

        let clash = r#"{"name":"X","themes":[{"name":"Vaultty Dark","appearance":"dark"}]}"#;
        let outside = write_package(tmp.path(), "outside", OCEAN);
        fs::write(
            outside.join(MANIFEST),
            r#"{"schema_version":1,"id":"outside","name":"X","version":"1","themes":["../x.json"]}"#,
        )
        .unwrap();
        let future = write_package(tmp.path(), "future", OCEAN);
        fs::write(
            future.join(MANIFEST),
            r#"{"schema_version":2,"id":"future","name":"X","version":"1","themes":["themes/demo.json"]}"#,
        )
        .unwrap();

        assert!(install_into(&installed, &write_package(tmp.path(), "clash", clash)).is_err());
        assert!(install_into(&installed, &write_package(tmp.path(), "Bad_Id", OCEAN)).is_err());
        assert!(install_into(&installed, &outside).is_err());
        assert!(install_into(&installed, &future).is_err());
        assert!(!installed.exists());
    }

    #[test]
    fn missing_chosen_theme_falls_back_to_builtin() {
        let library = Library::load_from(None);
        let settings = AppearanceSettings {
            mode: Mode::System,
            dark: "Gone".into(),
            ..AppearanceSettings::default()
        };

        assert_eq!(library.pick(&settings, Appearance::Dark).name, DEFAULT_DARK);
        assert_eq!(
            library.pick(&settings, Appearance::Light).name,
            DEFAULT_LIGHT
        );

        let forced = AppearanceSettings {
            mode: Mode::Light,
            ..AppearanceSettings::default()
        };
        assert_eq!(library.pick(&forced, Appearance::Dark).name, DEFAULT_LIGHT);
    }
}
