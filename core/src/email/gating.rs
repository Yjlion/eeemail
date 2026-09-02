//! Mail from strangers is held, not delivered.
//!
//! An inbox anyone can write to is an inbox anyone can spend your attention
//! from. Spam filtering answers this statistically and on a server, which is not
//! available to us: the server never sees plaintext ([ADR 0003]).
//!
//! Core already knows the two things worth knowing about a correspondent's
//! identity, and [`super::receipts`] already composes them for the
//! verified-only read-receipt policy:
//!
//! * [`Contact::is_verified`] -- SecureJoin completed, so the key survived an
//!   active attacker.
//! * [`crate::contact::Origin::is_known`] -- you chose to know them, rather than
//!   they mailed you once.
//!
//! Gating needs **either**, where read receipts need both: writing to someone is
//! enough to want their reply in your inbox, and is not enough to disclose that
//! you read it.
//!
//! # What holding is and is not
//!
//! Held mail is downloaded and decrypted normally. `Holding` is a *view*, not a
//! refusal to fetch: refusing would leave the message on the server, which
//! contradicts [ADR 0003], and would make "let me see what this is" impossible
//! to answer.
//!
//! Held mail is **purged** after [`HOLD_DAYS`], not archived. Holding forever
//! turns it into a second inbox accumulating exactly the mail nobody wanted,
//! which is the problem this exists to solve. See [ADR 0018].
//!
//! [ADR 0003]: ../../../docs/adr/0003-imap-as-transport.md
//! [ADR 0018]: ../../../docs/adr/0018-contact-gating.md

use anyhow::Result;

use crate::config::Config;
use crate::constants::DC_CHAT_ID_TRASH;
use crate::contact::{Contact, ContactId};
use crate::context::Context;
use crate::message::MsgId;
use crate::tools::time;

use crate::sync::Sync;

use super::labels::{self, HOLDING};

/// How long held mail waits for the user to accept its sender.
///
/// Longer than the window in which unsolicited mail is worth anything, short
/// enough that Holding does not become a mailbox of its own.
pub const HOLD_DAYS: i64 = 30;

const HOLD_SECS: i64 = HOLD_DAYS * 86_400;

/// Whether a correspondent's mail reaches the inbox.
///
/// Deliberately **either** verified or known, not both: see the module docs.
///
/// Trust is decided per *person*, not per contact row. Core keys encryption off
/// the row, so the same correspondent is routinely two rows -- an
/// address-contact from the mail you sent them, and a key-contact from the
/// encrypted reply that came back ([ADR 0021]). Asking only about the row the
/// message arrived on would hold the first encrypted reply from everyone the
/// user has ever written to, which is the exact opposite of what this is for.
pub async fn is_trusted(context: &Context, contact_id: ContactId) -> Result<bool> {
    if contact_id == ContactId::SELF {
        return Ok(true);
    }
    let Ok(contact) = Contact::get_by_id(context, contact_id).await else {
        // A contact we cannot load is not one we can vouch for.
        return Ok(false);
    };
    if contact.origin.is_known() || contact.is_verified(context).await? {
        return Ok(true);
    }
    // Only the *known* half carries across rows. Verification is a claim about
    // a key surviving an active attacker, and an address is not a key -- so it
    // stays where it was earned, and nothing here makes anyone verified.
    let addr = contact.get_addr();
    if addr.is_empty() {
        return Ok(false);
    }
    let same_person: Vec<ContactId> = context
        .sql
        .query_map_vec(
            "SELECT id FROM contacts
             WHERE addr=?1 COLLATE NOCASE AND id!=?2 AND id>?3 AND blocked=0",
            (addr, contact_id, ContactId::LAST_SPECIAL),
            |row| Ok(row.get::<_, ContactId>(0)?),
        )
        .await?;
    for other_id in same_person {
        if let Ok(other) = Contact::get_by_id(context, other_id).await
            && other.origin.is_known()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether gating is on, from [`Config::InboxGating`].
pub async fn is_enabled(context: &Context) -> Result<bool> {
    context.get_config_bool(Config::InboxGating).await
}

/// Turns gating on or off.
///
/// Turning it off releases everything currently held: leaving mail in a view
/// the user has just switched off would strand it there until it purged.
pub async fn set_enabled(context: &Context, enabled: bool) -> Result<()> {
    context
        .set_config_bool(Config::InboxGating, enabled)
        .await?;
    if !enabled {
        release_all(context).await?;
    }
    Ok(())
}

/// Holds an incoming message if its sender is neither verified nor known.
///
/// Called from the receive path. Best-effort like every other hook there: a
/// message that has already been received must not be lost because we could not
/// classify its sender.
pub async fn apply(context: &Context, msg_id: MsgId) -> Result<()> {
    if !is_enabled(context).await? {
        return Ok(());
    }

    let Some((from_id, is_incoming)) = sender_of(context, msg_id).await? else {
        return Ok(());
    };
    // Outgoing mail is never held. This is reachable: the send path shares this
    // hook's neighbourhood, and a self-copy arrives through reception.
    if !is_incoming || from_id == ContactId::SELF {
        return Ok(());
    }
    if is_trusted(context, from_id).await? {
        return Ok(());
    }

    let now = time();
    let holding = labels::reserved(context, HOLDING).await?;
    // Not synced: the tag is a local classification of a message every device
    // receives and classifies for itself, and a device that trusts the sender
    // must not have Holding pushed onto it by one that does not.
    labels::set_ext(context, &[msg_id], &holding, true, Sync::Nosync).await?;
    context
        .sql
        .execute(
            "INSERT INTO held_msgs (msg_id, held_at, purge_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(msg_id) DO NOTHING",
            (msg_id, now, now.saturating_add(HOLD_SECS)),
        )
        .await?;
    Ok(())
}

/// Reads a message's sender and direction, or `None` if it is gone.
async fn sender_of(context: &Context, msg_id: MsgId) -> Result<Option<(ContactId, bool)>> {
    let row: Option<(ContactId, ContactId)> = context
        .sql
        .query_row_optional(
            "SELECT from_id, to_id FROM msgs WHERE id=?",
            (msg_id,),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .await?;
    Ok(row.map(|(from_id, to_id)| (from_id, to_id == ContactId::SELF)))
}

/// Releases the held mail of contacts that are now trusted.
///
/// Called when a contact's origin is scaled up or they become verified, which
/// are the two ways trust is gained. Takes a slice because both call sites have
/// one.
pub async fn release(context: &Context, contact_ids: &[ContactId]) -> Result<usize> {
    let mut released = 0usize;
    for &contact_id in contact_ids {
        if contact_id == ContactId::SELF {
            continue;
        }
        // Checked per contact rather than assumed from the call site: origin is
        // scaled up constantly, and most of those scale-ups do not cross the
        // threshold into trusted.
        if !is_trusted(context, contact_id).await? {
            continue;
        }
        let msgs: Vec<MsgId> = context
            .sql
            .query_map_vec(
                "SELECT h.msg_id FROM held_msgs h
                 JOIN msgs m ON m.id=h.msg_id
                 WHERE m.from_id=?",
                (contact_id,),
                |row| Ok(row.get::<_, MsgId>(0)?),
            )
            .await?;
        released = released.saturating_add(unhold(context, &msgs).await?);
    }
    Ok(released)
}

/// Releases everything currently held, whatever its sender.
pub async fn release_all(context: &Context) -> Result<usize> {
    let msgs: Vec<MsgId> = context
        .sql
        .query_map_vec("SELECT msg_id FROM held_msgs", (), |row| {
            Ok(row.get::<_, MsgId>(0)?)
        })
        .await?;
    unhold(context, &msgs).await
}

/// Takes `Holding` off messages and forgets their deadline.
async fn unhold(context: &Context, msgs: &[MsgId]) -> Result<usize> {
    if msgs.is_empty() {
        return Ok(0);
    }
    let holding = labels::reserved(context, HOLDING).await?;
    labels::set_ext(context, msgs, &holding, false, Sync::Nosync).await?;
    let ids: Vec<MsgId> = msgs.to_vec();
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                transaction.execute("DELETE FROM held_msgs WHERE msg_id=?", (msg_id,))?;
            }
            Ok(())
        })
        .await?;
    context.emit_msgs_changed_without_ids();
    Ok(msgs.len())
}

/// The messages currently held, newest first.
pub async fn held(context: &Context) -> Result<Vec<MsgId>> {
    let holding = labels::reserved(context, HOLDING).await?;
    labels::msgs_with(context, holding.id).await
}

/// Discards held mail whose hold has elapsed. Returns how many were discarded.
///
/// Runs in housekeeping. Purging is a **local** decision and is never synced: a
/// device that has been offline for six months must not come back and destroy
/// mail another device is still holding.
pub async fn purge(context: &Context) -> Result<usize> {
    let now = time();
    let expired: Vec<MsgId> = context
        .sql
        .query_map_vec(
            "SELECT msg_id FROM held_msgs WHERE purge_at<=?",
            (now,),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await?;
    if expired.is_empty() {
        // Still drop rows whose message is already gone, so the table does not
        // outlive what it describes.
        return context
            .sql
            .execute(
                // Trashed messages keep a tombstone row in `msgs` to suppress
                // re-download, so "not in msgs" would never match one. Their
                // content is gone, so they count as deleted -- the same rule
                // `rawmime::expire` follows.
                "DELETE FROM held_msgs WHERE msg_id NOT IN \
                 (SELECT id FROM msgs WHERE chat_id!=?)",
                (DC_CHAT_ID_TRASH,),
            )
            .await;
    }

    for &msg_id in &expired {
        // Through core's own deletion so that the tombstone which suppresses
        // re-download is written, and the blobs and raw MIME are reclaimed.
        msg_id.trash(context, true).await?;
    }
    let ids = expired.clone();
    context
        .sql
        .transaction(move |transaction| {
            for msg_id in &ids {
                transaction.execute("DELETE FROM held_msgs WHERE msg_id=?", (msg_id,))?;
            }
            Ok(())
        })
        .await?;
    info!(context, "Purged {} held message(s).", expired.len());
    context.emit_msgs_changed_without_ids();
    Ok(expired.len())
}

#[cfg(test)]
mod gating_tests;
