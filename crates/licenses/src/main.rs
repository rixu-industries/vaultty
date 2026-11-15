// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Generates `crates/desktop/assets/licenses.json`: the licenses of everything that ships inside the app
//! (Rust crates, built-in themes, icons and logos), for Help → Open Source Licenses. The MIT, BSD, Apache… licenses
//! require including their notice when the binary is distributed, so this is always done by hand.
//!
//! `cargo run -p licenses` (or `just licenses`) when dependencies or themes change. A test in
//! `desktop` fails if a version is no longer in `Cargo.lock` or if a theme is missing.
//!
//! - Crates: those `desktop` reaches through normal dependencies (not build or test ones) on
//!   Windows, macOS and Linux, excluding the workspace's own. From each one, the `LICENSE*`,
//!   `COPYING*` and `NOTICE*` files are taken. If the license is "A OR B", only the chosen one's text (MIT first).
//!   If it has none, the standard text with its authors.
//! - Repeated texts (almost all Apache-2.0 ones are identical) are stored once.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The platforms the app is distributed for.
const TARGETS: [&str; 4] = [
    "x86_64-pc-windows-msvc",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
];

/// When a license offers a choice, it is picked in this order.
const PREFERRED: [&str; 9] = [
    "MIT",
    "Apache-2.0",
    "ISC",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "Zlib",
    "0BSD",
    "Unlicense",
    "BSL-1.0",
];

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_root: PathBuf,
    resolve: Resolve,
}

#[derive(Deserialize, Clone)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    license_file: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dep>,
}

#[derive(Deserialize)]
struct Dep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct DepKind {
    kind: Option<String>,
}

#[derive(Serialize)]
struct Output {
    /// License texts, without repeats.
    texts: Vec<String>,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct Entry {
    /// `theme`, `asset`, or `crate`.
    group: &'static str,
    name: String,
    version: String,
    license: String,
    url: Option<String>,
    /// Indices into `texts`.
    texts: Vec<usize>,
}

/// Collects the texts without repeats.
#[derive(Default)]
struct Texts {
    list: Vec<String>,
    index: HashMap<String, usize>,
}

impl Texts {
    fn add(&mut self, text: &str) -> usize {
        let text = normalize(text);

        if let Some(i) = self.index.get(&text) {
            return *i;
        }

        self.list.push(text.clone());
        self.index.insert(text, self.list.len() - 1);
        self.list.len() - 1
    }
}

fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim_end)
        .collect();

    // Some texts have runs of blank lines: one is enough.
    let mut out: Vec<&str> = Vec::new();

    for line in lines {
        if line.is_empty() && out.last().is_some_and(|l| l.is_empty()) {
            continue;
        }

        out.push(line);
    }

    out.join("\n").trim_matches('\n').to_string()
}

fn metadata(target: &str) -> Metadata {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ])
        .output()
        .expect("could not run cargo metadata");

    assert!(out.status.success(), "cargo metadata failed for {target}");
    serde_json::from_slice(&out.stdout).expect("cargo metadata output")
}

/// The packages `desktop` reaches through normal dependencies on a platform.
fn shipped(meta: &Metadata) -> Vec<Package> {
    let packages: HashMap<&str, &Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: HashMap<&str, &Node> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n))
        .collect();
    let members: BTreeSet<&str> = meta.workspace_members.iter().map(String::as_str).collect();
    let root = meta
        .workspace_members
        .iter()
        .find(|id| packages[id.as_str()].name == "desktop")
        .expect("the workspace must have `desktop`");

    let mut seen = BTreeSet::new();
    let mut stack = vec![root.as_str()];

    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }

        for dep in &nodes[id].deps {
            if dep.dep_kinds.iter().any(|k| k.kind.is_none()) {
                stack.push(dep.pkg.as_str());
            }
        }
    }

    seen.into_iter()
        .filter(|id| !members.contains(id))
        .map(|id| packages[id].clone())
        .collect()
}

/// The license to use from an SPDX expression: with `OR` (or `/`), the preferred one.
fn chosen(expression: &str) -> Option<&'static str> {
    if expression.contains(" AND ") {
        return None;
    }

    let options: Vec<String> = expression
        .replace(['(', ')'], "")
        .split([' ', '/'])
        .filter(|s| !s.is_empty() && *s != "OR")
        .map(String::from)
        .collect();

    PREFERRED
        .iter()
        .find(|p| options.iter().any(|o| o == *p))
        .copied()
}

/// Which license a file belongs to, based on its name (`LICENSE-MIT`, `LICENSE-APACHE`…).
fn kind_of(file: &str) -> Option<&'static str> {
    let upper = file.to_uppercase();

    [
        ("APACHE", "Apache-2.0"),
        ("MIT", "MIT"),
        ("ISC", "ISC"),
        ("BSD", "BSD-3-Clause"),
        ("ZLIB", "Zlib"),
        ("UNLICENSE", "Unlicense"),
        ("BOOST", "BSL-1.0"),
    ]
    .into_iter()
    .find(|(key, _)| upper.contains(key))
    .map(|(_, kind)| kind)
}

fn license_files(dir: &Path, extra: Option<&str>) -> Vec<(String, String)> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let upper = name.to_uppercase();
            let wanted = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE"]
                .iter()
                .any(|p| upper.starts_with(p));

            if wanted
                && entry.path().is_file()
                && let Ok(text) = fs::read_to_string(entry.path())
            {
                files.push((name, text));
            }
        }
    }

    if let Some(extra) = extra
        && !files.iter().any(|(n, _)| n == extra)
        && let Ok(text) = fs::read_to_string(dir.join(extra))
    {
        files.push((extra.to_string(), text));
    }

    files.sort();
    files
}

fn template(root: &Path, license: &str, package: &Package) -> Option<String> {
    let text =
        fs::read_to_string(root.join(format!("crates/licenses/templates/{license}.txt"))).ok()?;
    let authors: Vec<String> = package
        .authors
        .iter()
        .map(|a| a.split(" <").next().unwrap_or(a).to_string())
        .collect();
    let holders = if authors.is_empty() {
        format!("The {} developers", package.name)
    } else {
        authors.join(", ")
    };

    Some(text.replace("{copyright}", &format!("Copyright (c) {holders}")))
}

fn main() {
    let first = metadata(TARGETS[0]);
    let root = first.workspace_root.clone();
    let mut packages: BTreeMap<(String, String), Package> = BTreeMap::new();

    for meta in std::iter::once(first).chain(TARGETS[1..].iter().map(|t| metadata(t))) {
        for p in shipped(&meta) {
            packages.insert((p.name.clone(), p.version.clone()), p);
        }
    }

    let mut texts = Texts::default();
    let mut entries = Vec::new();
    let mut missing = Vec::new();

    // Built-in themes: their LICENSE, with the original's notice.
    let builtin = root.join("crates/themes/builtin");
    let mut themes: Vec<PathBuf> = fs::read_dir(&builtin)
        .expect("crates/themes/builtin")
        .flatten()
        .map(|e| e.path())
        .collect();
    themes.sort();

    for dir in themes {
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();

        if manifest["id"] == "vaultty" {
            continue;
        }

        let Ok(text) = fs::read_to_string(dir.join("LICENSE")) else {
            missing.push(format!("theme {}", manifest["id"]));
            continue;
        };

        entries.push(Entry {
            group: "theme",
            name: format!("{} theme", manifest["name"].as_str().unwrap_or_default()),
            version: String::new(),
            license: manifest["license"].as_str().unwrap_or("MIT").to_string(),
            url: manifest["repository"].as_str().map(String::from),
            texts: vec![texts.add(&text)],
        });
    }

    // Icons and logos. Lucide lives inside gpui-kit-assets; the logos are in `assets/os`.
    if let Some(assets) = packages.values().find(|p| p.name == "gpui-kit-assets") {
        let dir = assets.manifest_path.parent().unwrap();

        if let Ok(text) = fs::read_to_string(dir.join("LICENSE-LUCIDE")) {
            entries.push(Entry {
                group: "asset",
                name: "Lucide icons".into(),
                version: String::new(),
                license: "ISC".into(),
                url: Some("https://lucide.dev".into()),
                texts: vec![texts.add(&text)],
            });
        }
    }

    entries.push(Entry {
        group: "asset",
        name: "Simple Icons (operating system logos)".into(),
        version: String::new(),
        license: "CC0-1.0".into(),
        url: Some("https://simpleicons.org".into()),
        texts: vec![texts.add(
            "The operating system logos come from Simple Icons and are dedicated to the public domain under CC0 1.0 Universal.\nhttps://creativecommons.org/publicdomain/zero/1.0/\n\nBrand names and logos belong to their owners.",
        )],
    });

    // Crates.
    for p in packages.values() {
        let expression = p
            .license
            .clone()
            .unwrap_or_else(|| "see the license file".into());
        let dir = p.manifest_path.parent().unwrap();
        let files = license_files(dir, p.license_file.as_deref());
        let pick = chosen(&expression);

        let (notices, licenses): (Vec<_>, Vec<_>) = files
            .into_iter()
            .partition(|(name, _)| name.to_uppercase().starts_with("NOTICE"));

        let kept: Vec<(String, String)> = match pick {
            Some(pick) => {
                let named: Vec<_> = licenses
                    .iter()
                    .filter(|(n, _)| kind_of(n) == Some(pick))
                    .cloned()
                    .collect();
                let plain: Vec<_> = licenses
                    .iter()
                    .filter(|(n, _)| kind_of(n).is_none())
                    .cloned()
                    .collect();

                if !named.is_empty() {
                    named
                } else if !plain.is_empty() {
                    plain
                } else {
                    licenses
                }
            }
            None => licenses,
        };

        let mut ids: Vec<usize> = kept.iter().map(|(_, t)| texts.add(t)).collect();

        if ids.is_empty() {
            match pick.and_then(|l| template(&root, l, p)) {
                Some(text) => ids.push(texts.add(&text)),
                None => missing.push(format!("{} {} ({expression})", p.name, p.version)),
            }
        }

        ids.extend(notices.iter().map(|(_, t)| texts.add(t)));

        entries.push(Entry {
            group: "crate",
            name: p.name.clone(),
            version: p.version.clone(),
            license: expression,
            url: p.repository.clone().or_else(|| p.homepage.clone()),
            texts: ids,
        });
    }

    let out = Output {
        texts: texts.list,
        entries,
    };
    let path = root.join("crates/desktop/assets/licenses.json");
    let json = serde_json::to_string(&out).unwrap() + "\n";
    fs::write(&path, &json).unwrap();

    println!(
        "{}: {} entries, {} texts, {} KB",
        path.display(),
        out.entries.len(),
        out.texts.len(),
        json.len() / 1024
    );

    for m in &missing {
        println!("no license text: {m}");
    }
}
