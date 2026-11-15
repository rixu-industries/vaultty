// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Switch, built on the unstyled gpui-kit switch (`base`): it handles focus, keyboard and
//! accessibility; the track, the thumb and the color live here.

use crate::app::theme::colors;
use crate::app::theme::px;
use gpui_kit::base::{Switch as BaseSwitch, SwitchThumb, SwitchTrack};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// Switch. `on_click` receives the new value. The text next to it is set by the caller; `label`
/// is only the name screen readers announce.
#[derive(IntoElement)]
pub struct Switch {
    id: gpui_kit::ElementId,
    checked: bool,
    disabled: bool,
    label: SharedString,
    on_click: Option<Handler>,
}

impl Switch {
    pub fn new(id: impl Into<gpui_kit::ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            checked: false,
            disabled: false,
            label: label.into(),
            on_click: None,
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let c = colors();
        let (checked, disabled) = (self.checked, self.disabled);
        let on_click = self.on_click;

        let track = SwitchTrack::new((self.id.clone(), "track"))
            .checked(checked)
            .disabled(disabled)
            .flex()
            .items_center()
            .w(px(32.))
            .h(px(18.))
            .px(px(2.))
            .rounded_full()
            .border_1()
            .border_color(if checked { c.primary } else { c.border })
            .bg(if checked { c.primary } else { c.secondary })
            .when(checked, |this| this.justify_end())
            .child(
                SwitchThumb::new(checked)
                    .disabled(disabled)
                    .size(px(12.))
                    .rounded_full()
                    .bg(if checked {
                        c.primary_foreground
                    } else {
                        c.muted_foreground
                    }),
            );

        BaseSwitch::new(self.id)
            .checked(checked)
            .disabled(disabled)
            .accessibility_label(self.label)
            .flex_shrink_0()
            .when(!disabled, |this| this.cursor_pointer())
            .when(disabled, |this| this.opacity(0.5))
            .when_some(on_click, |this, f| {
                this.on_change(move |next, _, window, cx| f(&next, window, cx))
            })
            .child(track)
    }
}
