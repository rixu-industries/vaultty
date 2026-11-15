// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! App-styled button, built on the unstyled gpui-kit button (`base`): it handles
//! focus, keyboard and accessibility; the variants and sizes live here, as in shadcn.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::glyph;
use gpui_kit::assets::IconName;
use gpui_kit::base::{Button as BaseButton, Selectable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, ClickEvent, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels,
    Refineable as _, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled,
    Window,
};
use std::rc::Rc;

type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Main action, in the accent color. One per screen.
    Primary,
    #[default]
    Secondary,
    Outline,
    /// No background or border until hovered.
    Ghost,
    /// Confirms an action that deletes something, with visual weight (dialogs).
    Danger,
    /// Triggers an action that deletes something: gray until hovered, then soft red.
    Destructive,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    /// Compact actions, inside rows or headers.
    Small,
    #[default]
    Medium,
    /// Square with only an icon.
    Icon,
    /// Same as `Icon`, a bit smaller.
    IconSmall,
}

/// Colors and shape of a variant.
struct Look {
    bg: Option<Hsla>,
    fg: Hsla,
    hover_bg: Option<Hsla>,
    hover_fg: Hsla,
    border: bool,
    radius: Pixels,
}

impl ButtonVariant {
    fn look(self) -> Look {
        let c = colors();

        match self {
            Self::Primary => Look {
                bg: Some(c.primary),
                fg: c.primary_foreground,
                hover_bg: Some(c.primary_hover),
                hover_fg: c.primary_foreground,
                border: false,
                radius: px(6.),
            },
            Self::Secondary => Look {
                bg: Some(c.secondary),
                fg: c.secondary_foreground,
                hover_bg: Some(c.secondary_hover),
                hover_fg: c.secondary_foreground,
                border: true,
                radius: px(6.),
            },
            Self::Outline => Look {
                bg: None,
                fg: c.foreground,
                hover_bg: Some(c.secondary_hover),
                hover_fg: c.accent_foreground,
                border: true,
                radius: px(6.),
            },
            Self::Ghost => Look {
                bg: None,
                fg: c.muted_foreground,
                hover_bg: Some(c.secondary_hover),
                hover_fg: c.accent_foreground,
                border: false,
                radius: px(4.),
            },
            Self::Danger => Look {
                bg: Some(c.destructive),
                fg: c.destructive_foreground,
                hover_bg: Some(c.destructive_hover),
                hover_fg: c.destructive_foreground,
                border: false,
                radius: px(6.),
            },
            Self::Destructive => Look {
                bg: None,
                fg: c.muted_foreground,
                hover_bg: Some(c.destructive_subtle),
                hover_fg: c.destructive_foreground,
                border: false,
                radius: px(4.),
            },
        }
    }
}

impl ButtonSize {
    /// Height, fixed width (square ones only) and horizontal padding.
    fn metrics(self) -> (Pixels, Option<Pixels>, Pixels) {
        match self {
            Self::Small => (px(24.), None, px(8.)),
            Self::Medium => (px(28.), None, px(12.)),
            Self::Icon => (px(26.), Some(px(26.)), px(0.)),
            Self::IconSmall => (px(22.), Some(px(22.)), px(0.)),
        }
    }
}

/// App-styled button. `variant` changes the style and `size` the size. Accepts `Styled`
/// (whatever is set wins over the variant), its own children, and it can be used as a menu trigger.
#[derive(IntoElement)]
pub struct Button {
    base: BaseButton,
    style: StyleRefinement,
    variant: ButtonVariant,
    size: ButtonSize,
    label: Option<SharedString>,
    icon: Option<IconName>,
    tooltip: Option<SharedString>,
    disabled: bool,
    selected: bool,
    children: Vec<AnyElement>,
    on_click: Option<Handler>,
}

impl Button {
    pub fn new(id: impl Into<gpui_kit::ElementId>) -> Self {
        Self {
            base: BaseButton::new(id),
            style: StyleRefinement::default(),
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            label: None,
            icon: None,
            tooltip: None,
            disabled: false,
            selected: false,
            children: Vec::new(),
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    /// With `false` the button doesn't steal focus or enter the tab order: for those placed inside another
    /// control (the eye of a password field, the ✕ of a search box).
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.base = self.base.focusable(focusable).tab_stop(focusable);
        self
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// What a dropdown menu asks of its trigger: that it knows whether its menu is open.
impl Selectable for Button {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let c = colors();
        let look = self.variant.look();
        let (h, w, pad) = self.size.metrics();
        let (disabled, hover_fg, hover_bg) = (self.disabled, look.hover_fg, look.hover_bg);
        let on_click = self.on_click;

        let mut base = self
            .base
            .disabled(disabled)
            .gap(px(6.))
            .flex_shrink_0()
            .h(h)
            .when_some(w, |this, w| this.w(w))
            .when(w.is_none(), |this| this.px(pad))
            .rounded(look.radius)
            .text_color(look.fg)
            .whitespace_nowrap()
            .when_some(look.bg, |this, bg| this.bg(bg))
            .when(look.border, |this| this.border_1().border_color(c.border))
            // An open menu leaves the button looking as if the mouse were over it.
            .styles(|styles| {
                styles
                    .selected(|s| s.text_color(hover_fg).when_some(hover_bg, |s, bg| s.bg(bg)))
                    .disabled(|s| s.opacity(0.4))
            })
            .when(!disabled, |this| {
                this.cursor_pointer().hover(move |s| {
                    let s = s.text_color(hover_fg);

                    match hover_bg {
                        Some(bg) => s.bg(bg),
                        None => s,
                    }
                })
            })
            .when_some(self.icon, |this, name| this.child(glyph(name, 13.)))
            .when_some(self.label, |this, label| this.child(label))
            .children(self.children)
            .when_some(self.tooltip, |this, tip| {
                this.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
            })
            .when_some(on_click.filter(|_| !disabled), |this, f| {
                this.on_click(move |e, window, cx| {
                    cx.stop_propagation();
                    f(e, window, cx)
                })
            })
            .selected(self.selected);

        // Whatever is set with `Styled` goes last, so it wins over the variant.
        base.style().refine(&self.style);

        base
    }
}
