// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! SFTP tab: local panel on the left, the server one on the right and the transfer queue
//! below. Here live the Shift/Ctrl selection, what is dragged between panels and the listing helpers.

pub mod conflict;
pub mod name_dialog;
pub mod view;

use crate::app::theme::px;
use gpui_kit::{Styled, div};
use sftp::{Column, Side};
use uuid::Uuid;

/// New selection after clicking `path`. `order` holds the paths as they are shown; `anchor`, the last one
/// clicked without Shift. Returns the new selection and anchor.
pub fn select(
    current: &[String],
    order: &[String],
    path: &str,
    meta: bool,
    shift: bool,
    anchor: Option<&str>,
) -> (Vec<String>, String) {
    let from = anchor.and_then(|a| order.iter().position(|p| p == a));

    if shift && let (Some(from), Some(to)) = (from, order.iter().position(|p| p == path)) {
        let (a, b) = (from.min(to), from.max(to));
        return (order[a..=b].to_vec(), anchor.unwrap_or(path).to_string());
    }

    if meta {
        let selected = if current.iter().any(|p| p == path) {
            current.iter().filter(|p| *p != path).cloned().collect()
        } else {
            let mut selected = current.to_vec();
            selected.push(path.to_string());
            selected
        };

        return (selected, path.to_string());
    }

    (vec![path.to_string()], path.to_string())
}

/// What is dragged between panels: the paths and the side they come from.
#[derive(Clone)]
pub struct DragEntries {
    pub tab: Uuid,
    pub side: Side,
    pub paths: Vec<String>,
}

/// Row with the four columns of the listing.
fn grid_row() -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .cursor_default()
}

fn cell(column: Column) -> gpui_kit::Div {
    let base = div().flex().items_center().min_w_0().overflow_hidden();

    // The name is what matters: it has a minimum width and, if the panel is narrow (small window or
    // large UI scale), the other columns shrink and are cut off with «…».
    match column {
        Column::Name => base.flex_1().min_w(px(120.)),
        Column::Size => base.w(px(72.)),
        Column::Modified => base.w(px(120.)),
        Column::Perms => base.w(px(84.)),
    }
}

/// Short date and time in the computer's time zone.
fn format_date(secs: Option<u64>) -> String {
    use chrono::TimeZone;

    secs.and_then(|s| chrono::Local.timestamp_opt(s as i64, 0).single())
        .map(|d| d.format("%d/%m/%y %H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn simple_click_with_ctrl_and_shift() {
        let order = paths(&["a", "b", "c", "d"]);

        let (sel, anchor) = select(&[], &order, "b", false, false, None);
        assert_eq!((sel.clone(), anchor.as_str()), (paths(&["b"]), "b"));

        // Ctrl adds and removes.
        let (sel, _) = select(&sel, &order, "d", true, false, Some("b"));
        assert_eq!(sel, paths(&["b", "d"]));
        let (sel, anchor) = select(&sel, &order, "b", true, false, Some("d"));
        assert_eq!((sel, anchor.as_str()), (paths(&["d"]), "b"));

        // Shift selects the range from the anchor, in both directions.
        let (sel, anchor) = select(&[], &order, "d", false, true, Some("b"));
        assert_eq!((sel, anchor.as_str()), (paths(&["b", "c", "d"]), "b"));
        let (sel, _) = select(&[], &order, "a", false, true, Some("c"));
        assert_eq!(sel, paths(&["a", "b", "c"]));

        // Shift without an anchor is a normal click.
        let (sel, _) = select(&[], &order, "c", false, true, None);
        assert_eq!(sel, paths(&["c"]));
    }
}
