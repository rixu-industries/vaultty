// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Vaults: the list, creating a new one, unlocking, their settings and the lost-vault notice.

pub mod create;
pub mod lost;
pub mod picker;
pub mod settings;
pub mod unlock;

const MIN_PASSWORD: usize = 8;
