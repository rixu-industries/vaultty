// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! What an SFTP layer does without depending on tabs or UI: operate on files on the local disk
//! or on the server (`files`), plan and copy transfers (`transfer`) and keep their queue
//! (`queue`). Connections and tasks live in `sessions`.

pub mod files;
pub mod queue;
pub mod transfer;

pub use files::{Entry, EntryKind, Fs, Remote, Side, human};
pub use queue::{Answer, Ask, Choice, Job, JobState, QueueView};
pub use transfer::{CANCELLED, CopyError, Item};

use std::cmp::Ordering;

/// Column a listing is sorted by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Name,
    Size,
    Modified,
    Perms,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub column: Column,
    pub desc: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort {
            column: Column::Name,
            desc: false,
        }
    }
}

impl Entry {
    /// Folder or link to a folder: can be entered.
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, EntryKind::Dir | EntryKind::LinkDir)
    }
}

/// By name, case-insensitive, with numbers in natural order (`a2` before `a10`).
fn by_name(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());

    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();

                    while let Some(c) = it.peek().filter(|c| c.is_ascii_digit()) {
                        digits.push(*c);
                        it.next();
                    }

                    digits
                };

                let (x, y) = (number(&mut a), number(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));

                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());

                if order != Ordering::Equal {
                    return order;
                }

                a.next();
                b.next();
            }
        }
    }
}

/// Folders first; within each group, by the chosen column and, on ties, by name. Without
/// `show_hidden`, entries starting with a dot are removed.
pub fn sort_entries(entries: &[Entry], sort: Sort, show_hidden: bool) -> Vec<Entry> {
    let mut list: Vec<Entry> = entries
        .iter()
        .filter(|e| show_hidden || !e.name.starts_with('.'))
        .cloned()
        .collect();

    list.sort_by(|a, b| {
        let column = match sort.column {
            Column::Name => by_name(&a.name, &b.name),
            Column::Size => a.size.cmp(&b.size),
            Column::Modified => a.modified.unwrap_or(0).cmp(&b.modified.unwrap_or(0)),
            Column::Perms => a.perms.cmp(&b.perms),
        };
        let column = if sort.desc { column.reverse() } else { column };

        b.is_dir()
            .cmp(&a.is_dir())
            .then(column)
            .then_with(|| by_name(&a.name, &b.name))
    });

    list
}

/// Path of `name` inside `dir`, using the separator `dir` already uses (`\` in Windows paths).
pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') || dir.ends_with('\\') {
        return format!("{dir}{name}");
    }

    let windows = dir.len() >= 3 && dir.as_bytes()[1] == b':' && dir.as_bytes()[2] == b'\\';
    let sep = if windows { '\\' } else { '/' };

    format!("{dir}{sep}{name}")
}

/// Human-readable size: `512 B`, `1.5 KB`, `20 MB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut i = 0;

    while n >= 1024.0 && i < UNITS.len() - 1 {
        n /= 1024.0;
        i += 1;
    }

    match i {
        0 => format!("{bytes} B"),
        _ if n < 10.0 => format!("{n:.1} {}", UNITS[i]),
        _ => format!("{n:.0} {}", UNITS[i]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, size: u64, modified: Option<u64>) -> Entry {
        Entry {
            name: name.into(),
            path: format!("/x/{name}"),
            kind,
            size,
            modified,
            perms: String::new(),
        }
    }

    fn names(list: &[Entry]) -> Vec<&str> {
        list.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn folders_first_and_natural_order() {
        let list = [
            entry("b10.txt", EntryKind::File, 1, None),
            entry("B2.txt", EntryKind::File, 1, None),
            entry("zeta", EntryKind::Dir, 0, None),
            entry("alpha", EntryKind::LinkDir, 0, None),
            entry(".hidden", EntryKind::File, 1, None),
        ];

        let sorted = sort_entries(&list, Sort::default(), false);
        assert_eq!(names(&sorted), ["alpha", "zeta", "B2.txt", "b10.txt"]);

        let all = sort_entries(&list, Sort::default(), true);
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn by_column_and_descending_without_moving_folders() {
        let list = [
            entry("a", EntryKind::File, 30, Some(3)),
            entry("b", EntryKind::File, 10, Some(1)),
            entry("c", EntryKind::File, 20, Some(2)),
            entry("d", EntryKind::Dir, 0, Some(9)),
        ];

        let size = sort_entries(
            &list,
            Sort {
                column: Column::Size,
                desc: false,
            },
            false,
        );
        assert_eq!(names(&size), ["d", "b", "c", "a"]);

        let newest = sort_entries(
            &list,
            Sort {
                column: Column::Modified,
                desc: true,
            },
            false,
        );
        assert_eq!(names(&newest), ["d", "a", "c", "b"]);
    }

    #[test]
    fn join_uses_the_path_separator() {
        assert_eq!(join("/home/u", "a"), "/home/u/a");
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join(r"C:\Users", "a"), r"C:\Users\a");
        assert_eq!(join(r"C:\", "a"), r"C:\a");
    }

    #[test]
    fn readable_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(20 * 1024 * 1024), "20 MB");
    }
}
