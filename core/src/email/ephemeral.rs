//! Ephemeral expiry moves a message to Trash instead of destroying it.
//!
//! [ADR 0011] shipped ephemeral messages disabled for one reason: expiry is
//! irreversible. Core's [`crate::ephemeral::delete_expired_messages`] rewrites
//! the row into a tombstone and drops the content. For Delta Chat that costs a
//! chat message; for eeemail the local store is the only durable copy of the
//! mailbox ([ADR 0004]) and the server is a spool with nothing to re-download,
//! so a timer meant the user's mail deleted itself for good.
//!
//! That reasoning was about **irreversibility**, not about timers. Nothing in
//! the protocol requires immediate local destruction: the timer is an agreement
//! with the correspondent about the message's lifetime, and we honour it by
//! removing the message from view when it fires. What the local client does
//! with the bytes afterwards is the same question every mail client answers
//! with a trash folder.
//!
//! So expiry lands here: [`divert`] runs *before* core's sweep, tags the
//! message `Trash`, records a purge deadline [`PURGE_DAYS`] out, and clears
//! `ephemeral_timestamp` so core's own sweep leaves it alone. [`purge`] then
//! destroys it for real, through the same path a manual delete uses.
//!
//! Removal from the server and from the correspondent's client is unchanged and
//! immediate. Only the local copy is deferred. See [ADR 0019].
//!
//! # `delete_device_after` is not diverted
//!
//! Core's separate `delete_device_after` setting also expires messages, and
//! [`divert`] deliberately leaves it alone. That setting exists to reclaim
//! disk, and a user who asks for the disk back and gets a trash folder full of
//! the same bytes has been ignored.
//!
//! [ADR 0004]: ../../../docs/adr/0004-local-store-and-raw-mime.md
//! [ADR 0011]: ../../../docs/adr/0011-receipts-and-ephemeral.md
//! [ADR 0019]: ../../../docs/adr/0019-recoverable-ephemeral-expiry.md

use anyhow::Result;

use crate::config::Config;
use crate::constants::DC_CHAT_ID_TRASH;
use crate::context::Context;
use crate::ephemeral::Timer;
use crate::message::MsgId;
use crate::sync::Sync;
use crate::tools::time;

use super::labels::{self, TRASH};

/// How long a trashed message stays recoverable, for a fresh eeemail account.
///
/// Applied by [`super::policy::apply_defaults`] rather than being the
/// compile-time default of [`Config::TrashPurgeDays`], because upstream's
/// ephemeral tests assert that a fired timer removes the message. See
/// `docs/adr/0012-rpc-and-cli.md` for why that distinction is worth keeping.
pub const DEFAULT_PURGE_DAYS: i64 = 30;

/// How long a trashed message stays recoverable on this account.
///
/// `0` means destroy immediately, which is upstream's behaviour and what a user
/// who wants a fired timer to mean *gone* would choose.
///
/// This governs **everything** in `Trash`, not only expired mail: a message the
/// user threw away, one whose ephemeral timer fired, and one swept out of the
/// unverified view all arrive here and leave on the same deadline. `Trash` is
/// the only place in eeemail that destroys mail, so it is the only place with a
/// deadline the user has to understand.
pub async fn purge_days(context: &Context) -> Result<i64> {
    Ok(context.get_config_int(Config::TrashPurgeDays).await?.into())
}

/// Sets how long a trashed message stays recoverable, in days.
///
/// Does not retroactively re-time what is already in the trash: each message
/// keeps the deadline it was given when it arrived there, so a countdown the
/// user is watching cannot move under them.
pub async fn set_purge_days(context: &Context, days: i64) -> Result<()> {
    context
        .set_config(Config::TrashPurgeDays, Some(&days.max(0).to_string()))
        .await
}

async fn purge_secs(context: &Context) -> Result<i64> {
    Ok(purge_days(context).await?.saturating_mul(86_400))
}

/// Why a message is in the trash.
///
/// Kept because "this expired" and "you deleted this" read very differently to
/// someone looking at a message they did not expect to find here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The user threw it away.
    Deleted = 0,
    /// Its ephemeral timer fired.
    Expired = 1,
    /// It was held from an unverified sender and never accepted.
    Unaccepted = 2,
}

impl Reason {
    fn from_i64(value: i64) -> Self {
        match value {
            1 => Reason::Expired,
            2 => Reason::Unaccepted,
            _ => Reason::Deleted,
        }
    }
}

/// What the trash knows about a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trashed {
    /// When it was trashed.
    pub trashed_at: i64,
    /// When it stops being recoverable.
    pub purge_at: i64,
    /// Why it is here.
    pub reason: Reason,
}

/// Diverts messages whose ephemeral timer has fired into the trash.
///
/// Must run **before** core's `select_expired_messages`, which is why it is
/// called from the top of `delete_expired_messages` rather than from
/// housekeeping. Returns how many were diverted.
pub(crate) async fn divert(context: &Context, now: i64) -> Result<usize> {
    // A zero window means the user wants a fired timer to destroy the message,
    // so there is nothing to divert and core's own sweep does the work.
    if purge_days(context).await? <= 0 {
        return Ok(0);
    }
    let expired: Vec<MsgId> = context
        .sql
        .query_map_vec(
            "SELECT id FROM msgs
             WHERE ephemeral_timestamp != 0 AND ephemeral_timestamp <= ?1 AND chat_id != ?2",
            (now, DC_CHAT_ID_TRASH),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await?;
    if expired.is_empty() {
        return Ok(0);
    }

    to_trash(context, &expired, Reason::Expired, now).await?;

    // Clearing the timestamp is what makes this a diversion rather than a
    // duplicate: core's sweep runs immediately after and must not find these.
    let ids = expired.clone();
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                transaction.execute(
                    "UPDATE msgs SET ephemeral_timestamp=0 WHERE id=?",
                    (msg_id,),
                )?;
            }
            Ok(())
        })
        .await?;

    info!(
        context,
        "Ephemeral timer fired for {} message(s); moved to Trash for {} days.",
        expired.len(),
        purge_days(context).await?
    );
    context.emit_msgs_changed_without_ids();
    Ok(expired.len())
}

/// Throws messages away: applies `Trash` and schedules a purge.
pub async fn trash(context: &Context, msgs: &[MsgId]) -> Result<()> {
    to_trash(context, msgs, Reason::Deleted, time()).await?;
    context.emit_msgs_changed_without_ids();
    Ok(())
}

pub(crate) async fn to_trash(
    context: &Context,
    msgs: &[MsgId],
    reason: Reason,
    now: i64,
) -> Result<()> {
    if msgs.is_empty() {
        return Ok(());
    }
    let trash_label = labels::reserved(context, TRASH).await?;
    // A user throwing something away is an intent worth carrying to their other
    // devices; a timer firing is not, because every device runs the same timer
    // and reaches the same conclusion on its own.
    let sync = match reason {
        Reason::Deleted => Sync::Sync,
        // A timer firing is not an intent worth carrying, because every device
        // runs the same timer and reaches the same conclusion on its own; nor is
        // a sweep, for the same reason and because the hold it ends was never
        // synced either.
        Reason::Expired | Reason::Unaccepted => Sync::Nosync,
    };
    labels::set_ext(context, msgs, &trash_label, true, sync).await?;

    let ids: Vec<MsgId> = msgs.to_vec();
    let purge_at = now.saturating_add(purge_secs(context).await?);
    let reason = reason as i64;
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                // `DO NOTHING`, not `DO UPDATE`: a message already in the trash
                // keeps its original deadline, so re-trashing cannot be used to
                // extend it indefinitely, and an expiry that fires twice cannot
                // reset a countdown the user is watching.
                transaction.execute(
                    "INSERT INTO trashed_msgs (msg_id, trashed_at, purge_at, reason)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(msg_id) DO NOTHING",
                    (msg_id, now, purge_at, reason),
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Takes messages back out of the trash.
///
/// The message keeps whatever ephemeral timer its conversation has, but its own
/// countdown is not restarted: restoring a message the user asked to keep and
/// then expiring it again an hour later would be a bug wearing a feature's
/// clothes.
pub async fn restore(context: &Context, msgs: &[MsgId]) -> Result<()> {
    if msgs.is_empty() {
        return Ok(());
    }
    let trash_label = labels::reserved(context, TRASH).await?;
    labels::set_ext(context, msgs, &trash_label, false, Sync::Sync).await?;
    let ids: Vec<MsgId> = msgs.to_vec();
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                transaction.execute("DELETE FROM trashed_msgs WHERE msg_id=?", (msg_id,))?;
                transaction.execute(
                    "UPDATE msgs SET ephemeral_timestamp=0 WHERE id=?",
                    (msg_id,),
                )?;
            }
            Ok(())
        })
        .await?;
    context.emit_msgs_changed_without_ids();
    Ok(())
}

/// What the trash knows about a message, or `None` if it is not in the trash.
pub async fn trashed(context: &Context, msg_id: MsgId) -> Result<Option<Trashed>> {
    context
        .sql
        .query_row_optional(
            "SELECT trashed_at, purge_at, reason FROM trashed_msgs WHERE msg_id=?",
            (msg_id,),
            |row| {
                Ok(Trashed {
                    trashed_at: row.get(0)?,
                    purge_at: row.get(1)?,
                    reason: Reason::from_i64(row.get(2)?),
                })
            },
        )
        .await
}

/// Sets one message's ephemeral timer, overriding whatever its conversation has.
///
/// [`Timer::Disabled`] clears it, which is how a user takes an expiry off a
/// message they decided to keep. Issue #3 asks for exactly this: the timer is
/// the user's, per message, not a property of the conversation they cannot
/// escape.
pub async fn set_message_timer(context: &Context, msg_id: MsgId, timer: Timer) -> Result<()> {
    let expires_at = match timer {
        Timer::Disabled => 0,
        Timer::Enabled { duration } => time().saturating_add(i64::from(duration.get())),
    };
    context
        .sql
        .execute(
            "UPDATE msgs SET ephemeral_timestamp=? WHERE id=?",
            (expires_at, msg_id),
        )
        .await?;
    // The scheduler sleeps until the next known expiry, so a timer set to fire
    // sooner than that has to wake it or it fires late.
    context.scheduler.interrupt_ephemeral_task().await;
    Ok(())
}

/// When one message expires, or `None` if it has no timer.
pub async fn message_expires_at(context: &Context, msg_id: MsgId) -> Result<Option<i64>> {
    let at: Option<i64> = context
        .sql
        .query_get_value("SELECT ephemeral_timestamp FROM msgs WHERE id=?", (msg_id,))
        .await?;
    Ok(at.filter(|&at| at != 0))
}

/// The messages in the trash, newest first.
pub async fn in_trash(context: &Context) -> Result<Vec<MsgId>> {
    let trash_label = labels::reserved(context, TRASH).await?;
    labels::msgs_with(context, trash_label.id).await
}

/// Destroys trashed messages whose recoverable window has elapsed.
///
/// Runs in housekeeping. The deadline is local and never synced, for the same
/// reason [`super::gating`]'s is: a device's own clock is the only one it can
/// reason about.
pub async fn purge(context: &Context) -> Result<usize> {
    let now = time();
    let due: Vec<MsgId> = context
        .sql
        .query_map_vec(
            "SELECT msg_id FROM trashed_msgs WHERE purge_at<=?",
            (now,),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await?;

    if due.is_empty() {
        return context
            .sql
            .execute(
                // Trashed messages keep a tombstone row in `msgs` to suppress
                // re-download, so "not in msgs" would never match one. Their
                // content is gone, so they count as deleted -- the same rule
                // `rawmime::expire` follows.
                "DELETE FROM trashed_msgs WHERE msg_id NOT IN \
                 (SELECT id FROM msgs WHERE chat_id!=?)",
                (DC_CHAT_ID_TRASH,),
            )
            .await;
    }

    for &msg_id in &due {
        msg_id.trash(context, true).await?;
    }
    let ids = due.clone();
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                transaction.execute("DELETE FROM trashed_msgs WHERE msg_id=?", (msg_id,))?;
            }
            Ok(())
        })
        .await?;
    info!(context, "Purged {} trashed message(s).", due.len());
    context.emit_msgs_changed_without_ids();
    Ok(due.len())
}

#[cfg(test)]
mod ephemeral_tests;
