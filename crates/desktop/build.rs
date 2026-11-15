// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

// On Windows, gpui takes the window and taskbar icon from resource 1 of the .exe.
// The file metadata is also checked by code signing: the product name must be the project
// name (winresource defaults to the crate name, "desktop"). The file and product versions
// stay at the default, the package version, which the Release workflow sets.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico")
            .set("ProductName", "Vaultty")
            .set("FileDescription", "Vaultty")
            .set("CompanyName", "Rixu Industries")
            .set(
                "LegalCopyright",
                "Copyright (C) 2026 José García \"Ruxwez\" (Rixu Industries). GPL-3.0-or-later",
            )
            .set("InternalName", "vaultty")
            .set("OriginalFilename", "vaultty.exe");
        res.compile().expect("could not embed the icon");
    }
}
