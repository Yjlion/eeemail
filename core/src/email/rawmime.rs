//! Retention of original MIME bytes.
//!
//! Upstream core does not keep raw MIME: it parses a message, stores the
//! decrypted result, and discards the original. That is the right trade for a
//! chat app and the wrong one for an email client, which needs the original
//! bytes for "view source", signature re-verification, faithful forwarding and
//! standards-conformant export.
//!
//! Keeping *both* the decrypted content and the raw bytes forever would roughly
//! double disk usage for a benefit that is mostly needed while a message is in
//! flight and during the window in which it may be replied to. So decrypted
//! content is canonical and permanent, and raw MIME is retained for a
//! configurable period -- short by default, up to indefinite.
//!
//! See `docs/adr/0004-local-store-and-raw-mime.md`.
//!
//! # Storage
//!
//! Bytes live in the blobdir via [`BlobObject::create_and_deduplicate_from_bytes`],
//! which is content-addressed, so two accounts receiving the same message store
//! one copy. The `raw_mime` table maps a message to its blob and records when
//! the blob may be reclaimed.
//!
//! Blobs are reclaimed by core's existing housekeeping: [`expire`] drops rows
//! whose `expires_at` has passed, and `sql::remove_unused_files` then collects
//! any blob no longer referenced. That ordering matters -- expiry must run
//! before reference collection in the same pass, or reclamation is deferred to
//! the following one.

use anyhow::{Context as _, Result};

use crate::blob::BlobObject;
use crate::config::Config;
use crate::context::Context;
use crate::message::MsgId;
use crate::tools::time;

/// How long raw MIME is kept, from [`Config::RawMimeRetentionDays`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Do not retain raw MIME at all (`0`).
    ///
    /// Chosen deliberately by a user who wants the smallest possible on-disk
    /// footprint, and who accepts losing view-source and signature
    /// re-verification.
    Disabled,
    /// Keep for this many days (`> 0`).
    Days(u32),
    /// Keep indefinitely (any negative value; `-1` is canonical).
    Forever,
}

impl Retention {
    /// Reads the configured retention.
    pub async fn load(context: &Context) -> Result<Self> {
        Ok(Self::from_days(
            context.get_config_int(Config::RawMimeRetentionDays).await?,
        ))
    }

    fn from_days(days: i32) -> Self {
        match days {
            0 => Retention::Disabled,
            d if d < 0 => Retention::Forever,
            d => Retention::Days(d.unsigned_abs()),
        }
    }

    /// Absolute expiry timestamp for something stored at `now`.
    ///
    /// `None` means "never expires", which is stored as SQL NULL.
    fn expires_at(self, now: i64) -> Option<i64> {
        match self {
            // Callers must not store when disabled; treat as immediately
            // expired rather than silently retaining forever.
            Retention::Disabled => Some(now),
            Retention::Forever => None,
            Retention::Days(d) => Some(now.saturating_add(i64::from(d).saturating_mul(86_400))),
        }
    }
}

/// Stores the raw bytes of a message, honouring the configured retention.
///
/// A no-op when retention is disabled. Errors are the caller's to log: raw MIME
/// is an enhancement, and failing to keep it must never fail message reception
/// or sending.
pub async fn store(context: &Context, msg_id: MsgId, raw: &[u8]) -> Result<()> {
    let retention = Retention::load(context).await?;
    if retention == Retention::Disabled {
        return Ok(());
    }
    store_with_retention(context, msg_id, raw, retention).await
}

async fn store_with_retention(
    context: &Context,
    msg_id: MsgId,
    raw: &[u8],
    retention: Retention,
) -> Result<()> {
    let now = time();
    let blob = BlobObject::create_and_deduplicate_from_bytes(context, raw, "raw.eml")
        .context("failed to store raw MIME blob")?;

    context
        .sql
        .execute(
            "INSERT INTO raw_mime (msg_id, blobname, size, stored_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(msg_id) DO UPDATE SET
                blobname=excluded.blobname,
                size=excluded.size,
                stored_at=excluded.stored_at,
                expires_at=excluded.expires_at",
            (
                msg_id,
                blob.as_name(),
                i64::try_from(raw.len()).unwrap_or(i64::MAX),
                now,
                retention.expires_at(now),
            ),
        )
        .await
        .context("failed to record raw MIME")?;
    Ok(())
}

/// Returns the original bytes of a message, if still retained.
///
/// `None` means the message never had raw MIME stored, or it has since
/// expired. Callers must degrade gracefully rather than treating this as an
/// error: for an old message it is the expected outcome.
pub async fn load(context: &Context, msg_id: MsgId) -> Result<Option<Vec<u8>>> {
    let Some(blobname) = context
        .sql
        .query_get_value::<String>("SELECT blobname FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await?
    else {
        return Ok(None);
    };

    match tokio::fs::read(&blob_path(context, &blobname)).await {
        Ok(bytes) => Ok(Some(bytes)),
        // The row outlived its blob. Housekeeping can reclaim a blob whose row
        // is being deleted concurrently, so treat this as "not retained"
        // rather than an error, and drop the dangling row.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            context
                .sql
                .execute("DELETE FROM raw_mime WHERE msg_id=?", (msg_id,))
                .await?;
            Ok(None)
        }
        Err(err) => Err(err).context("failed to read raw MIME blob"),
    }
}

/// Resolves a stored blob name to an absolute path.
///
/// Names are stored `$BLOBDIR/`-prefixed, matching the convention upstream
/// uses for `http_cache`. That prefix is what lets housekeeping's
/// `maybe_add_file` recognise the reference, so it must not be stripped before
/// storing -- only when reading back.
fn blob_path(context: &Context, blobname: &str) -> std::path::PathBuf {
    context
        .get_blobdir()
        .join(blobname.strip_prefix("$BLOBDIR/").unwrap_or(blobname))
}

/// Whether raw MIME is currently retained for a message.
pub async fn is_retained(context: &Context, msg_id: MsgId) -> Result<bool> {
    context
        .sql
        .exists("SELECT COUNT(*) FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await
}

/// Drops rows whose retention has elapsed. Returns how many were dropped.
///
/// Only the table rows go here. The blobs themselves are reclaimed by
/// `sql::remove_unused_files`, which runs later in the same housekeeping pass
/// and collects anything no longer referenced.
pub async fn expire(context: &Context) -> Result<usize> {
    let now = time();
    let count = context
        .sql
        .execute(
            "DELETE FROM raw_mime WHERE expires_at IS NOT NULL AND expires_at <= ?",
            (now,),
        )
        .await
        .context("failed to expire raw MIME")?;
    if count > 0 {
        info!(context, "Expired raw MIME for {count} message(s).");
    }
    Ok(count)
}

/// Forgets the raw MIME of a message immediately.
///
/// Used when a message is deleted, so the original does not outlive the
/// message it belongs to.
pub async fn delete(context: &Context, msg_id: MsgId) -> Result<()> {
    context
        .sql
        .execute("DELETE FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await?;
    Ok(())
}

#[cfg(test)]
mod rawmime_tests;
