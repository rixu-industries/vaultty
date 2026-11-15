// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Custom menus with Zed's look (compact rows, thin separator, check mark on the right): context
//! (right click) and dropdown. They rely on `base` to open, close and position; the gpui-kit menu
//! comes with a fixed style and can't be tweaked from outside.

use crate::app::theme::colors;
use crate::app::theme::px;
use crate::components::icon;
use gpui_kit::assets::IconName;
use gpui_kit::base::{POPUP_PRIORITY, Popover, Positioner, Selectable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Point,
    Render, RenderOnce, SharedString, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    deferred, div,
};
use std::rc::Rc;

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type Row = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type Close = Rc<dyn Fn(&mut Window, &mut App)>;
type Build = Rc<dyn Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu>;

enum Kind {
    Text(SharedString),
    Element(Row),
    Separator,
}

/// A menu row: text with an optional icon, or custom content.
pub struct PopupMenuItem {
    kind: Kind,
    icon: Option<IconName>,
    checked: bool,
    disabled: bool,
    on_click: Option<Click>,
}

impl PopupMenuItem {
    fn with(kind: Kind) -> Self {
        Self {
            kind,
            icon: None,
            checked: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn new(label: impl Into<SharedString>) -> Self {
        Self::with(Kind::Text(label.into()))
    }

    /// Row with custom content; the menu sets the background, the check mark and the click.
    pub fn element<E: IntoElement>(render: impl Fn(&mut Window, &mut App) -> E + 'static) -> Self {
        Self::with(Kind::Element(Rc::new(move |window, cx| {
            render(window, cx).into_any_element()
        })))
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
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

    fn selectable(&self) -> bool {
        !self.disabled && !matches!(self.kind, Kind::Separator)
    }
}

/// What gets added in the closure of `dropdown_menu` and `context_menu`.
pub struct PopupMenu {
    items: Vec<PopupMenuItem>,
    min_w: Pixels,
    max_h: Option<Pixels>,
    scrollable: bool,
}

impl Default for PopupMenu {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            min_w: px(128.),
            max_h: None,
            scrollable: false,
        }
    }
}

impl PopupMenu {
    pub fn min_w(mut self, width: impl Into<Pixels>) -> Self {
        self.min_w = width.into();
        self
    }

    /// Maximum height; only applies if the menu is `scrollable`.
    pub fn max_h(mut self, height: impl Into<Pixels>) -> Self {
        self.max_h = Some(height.into());
        self
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    pub fn item(mut self, item: PopupMenuItem) -> Self {
        self.items.push(item);
        self
    }

    /// A separator at the start or repeated is not painted.
    pub fn separator(mut self) -> Self {
        let redundant = self
            .items
            .last()
            .is_none_or(|i| matches!(i.kind, Kind::Separator));

        if !redundant {
            self.items.push(PopupMenuItem::with(Kind::Separator));
        }

        self
    }
}

/// The open menu: paints the rows and tracks the selected row and the keyboard.
struct MenuView {
    menu: PopupMenu,
    selected: Option<usize>,
    focus: FocusHandle,
    close: Close,
}

impl MenuView {
    fn open(menu: PopupMenu, close: Close, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let view = cx.new(|cx| Self {
            menu,
            selected: None,
            focus: cx.focus_handle(),
            close,
        });

        // With focus inside, the keyboard goes to the menu.
        view.read(cx).focus.clone().focus(window, cx);
        view
    }

    /// Moves the mark to the next selectable row, wrapping around at the end.
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.menu.items.len() as isize;
        let mut ix = self
            .selected
            .map_or(if delta > 0 { -1 } else { n }, |i| i as isize);

        for _ in 0..n {
            ix = (ix + delta).rem_euclid(n);

            if self.menu.items[ix as usize].selectable() {
                self.selected = Some(ix as usize);
                break;
            }
        }

        cx.notify();
    }

    fn activate(
        &mut self,
        ix: usize,
        ev: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = &self.menu.items[ix];

        if !item.selectable() {
            return;
        }

        let on_click = item.on_click.clone();

        // Close first: if the action opens a dialog, the focus that closing hands back won't take it from the dialog.
        (self.close.clone())(window, cx);

        if let Some(on_click) = on_click {
            on_click(ev, window, cx);
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "down" => self.step(1, cx),
            "up" => self.step(-1, cx),
            "enter" => {
                if let Some(ix) = self.selected {
                    self.activate(ix, &ClickEvent::default(), window, cx);
                }
            }
            "escape" => (self.close.clone())(window, cx),
            _ => return,
        }

        cx.stop_propagation();
    }
}

impl Render for MenuView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors();
        let has_icon = self.menu.items.iter().any(|i| i.icon.is_some());
        let on_key = cx.listener(Self::on_key);

        let rows =
            self.menu.items.iter().enumerate().map(|(ix, item)| {
                let content = match &item.kind {
                    Kind::Separator => {
                        return div().h(px(1.)).my(px(4.)).bg(c.border).into_any_element();
                    }
                    Kind::Text(label) => div().truncate().child(label.clone()).into_any_element(),
                    Kind::Element(render) => render(window, cx),
                };

                let mut row =
                    div()
                        .id(("menu-item", ix))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(24.))
                        .mx(px(4.))
                        .px(px(8.))
                        .rounded(px(4.))
                        .text_color(if item.disabled {
                            c.muted_foreground
                        } else {
                            c.popover_foreground
                        })
                        .when(self.selected == Some(ix) && !item.disabled, |this| {
                            this.bg(c.accent).text_color(c.accent_foreground)
                        })
                        .when(has_icon, |this| {
                            // Rows without an icon leave a gap so the text stays aligned.
                            this.child(div().size(px(14.)).flex_none().children(
                                item.icon.map(|name| icon(name, 14., c.muted_foreground)),
                            ))
                        })
                        .child(div().flex_1().min_w_0().child(content))
                        .when(item.checked, |this| {
                            this.child(icon(IconName::Check, 12., c.foreground))
                        });

                if item.selectable() {
                    row = row
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.selected != Some(ix) {
                                this.selected = Some(ix);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                            this.activate(ix, ev, window, cx)
                        }));
                }

                row.into_any_element()
            });

        div()
            .id("popup-menu")
            .track_focus(&self.focus)
            .on_key_down(on_key)
            .flex()
            .flex_col()
            .py(px(4.))
            .min_w(self.menu.min_w)
            .bg(c.popover)
            .border_1()
            .border_color(c.border)
            .rounded(px(6.))
            .shadow_md()
            .text_size(px(13.))
            .text_color(c.popover_foreground)
            // shortcut: the keyboard doesn't scroll the list; fine for short lists, change it if there are long menus.
            .when(self.menu.scrollable, |this| {
                this.when_some(self.menu.max_h, |this, h| this.max_h(h))
                    .overflow_y_scroll()
            })
            .children(rows)
    }
}

/// Right click on an element: opens a menu at the pointer.
pub trait ContextMenuExt:
    InteractiveElement + ParentElement + Styled + IntoElement + Sized
{
    #[track_caller]
    fn context_menu(
        mut self,
        f: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> ContextMenu<Self> {
        // The id must be stable across renders, or the open menu is lost on each one.
        let id = match self.interactivity().element_id.clone() {
            Some(id) => ElementId::Name(format!("context-menu-{id:?}").into()),
            None => ElementId::CodeLocation(*std::panic::Location::caller()),
        };

        ContextMenu {
            id,
            element: self,
            build: Rc::new(f),
        }
    }
}

impl<E: InteractiveElement + ParentElement + Styled + IntoElement> ContextMenuExt for E {}

#[derive(IntoElement)]
pub struct ContextMenu<E: InteractiveElement + ParentElement + Styled + IntoElement + 'static> {
    id: ElementId,
    element: E,
    build: Build,
}

impl<E: InteractiveElement + ParentElement + Styled + IntoElement + 'static> Styled
    for ContextMenu<E>
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E: InteractiveElement + ParentElement + Styled + IntoElement + 'static> ParentElement
    for ContextMenu<E>
{
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements)
    }
}

#[derive(Default)]
struct ContextState {
    at: Option<Point<Pixels>>,
    menu: Option<Entity<MenuView>>,
}

impl<E: InteractiveElement + ParentElement + Styled + IntoElement + 'static> RenderOnce
    for ContextMenu<E>
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id, cx, |_, _| ContextState::default());

        let element = self.element.on_mouse_down(MouseButton::Right, {
            let state = state.clone();

            move |ev, window, cx| {
                // The inner menu wins over a container's.
                cx.stop_propagation();
                state.update(cx, |s, _| {
                    s.at = Some(ev.position);
                    s.menu = None;
                });
                window.refresh();
            }
        });

        let Some(at) = state.read(cx).at else {
            return element.into_any_element();
        };

        let menu = match state.read(cx).menu.clone() {
            Some(menu) => menu,
            None => {
                let built = (self.build)(PopupMenu::default(), window, cx);

                if built.items.is_empty() {
                    return element.into_any_element();
                }

                let previous = window.focused(cx);
                let weak = state.downgrade();
                let close: Close = Rc::new(move |window, cx| {
                    _ = weak.update(cx, |s, _| {
                        s.at = None;
                        s.menu = None;
                    });

                    // The terminal and others get back the focus they had before the menu opened.
                    if let Some(previous) = &previous {
                        previous.focus(window, cx);
                    }

                    window.refresh();
                });

                let menu = MenuView::open(built, close, window, cx);
                state.update(cx, |s, _| s.menu = Some(menu.clone()));
                menu
            }
        };

        let close = menu.read(cx).close.clone();

        element
            .child(
                deferred(
                    Positioner::corner(Anchor::TopLeft, at)
                        .margin(px(8.))
                        .occlude()
                        .child(
                            div()
                                .on_mouse_down_out(move |_, window, cx| close(window, cx))
                                .child(menu),
                        ),
                )
                .with_priority(POPUP_PRIORITY),
            )
            .into_any_element()
    }
}

/// Dropdown: the element opens the menu below it on click.
pub trait DropdownMenu: Selectable + InteractiveElement + IntoElement + Sized + 'static {
    fn dropdown_menu(
        self,
        f: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        self.dropdown_menu_with_anchor(Anchor::TopLeft, f)
    }

    fn dropdown_menu_with_anchor(
        mut self,
        anchor: impl Into<Anchor>,
        f: impl Fn(PopupMenu, &mut Window, &mut App) -> PopupMenu + 'static,
    ) -> DropdownMenuPopover<Self> {
        DropdownMenuPopover {
            id: self.interactivity().element_id.clone().unwrap_or(0.into()),
            anchor: anchor.into(),
            trigger: self,
            build: Rc::new(f),
        }
    }
}

/// Our button opens dropdowns like any other element.
impl DropdownMenu for crate::components::Button {}

#[derive(IntoElement)]
pub struct DropdownMenuPopover<T: Selectable + IntoElement + 'static> {
    id: ElementId,
    anchor: Anchor,
    trigger: T,
    build: Build,
}

impl<T: Selectable + IntoElement + 'static> RenderOnce for DropdownMenuPopover<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // The menu is kept while it is open: it is rebuilt on each opening so the rows
        // reflect the state at that moment.
        let slot =
            window.use_keyed_state(
                (self.id.clone(), "menu"),
                cx,
                |_, _| None::<Entity<MenuView>>,
            );
        let weak = slot.downgrade();
        let build = self.build;

        Popover::new(self.id)
            .trigger(self.trigger)
            .anchor(self.anchor)
            .on_open_change(move |open, _, cx| {
                if !*open {
                    _ = weak.update(cx, |menu, _| *menu = None);
                }
            })
            .content(move |_, window, cx| {
                if let Some(menu) = slot.read(cx).clone() {
                    return menu;
                }

                let popover = cx.entity().downgrade();
                let close: Close = Rc::new(move |window, cx| {
                    _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                });
                let menu =
                    MenuView::open(build(PopupMenu::default(), window, cx), close, window, cx);

                slot.update(cx, |s, _| *s = Some(menu.clone()));
                menu
            })
    }
}
