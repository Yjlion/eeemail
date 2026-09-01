//! Encrypted backup.
//!
//! The mail server is a transport spool, not storage ([ADR 0003]), so the local
//! database is the only durable copy of the mailbox. That makes backup a
//! correctness feature rather than a nicety: a lost device with no backup is a
//! lost mailbox, and unlike a conventional IMAP client there is nothing on the
//! server to re-download.
//!
//! Core already does the hard part. [`ImexMode::ExportBackup`] writes a tar
//! containing the database, the blobdir and the keys, encrypted with a
//! passphrase, and [`ImexMode::ImportBackup`] restores it. This module adds
//! what an email client needs on top: knowing **when the last backup was
//! taken**, and saying so when it is stale.
//!
//! # Why staleness is tracked and not scheduled
//!
//! An automatic backup needs somewhere to put it, and every such place is a
//! decision with consequences the user has to make: a cloud provider sees the
//! ciphertext, its size and its timing, and learns when you use your mail. So
//! this module records and reports; choosing a destination and a cadence stays
//! with the user.
//!
//! Uploading to a specific cloud provider is deliberately **not** implemented.
//! It needs credentials, a provider API and a threat model per provider, none
//! of which can be built or tested honestly without picking one. Exporting to a
//! path is the part that is provider-independent, and it is what a
//! synchronising folder or an external disk needs anyway.
//!
//! [ADR 0003]: ../../../docs/adr/0003-imap-as-transport.md

use std::path::Path;

use anyhow::{Result, ensure};

use crate::context::Context;
use crate::imex::{ImexMode, imex};
use crate::tools::time;

/// Raw config key recording when a backup last completed.
const LAST_BACKUP_KEY: &str = "eeemail_last_backup";

/// How long a backup is considered current.
///
/// Seven days: long enough not to nag, short enough that a lost device costs at
/// most a week of mail rather than everything since the user last thought about
/// it.
const STALE_AFTER: i64 = 7 * 86_400;

/// When the last backup was taken and whether that is recent enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupStatus {
    /// Unix timestamp of the last successful backup, or `None` if there has
    /// never been one.
    pub last_backup: Option<i64>,

    /// True when there has never been a backup, or the last one is older than a
    /// week.
    pub stale: bool,
}

/// Reads the backup status.
pub async fn status(context: &Context) -> Result<BackupStatus> {
    let last_backup: Option<i64> = context.sql.get_raw_config_int64(LAST_BACKUP_KEY).await?;
    let stale = match last_backup {
        // Never backed up is the staler state, not a neutral one: it is the
        // case where a lost device costs the whole mailbox.
        None => true,
        Some(at) => time().saturating_sub(at) > STALE_AFTER,
    };
    Ok(BackupStatus { last_backup, stale })
}

/// Writes an encrypted backup into `dir` and records that it happened.
///
/// The passphrase is required. Core permits an empty one, which would write the
/// entire mailbox to a file in the clear; for a client whose local store *is*
/// the mailbox that is not a default anyone should be able to reach by leaving
/// a field blank.
pub async fn export(context: &Context, dir: &Path, passphrase: &str) -> Result<()> {
    ensure!(
        !passphrase.is_empty(),
        "a backup passphrase is required: an unencrypted backup is a copy of \
         the entire mailbox in the clear"
    );
    imex(
        context,
        ImexMode::ExportBackup,
        dir,
        Some(passphrase.to_string()),
    )
    .await?;
    context
        .sql
        .set_raw_config(LAST_BACKUP_KEY, Some(&time().to_string()))
        .await?;
    Ok(())
}

/// Restores from an encrypted backup file.
///
/// Replaces the current account contents. The caller is responsible for
/// confirming that with the user first.
pub async fn import(context: &Context, file: &Path, passphrase: &str) -> Result<()> {
    imex(
        context,
        ImexMode::ImportBackup,
        file,
        Some(passphrase.to_string()),
    )
    .await
}

#[cfg(test)]
mod backup_tests;
