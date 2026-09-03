//! Labels, tags and archive.
//!
//! With no IMAP folders ([ADR 0003]), organization is entirely client-side, so
//! we chose labels over a tree ([ADR 0005]): `msg_labels` is many-to-many by
//! construction, and a thread whose messages carry different labels is
//! representable, where "which folder is this thread in?" has no good answer.
//!
//! Labels never reach the server. They are synced between the user's own
//! devices over core's existing sync-message channel, by *name* rather than by
//! row id, because ids are assigned per device.
//!
//! # Archive
//!
//! ADR 0005 says "archive is simply the removal of the Inbox label". This
//! implements the equivalent inverse: archive is the *presence* of a reserved
//! `Archive` label, and the inbox is what has neither been archived nor moved
//! elsewhere. See [ADR 0009] for why. In short: every hook we install is
//! best-effort, because none of them may fail message reception. If archive
//! were the absence of an Inbox label, a hook that failed to apply Inbox would
//! make a message vanish from both views. As the presence of a label, a failed
//! hook leaves the message in the inbox -- visible and recoverable.
//!
//! # Sync ordering
//!
//! A label applied on one device is synced independently of the message it
//! refers to, and routinely arrives before it. Core's own sync handlers warn
//! and drop in that case, which is tolerable for a deletion (the message may
//! never arrive) and not for a label (the user would watch a label they applied
//! on their phone silently fail to appear on their laptop). So an application
//! naming an unknown message is parked in `pending_msg_labels` and drained by
//! [`drain_pending`] when the message arrives.
//!
//! [ADR 0003]: ../../../docs/adr/0003-imap-as-transport.md
//! [ADR 0005]: ../../../docs/adr/0005-labels-not-folders.md
//! [ADR 0009]: ../../../docs/adr/0009-labels-and-search.md

use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::context::Context;
use crate::log::warn;
use crate::message::MsgId;
use crate::sync::{Sync, SyncData};
use crate::tools::time;

/// Name of the reserved label that marks a message as archived.
pub const ARCHIVE: &str = "Archive";

/// Name of the reserved label for messages the user threw away, and for
/// ephemeral messages whose timer has fired. See
/// `docs/adr/0019-recoverable-ephemeral-expiry.md`.
pub const TRASH: &str = "Trash";

/// Name of the reserved label for mail from a sender who is neither verified
/// nor known. See `docs/adr/0018-contact-gating.md`.
pub const UNVERIFIED: &str = "Unverified";

/// Reserved names are not localised.
///
/// A name is the identifier on the sync wire, so translating it would make two
/// devices in two locales disagree about which tag is which. The UI translates
/// the display string. See `docs/adr/0017-system-tags.md`.
pub const RESERVED: [&str; 3] = [ARCHIVE, TRASH, UNVERIFIED];

/// How long a label application for a message we do not have is kept.
///
/// Long enough to cover a device that has been offline for a while, short
/// enough that applications for messages which will never arrive do not
/// accumulate forever.
const PENDING_TTL: i64 = 90 * 86_400;

/// Identifier of a label, a row in `labels`.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LabelId(i64);

impl LabelId {
    /// Wraps a raw database id.
    pub fn new(id: i64) -> Self {
        LabelId(id)
    }

    /// Returns the raw database id.
    pub fn to_i64(self) -> i64 {
        self.0
    }
}

impl rusqlite::types::ToSql for LabelId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Integer(self.0),
        ))
    }
}

impl rusqlite::types::FromSql for LabelId {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(LabelId)
    }
}

/// A label as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Local row id. Differs between the user's devices; sync uses the name.
    pub id: LabelId,

    /// Name as the user typed it.
    pub name: String,

    /// `0xRRGGBB`, or `None` if the user picked no colour.
    pub color: Option<u32>,

    /// True for labels we reserve, which cannot be renamed or deleted.
    pub is_system: bool,
}

/// Names are unique case-insensitively, so "Work" and "work" are one label.
fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

fn row_to_label(row: &rusqlite::Row) -> rusqlite::Result<Label> {
    Ok(Label {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row
            .get::<_, Option<i64>>(2)?
            .and_then(|c| u32::try_from(c).ok()),
        is_system: row.get::<_, i64>(3)? != 0,
    })
}

const SELECT_LABEL: &str = "SELECT id, name, color, system FROM labels";

/// Lists all labels, system ones first, then alphabetically.
pub async fn list(context: &Context) -> Result<Vec<Label>> {
    context
        .sql
        .query_map_vec(
            &format!("{SELECT_LABEL} ORDER BY system DESC, name_norm"),
            (),
            |row| Ok(row_to_label(row)?),
        )
        .await
}

/// Looks a label up by name, case-insensitively.
pub async fn by_name(context: &Context, name: &str) -> Result<Option<Label>> {
    context
        .sql
        .query_row_optional(
            &format!("{SELECT_LABEL} WHERE name_norm=?"),
            (normalize(name),),
            row_to_label,
        )
        .await
}

/// Returns the reserved archive label.
pub async fn archive_label(context: &Context) -> Result<Label> {
    reserved(context, ARCHIVE).await
}

/// Returns a reserved label by name.
///
/// Reserved labels are created by the migration, so a missing one is a
/// corrupted database rather than something to paper over by creating it here:
/// creating it would give this device a label the user's other devices do not
/// have, and the sync channel would then carry it as if it were a user tag.
pub async fn reserved(context: &Context, name: &str) -> Result<Label> {
    debug_assert!(RESERVED.contains(&name), "{name} is not a reserved label");
    by_name(context, name)
        .await?
        .with_context(|| format!("reserved label {name} is missing; the database was not migrated"))
}

/// Creates a label, or returns the existing one if the name is already taken.
///
/// Creation is idempotent because it is also how a synced label arrives: two
/// devices creating "Work" independently must converge on one label, not
/// collide.
pub async fn create(context: &Context, name: &str, color: Option<u32>) -> Result<Label> {
    create_ext(context, name, color, Sync::Sync).await
}

pub(crate) async fn create_ext(
    context: &Context,
    name: &str,
    color: Option<u32>,
    sync: Sync,
) -> Result<Label> {
    let trimmed = name.trim().to_string();
    ensure!(!trimmed.is_empty(), "label name cannot be empty");
    let name_norm = normalize(&trimmed);

    if let Some(existing) = by_name(context, &name_norm).await? {
        return Ok(existing);
    }

    context
        .sql
        .execute(
            "INSERT INTO labels (name, name_norm, color, system, created)
             VALUES (?1, ?2, ?3, 0, ?4)
             ON CONFLICT(name_norm) DO NOTHING",
            (&trimmed, &name_norm, color.map(i64::from), time()),
        )
        .await?;

    if sync == Sync::Sync {
        context
            .add_sync_item(SyncData::EmailLabel(LabelSyncItem::Create {
                name: trimmed.clone(),
                color,
            }))
            .await?;
        context.scheduler.interrupt_inbox().await;
    }

    by_name(context, &name_norm)
        .await?
        .context("label vanished immediately after being created")
}

/// Renames a label. System labels cannot be renamed.
pub async fn rename(context: &Context, id: LabelId, new_name: &str) -> Result<()> {
    let label = load(context, id).await?;
    rename_ext(context, &label, new_name, Sync::Sync).await
}

pub(crate) async fn rename_ext(
    context: &Context,
    label: &Label,
    new_name: &str,
    sync: Sync,
) -> Result<()> {
    ensure!(
        !label.is_system,
        "{} is a reserved label and cannot be renamed",
        label.name
    );
    let trimmed = new_name.trim().to_string();
    ensure!(!trimmed.is_empty(), "label name cannot be empty");
    let name_norm = normalize(&trimmed);

    if let Some(existing) = by_name(context, &name_norm).await?
        && existing.id != label.id
    {
        bail!("a label named {trimmed:?} already exists");
    }

    context
        .sql
        .execute(
            "UPDATE labels SET name=?1, name_norm=?2 WHERE id=?3",
            (&trimmed, &name_norm, label.id),
        )
        .await?;

    if sync == Sync::Sync {
        context
            .add_sync_item(SyncData::EmailLabel(LabelSyncItem::Rename {
                from: label.name.clone(),
                to: trimmed,
            }))
            .await?;
        context.scheduler.interrupt_inbox().await;
    }
    Ok(())
}

/// Sets or clears a label's colour.
pub async fn set_color(context: &Context, id: LabelId, color: Option<u32>) -> Result<()> {
    let label = load(context, id).await?;
    set_color_ext(context, &label, color, Sync::Sync).await
}

pub(crate) async fn set_color_ext(
    context: &Context,
    label: &Label,
    color: Option<u32>,
    sync: Sync,
) -> Result<()> {
    context
        .sql
        .execute(
            "UPDATE labels SET color=?1 WHERE id=?2",
            (color.map(i64::from), label.id),
        )
        .await?;

    if sync == Sync::Sync {
        context
            .add_sync_item(SyncData::EmailLabel(LabelSyncItem::SetColor {
                name: label.name.clone(),
                color,
            }))
            .await?;
        context.scheduler.interrupt_inbox().await;
    }
    Ok(())
}

/// Deletes a label and removes it from every message. System labels cannot be
/// deleted.
///
/// The messages themselves are untouched: deleting a label is an organizational
/// act, never a destructive one.
pub async fn delete(context: &Context, id: LabelId) -> Result<()> {
    let label = load(context, id).await?;
    delete_ext(context, &label, Sync::Sync).await
}

pub(crate) async fn delete_ext(context: &Context, label: &Label, sync: Sync) -> Result<()> {
    ensure!(
        !label.is_system,
        "{} is a reserved label and cannot be deleted",
        label.name
    );
    let id = label.id;
    context
        .sql
        .transaction(move |transaction| {
            transaction.execute("DELETE FROM msg_labels WHERE label_id=?", (id,))?;
            transaction.execute("DELETE FROM pending_msg_labels WHERE label_id=?", (id,))?;
            transaction.execute("DELETE FROM labels WHERE id=?", (id,))?;
            Ok(())
        })
        .await?;

    if sync == Sync::Sync {
        context
            .add_sync_item(SyncData::EmailLabel(LabelSyncItem::Delete {
                name: label.name.clone(),
            }))
            .await?;
        context.scheduler.interrupt_inbox().await;
    }
    Ok(())
}

async fn load(context: &Context, id: LabelId) -> Result<Label> {
    context
        .sql
        .query_row_optional(&format!("{SELECT_LABEL} WHERE id=?"), (id,), row_to_label)
        .await?
        .with_context(|| format!("no label with id {}", id.to_i64()))
}

/// Applies a label to messages. Already-labelled messages are left alone.
pub async fn apply(context: &Context, msgs: &[MsgId], id: LabelId) -> Result<()> {
    let label = load(context, id).await?;
    set_ext(context, msgs, &label, true, Sync::Sync).await
}

/// Removes a label from messages.
pub async fn unapply(context: &Context, msgs: &[MsgId], id: LabelId) -> Result<()> {
    let label = load(context, id).await?;
    set_ext(context, msgs, &label, false, Sync::Sync).await
}

/// Applies or removes a label, optionally without syncing.
///
/// `Sync::Nosync` is for tags a device derives for itself --
/// [`super::gating`]'s `Unverified` and [`super::ephemeral`]'s automatic `Trash`.
/// Syncing those would let one device's classification of a message override
/// another device's, when both classified the same message independently and
/// correctly.
pub(crate) async fn set_ext(
    context: &Context,
    msgs: &[MsgId],
    label: &Label,
    apply: bool,
    sync: Sync,
) -> Result<()> {
    if msgs.is_empty() {
        return Ok(());
    }
    let ids: Vec<MsgId> = msgs.to_vec();
    let label_id = label.id;
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                if apply {
                    transaction.execute(
                        "INSERT INTO msg_labels (msg_id, label_id) VALUES (?1, ?2)
                         ON CONFLICT(msg_id, label_id) DO NOTHING",
                        (msg_id, label_id),
                    )?;
                } else {
                    transaction.execute(
                        "DELETE FROM msg_labels WHERE msg_id=?1 AND label_id=?2",
                        (msg_id, label_id),
                    )?;
                }
            }
            Ok(())
        })
        .await?;

    if sync == Sync::Sync {
        // Sync by Message-ID: row ids differ between devices.
        let mids = rfc724_mids(context, msgs).await?;
        if !mids.is_empty() {
            let item = if apply {
                LabelSyncItem::Apply {
                    msgs: mids,
                    label: label.name.clone(),
                }
            } else {
                LabelSyncItem::Unapply {
                    msgs: mids,
                    label: label.name.clone(),
                }
            };
            context.add_sync_item(SyncData::EmailLabel(item)).await?;
            context.scheduler.interrupt_inbox().await;
        }
    }
    Ok(())
}

async fn rfc724_mids(context: &Context, msgs: &[MsgId]) -> Result<Vec<String>> {
    let mut mids = Vec::with_capacity(msgs.len());
    for msg_id in msgs {
        let mid: Option<String> = context
            .sql
            .query_get_value(
                "SELECT IFNULL(rfc724_mid, '') FROM msgs WHERE id=?",
                (msg_id,),
            )
            .await?;
        if let Some(mid) = mid
            && !mid.is_empty()
        {
            mids.push(mid);
        }
    }
    Ok(mids)
}

/// Returns the labels on a message, system ones first.
pub async fn of_msg(context: &Context, msg_id: MsgId) -> Result<Vec<Label>> {
    context
        .sql
        .query_map_vec(
            "SELECT l.id, l.name, l.color, l.system
             FROM msg_labels ml JOIN labels l ON l.id=ml.label_id
             WHERE ml.msg_id=?
             ORDER BY l.system DESC, l.name_norm",
            (msg_id,),
            |row| Ok(row_to_label(row)?),
        )
        .await
}

/// Returns the messages carrying a label, newest first.
pub async fn msgs_with(context: &Context, id: LabelId) -> Result<Vec<MsgId>> {
    context
        .sql
        .query_map_vec(
            "SELECT m.id FROM msg_labels ml
             JOIN msgs m ON m.id=ml.msg_id
             WHERE ml.label_id=? AND m.chat_id!=? AND m.hidden=0
             ORDER BY m.timestamp DESC, m.id DESC",
            (id, crate::constants::DC_CHAT_ID_TRASH),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await
}

/// Archives messages: applies the reserved archive label.
pub async fn archive(context: &Context, msgs: &[MsgId]) -> Result<()> {
    let label = archive_label(context).await?;
    set_ext(context, msgs, &label, true, Sync::Sync).await
}

/// Moves messages back to the inbox: removes the reserved archive label.
pub async fn unarchive(context: &Context, msgs: &[MsgId]) -> Result<()> {
    let label = archive_label(context).await?;
    set_ext(context, msgs, &label, false, Sync::Sync).await
}

/// Whether a message has been archived.
pub async fn is_archived(context: &Context, msg_id: MsgId) -> Result<bool> {
    context
        .sql
        .exists(
            "SELECT COUNT(*) FROM msg_labels ml JOIN labels l ON l.id=ml.label_id
             WHERE ml.msg_id=? AND l.name_norm=?",
            (msg_id, normalize(ARCHIVE)),
        )
        .await
}

// ---------------------------------------------------------------------------
// Device sync
// ---------------------------------------------------------------------------

/// A label change to replay on the user's other devices.
///
/// Everything is named rather than identified by row id, because ids are
/// assigned per device. Archiving is deliberately not a variant of its own: it
/// is `Apply`/`Unapply` of the reserved archive label, so there is one code
/// path to get right.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum LabelSyncItem {
    /// A label was created.
    Create {
        /// Name as the user typed it.
        name: String,
        /// `0xRRGGBB`, if one was chosen.
        color: Option<u32>,
    },
    /// A label was renamed.
    Rename {
        /// Previous name.
        from: String,
        /// New name.
        to: String,
    },
    /// A label was deleted.
    Delete {
        /// Name of the deleted label.
        name: String,
    },
    /// A label's colour changed.
    SetColor {
        /// Name of the label.
        name: String,
        /// New colour, or `None` to clear it.
        color: Option<u32>,
    },
    /// A label was applied to messages, identified by `Message-ID`.
    Apply {
        /// `Message-ID`s of the affected messages.
        msgs: Vec<String>,
        /// Name of the label.
        label: String,
    },
    /// A label was removed from messages, identified by `Message-ID`.
    Unapply {
        /// `Message-ID`s of the affected messages.
        msgs: Vec<String>,
        /// Name of the label.
        label: String,
    },
}

/// Replays a label change from another device.
///
/// Must never add sync items of its own: two devices echoing each other would
/// loop forever. That is why every mutation here goes through an `_ext` form
/// with [`Sync::Nosync`].
pub(crate) async fn execute_sync_item(
    context: &Context,
    item: &LabelSyncItem,
    timestamp: i64,
) -> Result<()> {
    match item {
        LabelSyncItem::Create { name, color } => {
            create_ext(context, name, *color, Sync::Nosync).await?;
        }
        LabelSyncItem::Rename { from, to } => {
            // A rename for a label we never heard of still has to land, or the
            // two devices diverge; create it under its new name.
            match by_name(context, from).await? {
                Some(label) => rename_ext(context, &label, to, Sync::Nosync).await?,
                None => {
                    create_ext(context, to, None, Sync::Nosync).await?;
                }
            }
        }
        LabelSyncItem::Delete { name } => {
            if let Some(label) = by_name(context, name).await? {
                delete_ext(context, &label, Sync::Nosync).await?;
            }
        }
        LabelSyncItem::SetColor { name, color } => {
            let label = match by_name(context, name).await? {
                Some(label) => label,
                None => create_ext(context, name, *color, Sync::Nosync).await?,
            };
            set_color_ext(context, &label, *color, Sync::Nosync).await?;
        }
        LabelSyncItem::Apply { msgs, label } => {
            sync_set(context, msgs, label, true, timestamp).await?;
        }
        LabelSyncItem::Unapply { msgs, label } => {
            sync_set(context, msgs, label, false, timestamp).await?;
        }
    }
    Ok(())
}

async fn sync_set(
    context: &Context,
    mids: &[String],
    label_name: &str,
    apply: bool,
    timestamp: i64,
) -> Result<()> {
    // The label may not exist here yet if its creation is later in the same
    // batch, or was lost. Applying a label implies it exists.
    let label = match by_name(context, label_name).await? {
        Some(label) => label,
        None => create_ext(context, label_name, None, Sync::Nosync).await?,
    };

    let mut resolved = Vec::new();
    let mut pending = Vec::new();
    for mid in mids {
        match crate::message::rfc724_mid_exists(context, mid).await? {
            Some(msg_id) => resolved.push(msg_id),
            None => pending.push(mid.clone()),
        }
    }

    set_ext(context, &resolved, &label, apply, Sync::Nosync).await?;

    if !pending.is_empty() {
        let label_id = label.id;
        let count = pending.len();
        context
            .sql
            .transaction(move |transaction| {
                for mid in &pending {
                    // Latest intent wins, so an apply followed by an unapply
                    // does not resurrect the label when the message arrives.
                    transaction.execute(
                        "INSERT INTO pending_msg_labels (rfc724_mid, label_id, apply, timestamp)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(rfc724_mid, label_id) DO UPDATE SET
                            apply=excluded.apply, timestamp=excluded.timestamp
                         WHERE excluded.timestamp >= pending_msg_labels.timestamp",
                        (mid, label_id, apply, timestamp),
                    )?;
                }
                Ok(())
            })
            .await?;
        info!(
            context,
            "Parked {count} label change(s) for message(s) not yet received."
        );
    }
    Ok(())
}

/// Applies any label changes that were waiting for this message.
///
/// Called from the receive hook. Best-effort like its neighbours: a message
/// that arrives without its parked labels is a visible annoyance, not a
/// failure to receive.
pub(crate) async fn drain_pending(context: &Context, msg_id: MsgId) -> Result<()> {
    let Some(rfc724_mid): Option<String> = context
        .sql
        .query_get_value(
            "SELECT IFNULL(rfc724_mid, '') FROM msgs WHERE id=?",
            (msg_id,),
        )
        .await?
    else {
        return Ok(());
    };
    if rfc724_mid.is_empty() {
        return Ok(());
    }

    let pending: Vec<(LabelId, bool)> = context
        .sql
        .query_map_vec(
            "SELECT label_id, apply FROM pending_msg_labels WHERE rfc724_mid=?",
            (&rfc724_mid,),
            |row| Ok((row.get::<_, LabelId>(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .await?;
    if pending.is_empty() {
        return Ok(());
    }

    context
        .sql
        .transaction(move |transaction| {
            for (label_id, apply) in &pending {
                if *apply {
                    transaction.execute(
                        "INSERT INTO msg_labels (msg_id, label_id) VALUES (?1, ?2)
                         ON CONFLICT(msg_id, label_id) DO NOTHING",
                        (msg_id, label_id),
                    )?;
                } else {
                    transaction.execute(
                        "DELETE FROM msg_labels WHERE msg_id=?1 AND label_id=?2",
                        (msg_id, label_id),
                    )?;
                }
            }
            transaction.execute(
                "DELETE FROM pending_msg_labels WHERE rfc724_mid=?",
                (&rfc724_mid,),
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// Removes label rows for messages that no longer exist, and parked changes for
/// messages that never arrived.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM msg_labels WHERE msg_id NOT IN \
             (SELECT id FROM msgs WHERE chat_id!=?)",
            (crate::constants::DC_CHAT_ID_TRASH,),
        )
        .await?;
    let dropped = context
        .sql
        .execute(
            "DELETE FROM pending_msg_labels WHERE timestamp < ?",
            (time().saturating_sub(PENDING_TTL),),
        )
        .await?;
    if dropped > 0 {
        warn!(
            context,
            "Dropped {dropped} label change(s) for message(s) that never arrived."
        );
    }
    Ok(())
}

#[cfg(test)]
mod labels_tests;
