// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! UI primitives, in the shadcn style: each one wraps an unstyled gpui-kit component
//! (`gpui_kit::base`) and applies the app's look, with its variants. They don't know the app state.
//!
//! `base` has no menus, stacked dialogs or icons: those three come from `gpui_kit::component`, but are only
//! touched from here (`menu`, `dialog`, `icon`).

pub mod button;
pub mod checkbox;
pub mod dialog;
pub mod empty_state;
pub mod field;
pub mod icon;
pub mod input;
pub mod menu;
pub mod property;
pub mod row;
pub mod section_title;
pub mod select;
pub mod switch;
