// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Everything visual in the app. `ui/` holds the primitives (button, field, checkbox, badge…), and the pieces
//! outside `ui/` already know app concepts (confirming a deletion, the access method…).
//!
//! The app imports nothing from `gpui_kit::component` except the theme and the title bar: if a new piece is needed,
//! it gets added here.

mod confirm_dialog;
mod drag_label;
mod method_badge;
mod ui;

pub use confirm_dialog::*;
pub use drag_label::*;
pub use method_badge::*;
pub use ui::button::*;
pub use ui::checkbox::*;
pub use ui::dialog::*;
pub use ui::empty_state::*;
pub use ui::field::*;
pub use ui::icon::*;
pub use ui::input::*;
pub use ui::menu::*;
pub use ui::property::*;
pub use ui::row::*;
pub use ui::section_title::*;
pub use ui::select::*;
pub use ui::switch::*;
