// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Icons: Lucide icons and operating system logos.

use crate::app::theme::px;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{AnyElement, Hsla, IntoElement, SharedString, Styled, svg};

/// A Lucide icon with the given size and color.
pub fn icon(name: IconName, size: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

/// A Lucide icon that takes the color of the surrounding text (inside buttons, menus…).
pub fn glyph(name: IconName, size: f32) -> Icon {
    Icon::new(name).size(px(size))
}

/// Logo of the host's operating system, or `fallback` if it is not known yet. An OS that is not in the
/// list shows Tux: whatever has os-release and we don't recognize is almost always a Linux.
pub fn os_icon(os: Option<&str>, fallback: IconName, size: f32, color: Hsla) -> AnyElement {
    let Some(os) = os else {
        return icon(fallback, size, color).into_any_element();
    };

    let (file, hex) = match os {
        "almalinux" => ("almalinux", 0x000000),
        "alpine" => ("alpinelinux", 0x0D597F),
        "arch" => ("archlinux", 0x1793D1),
        "centos" => ("centos", 0x262577),
        "debian" => ("debian", 0xA81D33),
        "fedora" => ("fedora", 0x51A2DA),
        "freebsd" => ("freebsd", 0xAB2B28),
        "gentoo" => ("gentoo", 0x54487A),
        "kali" => ("kalilinux", 0x557C94),
        "linuxmint" => ("linuxmint", 0x86BE43),
        "macos" => ("apple", 0x000000),
        "manjaro" => ("manjaro", 0x35BFA4),
        "nixos" => ("nixos", 0x5277C3),
        "openbsd" => ("openbsd", 0xF2CA30),
        "opensuse-leap" | "opensuse-tumbleweed" => ("opensuse", 0x73BA25),
        "pop" => ("popos", 0x48B9C7),
        "raspbian" => ("raspberrypi", 0xA22846),
        "rhel" => ("redhat", 0xEE0000),
        "rocky" => ("rockylinux", 0x10B981),
        "ubuntu" => ("ubuntu", 0xE95420),
        "windows" => ("windows", 0x0078D4),
        _ => ("linux", 0xFCC624),
    };

    // Near-black logos (Apple, AlmaLinux…) would not be visible in the dark theme: they use the text color.
    let dark = (hex >> 16) + ((hex >> 8) & 255) + (hex & 255) < 200;
    let tint: Hsla = if dark {
        color
    } else {
        gpui_kit::rgb(hex).into()
    };

    svg()
        .path(SharedString::from(format!("os/{file}.svg")))
        .size(px(size))
        .flex_shrink_0()
        .text_color(tint)
        .into_any_element()
}
