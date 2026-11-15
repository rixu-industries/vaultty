// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! File operations for the two panes of the SFTP view: the local disk and the server, behind
//! the same API. The UI receives listings and readable errors; the bytes never pass through it.

use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::{StreamExt, stream};
use ssh::{OpenFlags, SftpError, StatusCode};
use tokio::io::{AsyncRead, AsyncWrite};

/// An open SFTP connection. `ssh` keeps alive the session that `sftp` hangs from.
pub struct Remote {
    pub ssh: ssh::Session,
    pub sftp: ssh::SftpSession,
}

/// Pane: this machine's disk or the server.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    File,
    Dir,
    /// Link to a file (or broken).
    Link,
    /// Link to a folder.
    LinkDir,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Unix seconds.
    pub modified: Option<u64>,
    /// `rwxr-xr-x`, or empty if the system has no Unix permissions.
    pub perms: String,
}

impl Side {
    /// The other pane.
    pub fn other(self) -> Side {
        match self {
            Side::Local => Side::Remote,
            Side::Remote => Side::Local,
        }
    }

    /// Index of the pane (`0` local, `1` remote), for storing something per pane in an array.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Path of `name` inside `dir`. Remote paths always use `/`; local ones use the OS separator.
    pub fn join(self, dir: &str, name: &str) -> String {
        match self {
            Side::Remote if dir.ends_with('/') => format!("{dir}{name}"),
            Side::Remote => format!("{dir}/{name}"),
            Side::Local => std::path::Path::new(dir)
                .join(name)
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// Folder containing `path`, or `None` at the root. On Windows, above a drive it is `""`,
    /// the list of drives.
    pub fn parent(self, path: &str) -> Option<String> {
        match self {
            Side::Remote => {
                let path = path.trim_end_matches('/');

                match path.rfind('/') {
                    _ if path.is_empty() => None,
                    Some(0) => Some("/".into()),
                    Some(i) => Some(path[..i].into()),
                    None => None,
                }
            }
            Side::Local => {
                let parent = std::path::Path::new(path).parent();

                match parent {
                    Some(p) => Some(p.to_string_lossy().into_owned()),
                    None if cfg!(windows) && !path.is_empty() => Some(String::new()),
                    None => None,
                }
            }
        }
    }
}

/// Unix permissions as shown by `ls -l` (without the type).
pub fn perms(mode: u32) -> String {
    let chars = ['r', 'w', 'x'];

    (0..9)
        .map(|i| {
            if mode & (0o400 >> i) != 0 {
                chars[i % 3]
            } else {
                '-'
            }
        })
        .collect()
}

fn secs(time: io::Result<SystemTime>) -> Option<u64> {
    time.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn exists(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("Already exists: {path}"),
    )
}

/// Converts an SFTP error to `io::Error`, keeping the cases the UI can explain.
fn sftp_err(e: SftpError) -> io::Error {
    let kind = match &e {
        SftpError::Status(s) if s.status_code == StatusCode::NoSuchFile => io::ErrorKind::NotFound,
        SftpError::Status(s) if s.status_code == StatusCode::PermissionDenied => {
            io::ErrorKind::PermissionDenied
        }
        _ => io::ErrorKind::Other,
    };

    io::Error::new(kind, e.to_string())
}

/// Message for the UI. `path` is the path the error refers to.
pub fn human(e: &io::Error, path: &str) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => format!("Not found: {path}"),
        io::ErrorKind::PermissionDenied => format!("Permission denied: {path}"),
        io::ErrorKind::AlreadyExists => format!("Already exists: {path}"),
        _ => format!("{e} ({path})"),
    }
}

pub fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_owned()
}

pub type Reader = Box<dyn AsyncRead + Send + Unpin>;
pub type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// A side of the tab to operate on.
pub enum Fs {
    Local,
    Remote(Arc<Remote>),
}

impl Fs {
    pub fn side(&self) -> Side {
        match self {
            Fs::Local => Side::Local,
            Fs::Remote(_) => Side::Remote,
        }
    }

    pub async fn list(&self, dir: &str) -> io::Result<Vec<Entry>> {
        match self {
            Fs::Local if dir.is_empty() => Ok(drives()),
            Fs::Local => {
                let mut read = tokio::fs::read_dir(dir).await?;
                let mut entries = Vec::new();

                while let Some(item) = read.next_entry().await? {
                    let path = item.path().to_string_lossy().into_owned();

                    // A file that vanishes while listing does not break the listing.
                    if let Ok(entry) = local_entry(&path).await {
                        entries.push(entry);
                    }
                }

                Ok(entries)
            }
            Fs::Remote(r) => {
                let read = r.sftp.read_dir(dir).await.map_err(sftp_err)?;

                let items = read.filter_map(|item| {
                    let name = item.file_name();

                    if name == "." || name == ".." {
                        return None;
                    }

                    let path = Side::Remote.join(dir, &name);
                    Some(remote_entry(r, name, path, item.metadata()))
                });

                // Each link asks the server for a `stat`: in parallel, a folder with hundreds of
                // links waits one round trip instead of hundreds. The limit does not saturate
                // small servers.
                Ok(stream::iter(items).buffered(32).collect().await)
            }
        }
    }

    /// What is at `path` following links, or `None` if it does not exist.
    pub async fn stat(&self, path: &str) -> io::Result<Option<Entry>> {
        let found = match self {
            Fs::Local => match tokio::fs::metadata(path).await {
                Ok(_) => local_entry(path).await,
                Err(e) => Err(e),
            },
            Fs::Remote(r) => match r.sftp.metadata(path).await.map_err(sftp_err) {
                Ok(meta) => Ok(remote_entry(r, file_name(path), path.into(), meta).await),
                Err(e) => Err(e),
            },
        };

        match found {
            Ok(entry) => Ok(Some(entry)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn home(&self) -> io::Result<String> {
        match self {
            Fs::Local => dirs::home_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .ok_or_else(|| io::Error::other("Home folder not found")),
            Fs::Remote(r) => r.sftp.canonicalize(".").await.map_err(sftp_err),
        }
    }

    /// Creates the folder; fails if something already has that name.
    pub async fn mkdir(&self, path: &str) -> io::Result<()> {
        if self.lstat_exists(path).await? {
            return Err(exists(path));
        }

        match self {
            Fs::Local => tokio::fs::create_dir(path).await,
            Fs::Remote(r) => r.sftp.create_dir(path).await.map_err(sftp_err),
        }
    }

    /// Renames; fails if something already exists at `to` (SFTP does not guarantee not
    /// overwriting it).
    pub async fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        if self.lstat_exists(to).await? {
            return Err(exists(to));
        }

        match self {
            Fs::Local => tokio::fs::rename(from, to).await,
            Fs::Remote(r) => r.sftp.rename(from, to).await.map_err(sftp_err),
        }
    }

    /// Deletes a file or a folder with all its contents. Links are deleted, not followed.
    pub async fn remove(&self, path: &str) -> io::Result<()> {
        match self {
            Fs::Local => {
                if tokio::fs::symlink_metadata(path).await?.is_dir() {
                    tokio::fs::remove_dir_all(path).await
                } else {
                    tokio::fs::remove_file(path).await
                }
            }
            Fs::Remote(r) => Box::pin(remove_remote(r, path)).await,
        }
    }

    pub async fn reader(&self, path: &str) -> io::Result<Reader> {
        Ok(match self {
            Fs::Local => Box::new(tokio::fs::File::open(path).await?),
            Fs::Remote(r) => Box::new(r.sftp.open(path).await.map_err(sftp_err)?),
        })
    }

    /// Creates the file. If it already exists, empties it with `overwrite`; without it, fails
    /// with `AlreadyExists`.
    pub async fn writer(&self, path: &str, overwrite: bool) -> io::Result<Writer> {
        let opened: io::Result<Writer> = match self {
            Fs::Local => tokio::fs::OpenOptions::new()
                .write(true)
                .create(overwrite)
                .truncate(overwrite)
                .create_new(!overwrite)
                .open(path)
                .await
                .map(|f| Box::new(f) as Writer),
            Fs::Remote(r) => {
                let flags = if overwrite {
                    OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
                } else {
                    OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE
                };

                r.sftp
                    .open_with_flags(path, flags)
                    .await
                    .map(|f| Box::new(f) as Writer)
                    .map_err(sftp_err)
            }
        };

        match opened {
            // SFTPv3 has no "already exists" code: a failure with EXCLUDE is checked by hand.
            Err(e)
                if !overwrite
                    && e.kind() != io::ErrorKind::AlreadyExists
                    && self.lstat_exists(path).await? =>
            {
                Err(exists(path))
            }
            other => other,
        }
    }

    async fn lstat_exists(&self, path: &str) -> io::Result<bool> {
        let found = match self {
            Fs::Local => tokio::fs::symlink_metadata(path).await.map(|_| ()),
            Fs::Remote(r) => r
                .sftp
                .symlink_metadata(path)
                .await
                .map(|_| ())
                .map_err(sftp_err),
        };

        match found {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

async fn local_entry(path: &str) -> io::Result<Entry> {
    let meta = tokio::fs::symlink_metadata(path).await?;

    let (kind, meta) = if meta.is_symlink() {
        match tokio::fs::metadata(path).await {
            Ok(target) if target.is_dir() => (EntryKind::LinkDir, target),
            Ok(target) => (EntryKind::Link, target),
            Err(_) => (EntryKind::Link, meta),
        }
    } else if meta.is_dir() {
        (EntryKind::Dir, meta)
    } else {
        (EntryKind::File, meta)
    };

    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        perms(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode = String::new();

    Ok(Entry {
        name: file_name(path),
        path: path.into(),
        kind,
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: secs(meta.modified()),
        perms: mode,
    })
}

/// `meta` comes without following links (from `read_dir`); if it is a link, its target is checked.
async fn remote_entry(r: &Remote, name: String, path: String, meta: ssh::Metadata) -> Entry {
    let (kind, meta) = if meta.file_type().is_symlink() {
        match r.sftp.metadata(path.as_str()).await {
            Ok(target) if target.file_type().is_dir() => (EntryKind::LinkDir, target),
            Ok(target) => (EntryKind::Link, target),
            Err(_) => (EntryKind::Link, meta),
        }
    } else if meta.file_type().is_dir() {
        (EntryKind::Dir, meta)
    } else {
        (EntryKind::File, meta)
    };

    Entry {
        name,
        path,
        kind,
        size: if meta.file_type().is_dir() {
            0
        } else {
            meta.len()
        },
        modified: meta.mtime.map(u64::from),
        perms: meta.permissions.map(perms).unwrap_or_default(),
    }
}

async fn remove_remote(r: &Remote, path: &str) -> io::Result<()> {
    let meta = r.sftp.symlink_metadata(path).await.map_err(sftp_err)?;

    if !meta.file_type().is_dir() {
        return r.sftp.remove_file(path).await.map_err(sftp_err);
    }

    let children = r.sftp.read_dir(path).await.map_err(sftp_err)?;

    for child in children {
        let name = child.file_name();

        if name != "." && name != ".." {
            Box::pin(remove_remote(r, &Side::Remote.join(path, &name))).await?;
        }
    }

    r.sftp.remove_dir(path).await.map_err(sftp_err)
}

/// Windows drives (`C:\`…). On other systems there is nothing above `/`.
fn drives() -> Vec<Entry> {
    if !cfg!(windows) {
        return Vec::new();
    }

    (b'A'..=b'Z')
        .map(|l| format!("{}:\\", l as char))
        .filter(|d| std::path::Path::new(d).exists())
        .map(|d| Entry {
            name: d.clone(),
            path: d,
            kind: EntryKind::Dir,
            size: 0,
            modified: None,
            perms: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn path(dir: &Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    fn dir(d: &tempfile::TempDir) -> String {
        d.path().to_string_lossy().into_owned()
    }

    #[test]
    fn perms_format() {
        assert_eq!(perms(0o755), "rwxr-xr-x");
        assert_eq!(perms(0o640), "rw-r-----");
    }

    #[tokio::test]
    async fn local_lists_types() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "abc").unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("piñata 🦤.txt"), "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("sub"), d.path().join("link")).unwrap();

        let list = Fs::Local.list(&dir(&d)).await.unwrap();
        let find = |name: &str| list.iter().find(|e| e.name == name).unwrap();

        let a = find("a.txt");
        assert_eq!(a.kind, EntryKind::File);
        assert_eq!(a.size, 3);
        assert!(a.modified.is_some());
        assert_eq!(a.path, path(d.path(), "a.txt"));

        assert_eq!(find("sub").kind, EntryKind::Dir);
        assert_eq!(find("piñata 🦤.txt").kind, EntryKind::File);
        #[cfg(unix)]
        assert_eq!(find("link").kind, EntryKind::LinkDir);
    }

    #[tokio::test]
    async fn local_mkdir_and_rename_do_not_overwrite() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, sub) = (
            path(d.path(), "a"),
            path(d.path(), "b"),
            path(d.path(), "sub"),
        );
        std::fs::write(&a, "A").unwrap();
        std::fs::write(&b, "B").unwrap();
        std::fs::create_dir(&sub).unwrap();

        let err = Fs::Local.mkdir(&sub).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);

        let err = Fs::Local.rename(&a, &b).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "B");

        Fs::Local.rename(&a, &path(d.path(), "c")).await.unwrap();
        assert_eq!(std::fs::read_to_string(path(d.path(), "c")).unwrap(), "A");
    }

    #[tokio::test]
    async fn local_remove_recursive_without_following_links() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("x"), "").unwrap();

        let to_remove = d.path().join("to_remove");
        std::fs::create_dir_all(to_remove.join("q")).unwrap();
        std::fs::write(to_remove.join("q/b.txt"), "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, to_remove.join("link")).unwrap();

        Fs::Local
            .remove(&to_remove.to_string_lossy())
            .await
            .unwrap();

        assert!(!to_remove.exists());
        assert!(outside.join("x").exists());
    }

    #[tokio::test]
    async fn writer_without_overwrite_does_not_touch_existing() {
        let d = tempfile::tempdir().unwrap();
        let a = path(d.path(), "a");
        std::fs::write(&a, "original").unwrap();

        let err = Fs::Local.writer(&a, false).await.err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "original");

        assert!(Fs::Local.writer(&a, true).await.is_ok());
        assert!(
            Fs::Local
                .writer(&path(d.path(), "new"), false)
                .await
                .is_ok()
        );
    }

    #[test]
    fn join_and_parent_remote() {
        let r = Side::Remote;
        assert_eq!(r.join("/home/u", "a b"), "/home/u/a b");
        assert_eq!(r.join("/", "x"), "/x");
        assert_eq!(r.parent("/home"), Some("/".into()));
        assert_eq!(r.parent("/home/u/"), Some("/home".into()));
        assert_eq!(r.parent("/"), None);
    }

    #[cfg(unix)]
    #[test]
    fn join_and_parent_local() {
        let l = Side::Local;
        assert_eq!(l.join("/tmp", "a"), "/tmp/a");
        assert_eq!(l.parent("/tmp/a"), Some("/tmp".into()));
        assert_eq!(l.parent("/"), None);
    }
}
