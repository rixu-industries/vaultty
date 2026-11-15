// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Checkbox with text, built on the unstyled gpui-kit checkbox (`base`): it handles focus, keyboard and
//! accessibility; the square, the check mark and the color live here.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::icon;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Checkbox as BaseCheckbox, CheckboxState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// Checkbox with its text on the right. `on_click` receives the new value.
#[derive(IntoElement)]
pub struct Checkbox {
    id: gpui_kit::ElementId,
    checked: bool,
    label: Option<SharedString>,
    on_click: Option<Handler>,
}

impl Checkbox {
    pub fn new(id: impl Into<gpui_kit::ElementId>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            label: None,
            on_click: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let c = colors();
        let checked = self.checked;
        let on_click = self.on_click;

        let square = div()
            .size(px(16.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .border_1()
            .border_color(if checked { c.primary } else { c.border })
            .when(checked, |this| {
                this.bg(c.primary)
                    .child(icon(IconName::Check, 12., c.primary_foreground))
            });

        BaseCheckbox::new(self.id)
            .checked(checked)
            .when_some(self.label.clone(), |this, label| {
                this.accessibility_label(label)
            })
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .when_some(on_click, |this, f| {
                this.on_change(move |state, _, window, cx| {
                    f(&(state == CheckboxState::Checked), window, cx)
                })
            })
            .child(square)
            .children(self.label)
    }
}
