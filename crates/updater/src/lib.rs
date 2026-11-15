// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Vaultty updates. Each version is a release of `rixu-industries/vaultty` with the
//! installer, its signature and a `latest.json`. Only what is signed with the key in
//! `updater.key.pub` gets installed, even if someone takes over the releases repo.

pub use cargo_packager_updater::{Error, Update};

/// GitHub redirects `latest/download` to the latest published release.
const ENDPOINT: &str =
    "https://github.com/rixu-industries/vaultty/releases/latest/download/latest.json";

/// Checks whether a version newer than this one exists. Blocks while the request runs.
pub fn check() -> Result<Option<Update>, Error> {
    if !self_updatable() {
        return Ok(None);
    }

    let config = cargo_packager_updater::Config {
        endpoints: vec![ENDPOINT.parse().expect("fixed URL")],
        pubkey: include_str!("../updater.key.pub").trim().into(),
        ..Default::default()
    };

    cargo_packager_updater::check_update(
        env!("CARGO_PKG_VERSION")
            .parse()
            .expect("version from Cargo.toml"),
        config,
    )
}

/// Whether this copy was installed from a package the updater can replace. On Linux the updater
/// overwrites the running file with the new AppImage, and on macOS it replaces the enclosing
/// `.app`; for a build from source or a distro package that would clobber the wrong file, so
/// those copies are left to be updated the way they were installed.
fn self_updatable() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var_os("APPIMAGE").is_some()
    } else if cfg!(target_os = "macos") {
        std::env::current_exe().is_ok_and(|exe| {
            exe.ancestors()
                .any(|dir| dir.extension().is_some_and(|ext| ext == "app"))
        })
    } else {
        true
    }
}
