// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Dropdown to pick an option from a list.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::icon;
use crate::components::{Button, ButtonVariant};
use crate::components::{DropdownMenu, PopupMenuItem};
use gpui_kit::assets::IconName;
use gpui_kit::{App, IntoElement, ParentElement, Styled, Window, div};

/// A dropdown with the `options` and the chosen one (`selected`).
pub fn choice<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, String)>,
    selected: Option<T>,
    on_pick: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let c = colors();
    let label = options
        .iter()
        .find(|(v, _)| Some(v) == selected.as_ref())
        .map(|(_, l)| l.clone())
        .unwrap_or_default();
    let on_pick = std::rc::Rc::new(on_pick);

    Button::new(id)
        .variant(ButtonVariant::Outline)
        .w_full()
        .child(
            div()
                .flex()
                .items_center()
                .w_full()
                .justify_between()
                .child(div().truncate().text_color(c.foreground).child(label))
                .child(icon(IconName::ChevronDown, 12., c.muted_foreground)),
        )
        .dropdown_menu(move |menu, _, _| {
            let mut menu = menu.min_w(px(240.)).max_h(px(240.)).scrollable(true);

            for (value, label) in options.clone() {
                let on_pick = on_pick.clone();
                let checked = Some(&value) == selected.as_ref();

                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(checked)
                        .on_click(move |_, window, cx| on_pick(value.clone(), window, cx)),
                );
            }

            menu
        })
}
