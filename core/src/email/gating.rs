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
//! # What the unverified view is and is not
//!
//! Held mail is downloaded and decrypted normally. `Unverified` is a *view*, not
//! a refusal to fetch: refusing would leave the message on the server, which
//! contradicts [ADR 0003], and would make "let me see what this is" impossible
//! to answer.
//!
//! Held mail is **swept into `Trash`** after [`hold_days`], not left there.
//! Holding forever turns the view into a second inbox accumulating exactly the
//! mail nobody wanted, which is the problem this exists to solve.
//!
//! The sweep moves mail rather than destroying it. That is a change from the
//! original [ADR 0018], which discarded it: the deadline next door in
//! [`super::ephemeral`] had already grown a recoverable window ([ADR 0019]) and
//! two deadlines a few lines apart disagreeing about whether a deadline may
//! destroy the only copy of a mailbox was not a distinction anyone could defend.
//! `Trash` then applies its own deadline, and is the one place that destroys.
//!
//! [ADR 0003]: ../../../docs/adr/0003-imap-as-transport.md
//! [ADR 0018]: ../../../docs/adr/0018-contact-gating.md
//! [ADR 0019]: ../../../docs/adr/0019-recoverable-ephemeral-expiry.md

use anyhow::Result;

use crate::config::Config;
use crate::constants::DC_CHAT_ID_TRASH;
use crate::contact::{Contact, ContactId};
use crate::context::Context;
use crate::message::MsgId;
use crate::tools::time;

use crate::sync::Sync;

use super::ephemeral::{self, Reason};
use super::labels::{self, UNVERIFIED};

/// How long held mail waits for the user to accept its sender, in days.
///
/// Longer than the window in which unsolicited mail is worth anything, short
/// enough that the unverified view does not become a mailbox of its own.
///
/// This is the value [`super::policy::apply_defaults`] writes for an eeemail
/// account, not the compile-time default of [`Config::UnverifiedTrashDays`] --
/// which is `0`, meaning never sweep, for the same reason [`Config::InboxGating`]
/// ships off: upstream's tests assert that a stranger's mail reaches the inbox
/// and stays there.
pub const DEFAULT_HOLD_DAYS: i64 = 30;

/// The configured hold window in days. `0` means never sweep.
pub async fn hold_days(context: &Context) -> Result<i64> {
    Ok(context
        .get_config_int(Config::UnverifiedTrashDays)
        .await?
        .into())
}

/// Sets the hold window. `0` means never sweep.
pub async fn set_hold_days(context: &Context, days: i64) -> Result<()> {
    context
        .set_config(Config::UnverifiedTrashDays, Some(&days.max(0).to_string()))
        .await
}

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
    for other_id in same_person(context, &contact).await? {
        if let Ok(other) = Contact::get_by_id(context, other_id).await
            && other.origin.is_known()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The *other* contact rows that are the same person as `contact`.
///
/// Core keys encryption off the contact row, so one correspondent is routinely
/// several rows: an address-contact from the mail you sent them, a key-contact
/// from the encrypted reply that came back ([ADR 0021]).
///
/// Shared by [`is_trusted`] and [`release`] deliberately. Those two disagreeing
/// about which rows are the same person is not a hypothetical -- it is how held
/// mail went missing: trust was decided per person while release selected per
/// row, so a message could be trusted and still held until it purged.
///
/// Excludes `contact` itself, the special rows at or below
/// [`ContactId::LAST_SPECIAL`], and blocked rows.
///
/// [ADR 0021]: ../../../docs/adr/0021-autocrypt-key-contacts.md
async fn same_person(context: &Context, contact: &Contact) -> Result<Vec<ContactId>> {
    let addr = contact.get_addr();
    if addr.is_empty() {
        return Ok(Vec::new());
    }
    context
        .sql
        .query_map_vec(
            "SELECT id FROM contacts
             WHERE addr=?1 COLLATE NOCASE AND id!=?2 AND id>?3 AND blocked=0",
            (addr, contact.id, ContactId::LAST_SPECIAL),
            |row| Ok(row.get::<_, ContactId>(0)?),
        )
        .await
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
    let unverified = labels::reserved(context, UNVERIFIED).await?;
    // Not synced: the tag is a local classification of a message every device
    // receives and classifies for itself, and a device that trusts the sender
    // must not have Unverified pushed onto it by one that does not.
    labels::set_ext(context, &[msg_id], &unverified, true, Sync::Nosync).await?;
    // Only `held_at` is stored. The deadline is `held_at` plus the *current*
    // window, computed at sweep time, so shortening or lengthening the setting
    // moves mail that is already waiting -- which is what someone changing it
    // means. Storing a deadline as well would be a second source of truth that
    // silently outvoted the setting.
    context
        .sql
        .execute(
            "INSERT INTO held_msgs (msg_id, held_at) VALUES (?1, ?2)
             ON CONFLICT(msg_id) DO NOTHING",
            (msg_id, now),
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
///
/// Releases across every row that is the same person, not just the row passed
/// in. The two are routinely different rows: cold mail from a stranger is held
/// on their *address* row, because an unsigned first message carries no
/// fingerprint to attach, while SecureJoin verifies their *key* row and calls
/// this with that. Selecting on the key row alone finds nothing, and the mail
/// stays held -- trusted by [`is_trusted`] and invisible -- until [`sweep`]
/// moves it to `Trash` at [`hold_days`].
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
        // Trust is decided per person, so release has to be too. Widening the
        // *release* set deliberately does not widen the *trust* decision above:
        // becoming trusted still has to be earned on a row of one's own.
        let mut sender_rows = vec![contact_id];
        if let Ok(contact) = Contact::get_by_id(context, contact_id).await {
            sender_rows.extend(same_person(context, &contact).await?);
        }
        for from_id in sender_rows {
            let msgs: Vec<MsgId> = context
                .sql
                .query_map_vec(
                    "SELECT h.msg_id FROM held_msgs h
                     JOIN msgs m ON m.id=h.msg_id
                     WHERE m.from_id=?",
                    (from_id,),
                    |row| Ok(row.get::<_, MsgId>(0)?),
                )
                .await?;
            released = released.saturating_add(unhold(context, &msgs).await?);
        }
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

/// Takes `Unverified` off messages and forgets their deadline.
async fn unhold(context: &Context, msgs: &[MsgId]) -> Result<usize> {
    if msgs.is_empty() {
        return Ok(0);
    }
    let unverified = labels::reserved(context, UNVERIFIED).await?;
    labels::set_ext(context, msgs, &unverified, false, Sync::Nosync).await?;
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
    let unverified = labels::reserved(context, UNVERIFIED).await?;
    labels::msgs_with(context, unverified.id).await
}

/// Sweeps held mail whose hold has elapsed into `Trash`. Returns how many moved.
///
/// Runs in housekeeping. Sweeping is a **local** decision and is never synced: a
/// device that has been offline for six months must not come back and move mail
/// another device is still holding.
///
/// The message keeps its content and stays restorable. `Trash` carries its own
/// deadline ([`super::ephemeral`]), and that is the only one that destroys.
///
/// Note what this costs: [`release`] reads `held_msgs`, so accepting a sender
/// after their mail has been swept does *not* bring it back. It is in `Trash`,
/// where the user can see it and restore it by hand. Releasing mail out of a
/// bin the user may have deliberately emptied would be the stranger behaviour.
pub async fn sweep(context: &Context) -> Result<usize> {
    let days = hold_days(context).await?;
    // Zero means the user wants mail to wait indefinitely rather than to be
    // swept at once. It is not "sweep immediately": the immediate-sweep setting
    // is turning gating off, which releases everything to the inbox instead.
    if days <= 0 {
        return prune(context).await;
    }
    let cutoff = time().saturating_sub(days.saturating_mul(86_400));
    let expired: Vec<MsgId> = context
        .sql
        .query_map_vec(
            "SELECT msg_id FROM held_msgs WHERE held_at<=?",
            (cutoff,),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await?;
    if expired.is_empty() {
        return prune(context).await;
    }

    // Into the trash first, then out of the hold: a failure between the two
    // leaves a message tagged both ways, which the next sweep corrects, rather
    // than a message tagged neither, which nothing would ever look at again.
    ephemeral::to_trash(context, &expired, Reason::Unaccepted, time()).await?;
    let swept = unhold(context, &expired).await?;
    info!(
        context,
        "Swept {} unaccepted message(s) into Trash after {} days.", swept, days
    );
    Ok(swept)
}

/// Drops rows whose message is already gone, so the table does not outlive what
/// it describes. Returns zero, because nothing was swept.
async fn prune(context: &Context) -> Result<usize> {
    context
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
        .await?;
    Ok(0)
}

#[cfg(test)]
mod gating_tests;
