// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! File operations for the two panels of an SFTP tab. They return readable errors and, if they
//! fail because the connection dropped, they notify the tab.

use std::io;
use std::sync::Arc;

use sftp::{Entry, Fs, Side, human};
use uuid::Uuid;

use crate::Sessions;
use crate::remote::{Open, SftpSlot, lost};

/// The remote connection of the tab, if the side is the remote one.
pub(crate) type Conn = Option<(Arc<SftpSlot>, Arc<Open>)>;

impl Sessions {
    /// The `side` of the tab. For the remote side it also returns its connection, for `failed`.
    pub(crate) fn fs(&self, tab: Uuid, side: Side) -> Result<(Fs, Conn), String> {
        match side {
            Side::Local => Ok((Fs::Local, None)),
            Side::Remote => {
                let slot = self.slot(tab).ok_or("Not connected")?;
                let open = slot.open.lock().unwrap().clone().ok_or("Not connected")?;

                Ok((Fs::Remote(open.remote.clone()), Some((slot, open))))
            }
        }
    }

    /// Lists `path`.
    pub async fn list_dir(
        self: Arc<Self>,
        tab: Uuid,
        side: Side,
        path: String,
    ) -> Result<Vec<Entry>, String> {
        let (fs, conn) = self.fs(tab, side)?;
        fs.list(&path).await.map_err(|e| failed(&conn, &e, &path))
    }

    /// Enters `dir`: lists it and, if possible, remembers it as the panel's folder. When reconnecting,
    /// the panel goes back to that folder.
    pub async fn open_dir(
        self: Arc<Self>,
        tab: Uuid,
        side: Side,
        path: String,
    ) -> Result<Vec<Entry>, String> {
        let (fs, conn) = self.fs(tab, side)?;
        let entries = fs.list(&path).await.map_err(|e| failed(&conn, &e, &path))?;

        self.set_folder(tab, side, path);
        Ok(entries)
    }

    /// Folder a panel starts in: the last one opened, or the home folder if there is none.
    pub async fn start_dir(self: Arc<Self>, tab: Uuid, side: Side) -> Result<String, String> {
        if let Some(folder) = self.folder(tab, side) {
            return Ok(folder);
        }

        self.home(tab, side).await
    }

    pub async fn home(self: Arc<Self>, tab: Uuid, side: Side) -> Result<String, String> {
        let (fs, conn) = self.fs(tab, side)?;
        fs.home().await.map_err(|e| failed(&conn, &e, "~"))
    }

    pub async fn mkdir(self: Arc<Self>, tab: Uuid, side: Side, path: String) -> Result<(), String> {
        let (fs, conn) = self.fs(tab, side)?;
        fs.mkdir(&path).await.map_err(|e| failed(&conn, &e, &path))
    }

    pub async fn rename(
        self: Arc<Self>,
        tab: Uuid,
        side: Side,
        from: String,
        to: String,
    ) -> Result<(), String> {
        let (fs, conn) = self.fs(tab, side)?;

        fs.rename(&from, &to).await.map_err(|e| {
            let path = if e.kind() == io::ErrorKind::AlreadyExists {
                &to
            } else {
                &from
            };
            failed(&conn, &e, path)
        })
    }

    /// Deletes files and folders along with all their contents.
    pub async fn remove(
        self: Arc<Self>,
        tab: Uuid,
        side: Side,
        paths: Vec<String>,
    ) -> Result<(), String> {
        let (fs, conn) = self.fs(tab, side)?;

        for path in &paths {
            fs.remove(path).await.map_err(|e| failed(&conn, &e, path))?;
        }

        Ok(())
    }
}

/// Converts the error for the interface and, if the connection dropped, reports it.
pub(crate) fn failed(conn: &Conn, e: &io::Error, path: &str) -> String {
    if let Some((slot, open)) = conn {
        lost(slot, open);
    }

    human(e, path)
}
