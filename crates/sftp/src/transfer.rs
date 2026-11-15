// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Transfers between the two panes. `plan` says what must be created and copied (and what already
//! exists), and the tab's queue copies item by item with `copy`, one at a time.

use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::AbortHandle;

use crate::files::{EntryKind, Fs, Reader, Side, Writer};

const CHUNK: usize = 256 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Error for a copy stopped on purpose (cancelled or when closing the tab).
pub const CANCELLED: &str = "Cancelled";

/// Something to create (`dir`) or copy to the destination.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub src: String,
    pub dst: String,
    /// Folder to create at the destination; it comes before its contents.
    pub dir: bool,
    pub size: u64,
    /// The destination file already exists (existing folders are reused).
    pub exists: bool,
    /// Reason it is not copied.
    pub skipped: Option<String>,
}

/// A name that comes from the other side is only a name: no separators, `..` or drive. A malicious
/// server could send `../../.zshrc` to write outside the chosen folder.
fn safe_name(name: &str) -> bool {
    let mut parts = std::path::Path::new(name).components();

    !name.contains(['/', '\\'])
        && matches!(parts.next(), Some(std::path::Component::Normal(_)))
        && parts.next().is_none()
}

/// What must be done to copy `paths` from `from` into `to_dir` on `to`.
pub async fn plan(from: &Fs, to: &Fs, paths: &[String], to_dir: &str) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    let existing = listing(to, to_dir, true).await?;

    for path in paths {
        let entry = from
            .stat(path)
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.clone()))?;

        let dst = to.side().join(to_dir, &entry.name);
        Box::pin(walk(from, to, entry, dst, &existing, &mut items)).await?;
    }

    Ok(items)
}

/// What a destination folder already holds: each name and whether it is a folder. `None` on the
/// local disk, which is asked file by file: that is cheap there, and it knows its own rules about
/// upper and lower case.
type Listing = Option<HashMap<String, bool>>;

/// Lists `dir` on a server once, instead of one round trip per file to copy into it. `is_dir`
/// false: it is not a folder there (missing, or a file), so there is nothing to list.
async fn listing(to: &Fs, dir: &str, is_dir: bool) -> io::Result<Listing> {
    if to.side() == Side::Local {
        return Ok(None);
    }

    if !is_dir {
        return Ok(Some(HashMap::new()));
    }

    match to.list(dir).await {
        Ok(entries) => Ok(Some(
            entries
                .into_iter()
                .map(|e| (e.name.clone(), e.is_dir()))
                .collect(),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Some(HashMap::new())),
        Err(e) => Err(e),
    }
}

/// `existing` is the listing of the folder `dst` goes into.
async fn walk(
    from: &Fs,
    to: &Fs,
    entry: crate::files::Entry,
    dst: String,
    existing: &Listing,
    items: &mut Vec<Item>,
) -> io::Result<()> {
    let mut item = Item {
        src: entry.path.clone(),
        dst,
        dir: false,
        size: entry.size,
        exists: false,
        skipped: None,
    };

    if !safe_name(&entry.name) {
        item.skipped = Some("Invalid name".into());
        items.push(item);
        return Ok(());
    }

    match entry.kind {
        // Copying what is inside could never finish (a link to a parent folder).
        EntryKind::LinkDir => {
            item.skipped = Some("Folder link skipped".into());
            items.push(item);
        }
        EntryKind::Dir => {
            item.dir = true;
            let dir = item.dst.clone();
            items.push(item);

            let is_dir = existing
                .as_ref()
                .is_some_and(|names| names.get(&entry.name) == Some(&true));
            let inside = listing(to, &dir, is_dir).await?;

            let mut children = from.list(&entry.path).await?;
            children.sort_by(|a, b| a.name.cmp(&b.name));

            for child in children {
                let dst = to.side().join(&dir, &child.name);
                Box::pin(walk(from, to, child, dst, &inside, items)).await?;
            }
        }
        EntryKind::File | EntryKind::Link => {
            item.exists = match existing {
                Some(names) => names.contains_key(&entry.name),
                None => to.stat(&item.dst).await?.is_some(),
            };
            items.push(item);
        }
    }

    Ok(())
}

/// Copies `reader` into `writer` in chunks. Calls `progress` at most every 100 ms and at the end.
async fn pump(
    mut reader: Reader,
    mut writer: Writer,
    mut progress: impl FnMut(u64),
) -> io::Result<()> {
    let mut buf = vec![0; CHUNK];
    let mut done = 0;
    let mut last = Instant::now();

    loop {
        let n = reader.read(&mut buf).await?;

        if n == 0 {
            break;
        }

        writer.write_all(&buf[..n]).await?;
        done += n as u64;

        if last.elapsed() >= PROGRESS_EVERY {
            progress(done);
            last = Instant::now();
        }
    }

    writer.flush().await?;
    writer.shutdown().await?;
    progress(done);
    Ok(())
}

/// Why a copy failed: a disk or network error at `path`, or it was cancelled.
#[derive(Debug)]
pub enum CopyError {
    Io(io::Error, String),
    Cancelled,
}

/// Creates the folder of `item` or copies its file from `src` to `dst`. Without `overwrite`, fails
/// if the destination already exists. The copy runs in its own task: `started` receives the handle
/// to stop it. If it is stopped or fails midway, it deletes the partially written file.
pub async fn copy(
    src: &Fs,
    dst: &Fs,
    item: &Item,
    overwrite: bool,
    progress: impl FnMut(u64) + Send + 'static,
    started: impl FnOnce(AbortHandle),
) -> Result<(), CopyError> {
    let at = |path: &str| {
        let path = path.to_string();
        move |e| CopyError::Io(e, path)
    };

    if item.dir {
        return match dst.mkdir(&item.dst).await {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match dst.stat(&item.dst).await {
                Ok(Some(entry)) if entry.kind == EntryKind::Dir => Ok(()),
                _ => Err(CopyError::Io(e, item.dst.clone())),
            },
            result => result.map_err(at(&item.dst)),
        };
    }

    let reader = src.reader(&item.src).await.map_err(at(&item.src))?;
    // Without permission to overwrite, fails if the file appeared after planning (nothing
    // is deleted). From here on the destination is already created or emptied: if something
    // fails, it is deleted.
    let writer = dst
        .writer(&item.dst, overwrite)
        .await
        .map_err(at(&item.dst))?;

    let task = tokio::spawn(pump(reader, writer, progress));
    started(task.abort_handle());

    let error = match task.await {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e)) => CopyError::Io(e, item.dst.clone()),
        Err(_) => CopyError::Cancelled,
    };

    // If the connection is gone, it cannot be deleted; there is nothing more to do.
    let _ = dst.remove(&item.dst).await;
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same as `copy` without a tab or cancellation.
    async fn copy(
        from: &Fs,
        to: &Fs,
        src: &str,
        dst: &str,
        progress: impl FnMut(u64),
    ) -> io::Result<()> {
        pump(
            from.reader(src).await?,
            to.writer(dst, false).await?,
            progress,
        )
        .await
    }

    fn s(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// `origin/p/{a.txt, q/b.txt}` and an empty destination.
    fn tree() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("origin"), d.path().join("d"));
        std::fs::create_dir_all(src.join("p/q")).unwrap();
        std::fs::create_dir(&dst).unwrap();
        std::fs::write(src.join("p/a.txt"), "aaa").unwrap();
        std::fs::write(src.join("p/q/b.txt"), "bb").unwrap();
        (d, src, dst)
    }

    #[test]
    fn names_that_leave_the_folder_are_invalid() {
        for bad in ["..", ".", "", "../x", "a/b", "a\\b", "/etc/passwd", "C:\\x"] {
            assert!(!safe_name(bad), "{bad:?} should be rejected");
        }

        for good in ["a.txt", ".env", "piñata 🦤", "a..b"] {
            assert!(safe_name(good), "{good:?} should be accepted");
        }

        // On Windows `C:x` is relative to drive C:, not a name.
        #[cfg(windows)]
        assert!(!safe_name("C:evil"));
    }

    #[tokio::test]
    async fn plan_recursive_in_order() {
        let (_d, src, dst) = tree();

        let items = plan(&Fs::Local, &Fs::Local, &[s(&src.join("p"))], &s(&dst))
            .await
            .unwrap();

        let got: Vec<_> = items
            .iter()
            .map(|i| (i.dst.clone(), i.dir, i.size))
            .collect();

        assert_eq!(
            got,
            vec![
                (s(&dst.join("p")), true, 0),
                (s(&dst.join("p").join("a.txt")), false, 3),
                (s(&dst.join("p").join("q")), true, 0),
                (s(&dst.join("p").join("q").join("b.txt")), false, 2),
            ]
        );
        assert_eq!(items[1].src, s(&src.join("p").join("a.txt")));
        assert!(items.iter().all(|i| !i.exists && i.skipped.is_none()));
    }

    #[tokio::test]
    async fn plan_marks_existing() {
        let (_d, src, dst) = tree();
        std::fs::create_dir(dst.join("p")).unwrap();
        std::fs::write(dst.join("p/a.txt"), "old").unwrap();

        let items = plan(&Fs::Local, &Fs::Local, &[s(&src.join("p"))], &s(&dst))
            .await
            .unwrap();

        let exists: Vec<_> = items
            .iter()
            .filter(|i| i.exists)
            .map(|i| i.dst.clone())
            .collect();
        assert_eq!(exists, vec![s(&dst.join("p").join("a.txt"))]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plan_skips_folder_link() {
        let (_d, src, dst) = tree();
        std::os::unix::fs::symlink(src.join("p/q"), src.join("p/l")).unwrap();
        std::os::unix::fs::symlink(src.join("p/a.txt"), src.join("p/fa")).unwrap();

        let items = plan(&Fs::Local, &Fs::Local, &[s(&src.join("p"))], &s(&dst))
            .await
            .unwrap();

        let find = |name: &str| {
            items
                .iter()
                .find(|i| i.dst == s(&dst.join("p").join(name)))
                .unwrap()
        };

        assert_eq!(find("l").skipped.as_deref(), Some("Folder link skipped"));
        let fa = find("fa");
        assert!(fa.skipped.is_none() && !fa.dir);
        assert_eq!(fa.size, 3);
    }

    #[tokio::test]
    async fn copy_large_file_with_progress() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("large"), d.path().join("copy"));
        let data: Vec<u8> = (0..1_048_583u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let mut seen = Vec::new();
        copy(&Fs::Local, &Fs::Local, &s(&src), &s(&dst), |b| seen.push(b))
            .await
            .unwrap();

        assert_eq!(std::fs::read(&dst).unwrap(), data);
        assert_eq!(seen.last(), Some(&1_048_583));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    }
}
