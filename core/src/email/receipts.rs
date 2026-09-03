//! Read receipts and ephemeral messages.
//!
//! Both are on by default in the sense the user asked for, but they are not the
//! same kind of setting and this module treats them differently on purpose.
//!
//! # Read receipts
//!
//! Core already defaults `MdnsEnabled` to on and has exactly one place where it
//! decides whether to send an MDN, with no notion of *who* it is for. An email
//! client needs that: read receipts are a disclosure, and who you are willing
//! to disclose to depends on the correspondent.
//!
//! So [`MdnPolicy`] adds a middle setting between never and always: send only
//! to contacts who are **verified and in the address book**. Verified means
//! SecureJoin, so it survives an active attacker; in the address book means you
//! chose to know them, rather than they mailed you once.
//!
//! `MdnsEnabled` stays authoritative for on-vs-off. A user who turns read
//! receipts off means it, and must not find them still going to some contacts.
//! Per-contact overrides sit on top of the policy but under that hard off.
//!
//! # Ephemeral messages
//!
//! Ephemeral deletion removes the message locally as well, and the local store
//! is the only durable copy of the mailbox
//! ([ADR 0004](../../../docs/adr/0004-local-store-and-raw-mime.md)). That used
//! to mean a non-zero default quietly destroyed the user's mail; since
//! [ADR 0019](../../../docs/adr/0019-recoverable-ephemeral-expiry.md) a fired
//! timer moves the message to `Trash` for
//! [`super::ephemeral::DEFAULT_PURGE_DAYS`] first, so it no longer does.
//!
//! The machinery here is complete -- a global default, per-contact overrides,
//! and automatic application to a conversation -- and honours whatever
//! [`Config::EphemeralDefaultSeconds`] is set to. It ships as `0`, which is now
//! a preference rather than a safety measure: whether mail expires is the
//! user's call, and no duration is right for everyone's. Revisited and
//! confirmed in issue #3; see ADR 0019.
//!
//! The default is applied when the **first** message is sent to a conversation.
//! Not on every send, or turning the timer off would not stick; not at chat
//! creation, which would need hooks in every path that makes a chat.

use anyhow::Result;
use std::num::NonZero;

use crate::chat::ChatId;
use crate::config::Config;
use crate::contact::{Contact, ContactId};
use crate::context::Context;
use crate::ephemeral::Timer;

/// Who gets read receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i64)]
pub enum MdnPolicy {
    /// Never send read receipts.
    Never = 0,

    /// Only to contacts verified through SecureJoin who are also in the address
    /// book.
    VerifiedOnly = 1,

    /// To anyone who asks. Core's behaviour, and the default.
    Always = 2,
}

impl MdnPolicy {
    fn from_i64(value: i64) -> Self {
        match value {
            0 => MdnPolicy::Never,
            1 => MdnPolicy::VerifiedOnly,
            // An unrecognized value is the default, not the most disclosing
            // option we happen to have.
            _ => MdnPolicy::Always,
        }
    }

    /// Reads the policy.
    ///
    /// `MdnsEnabled` off means [`MdnPolicy::Never`], whatever this key says.
    pub async fn load(context: &Context) -> Result<Self> {
        // Through core's own predicate rather than reading `MdnsEnabled`
        // directly, so if upstream ever adds a condition to "should we send
        // read receipts at all", we inherit it instead of diverging.
        if !context.should_send_mdns().await? {
            return Ok(MdnPolicy::Never);
        }
        Ok(Self::from_i64(
            context.get_config_int(Config::MdnPolicy).await?.into(),
        ))
    }

    /// Sets the policy, keeping `MdnsEnabled` in step.
    pub async fn set(context: &Context, policy: MdnPolicy) -> Result<()> {
        context
            .set_config_bool(Config::MdnsEnabled, policy != MdnPolicy::Never)
            .await?;
        context
            .set_config(Config::MdnPolicy, Some(&(policy as i64).to_string()))
            .await?;
        Ok(())
    }
}

/// Reads a contact's read-receipt override, if it has one.
pub async fn mdn_for_contact(context: &Context, id: ContactId) -> Result<Option<bool>> {
    let value: Option<Option<i64>> = context
        .sql
        .query_row_optional(
            "SELECT mdn_enabled FROM contact_policy WHERE contact_id=?",
            (id,),
            |row| row.get::<_, Option<i64>>(0),
        )
        .await?;
    Ok(value.flatten().map(|v| v != 0))
}

/// Sets or clears a contact's read-receipt override.
pub async fn set_mdn_for_contact(
    context: &Context,
    id: ContactId,
    enabled: Option<bool>,
) -> Result<()> {
    upsert_contact_policy(context, id, "mdn_enabled", enabled.map(i64::from)).await
}

/// Whether to send a read receipt to `contact_id`.
///
/// This is the single decision core makes, given a correspondent. The ordering
/// matters: the global off wins over everything, because a user who turned read
/// receipts off must not keep sending them to anyone.
pub async fn should_send_mdn(context: &Context, contact_id: ContactId) -> Result<bool> {
    let policy = MdnPolicy::load(context).await?;
    if policy == MdnPolicy::Never {
        return Ok(false);
    }
    if let Some(enabled) = mdn_for_contact(context, contact_id).await? {
        return Ok(enabled);
    }
    match policy {
        MdnPolicy::Never => Ok(false),
        MdnPolicy::Always => Ok(true),
        MdnPolicy::VerifiedOnly => {
            let Ok(contact) = Contact::get_by_id(context, contact_id).await else {
                return Ok(false);
            };
            Ok(contact.is_verified(context).await? && contact.origin.is_known())
        }
    }
}

/// Reads a contact's ephemeral-timer override, if it has one.
pub async fn timer_for_contact(context: &Context, id: ContactId) -> Result<Option<Timer>> {
    let value: Option<Option<i64>> = context
        .sql
        .query_row_optional(
            "SELECT ephemeral_secs FROM contact_policy WHERE contact_id=?",
            (id,),
            |row| row.get::<_, Option<i64>>(0),
        )
        .await?;
    Ok(value.flatten().map(timer_from_secs))
}

/// Sets or clears a contact's ephemeral-timer override.
pub async fn set_timer_for_contact(
    context: &Context,
    id: ContactId,
    timer: Option<Timer>,
) -> Result<()> {
    let secs = timer.map(|t| i64::from(t.to_u32()));
    upsert_contact_policy(context, id, "ephemeral_secs", secs).await
}

/// The globally configured default timer for new conversations.
pub async fn default_timer(context: &Context) -> Result<Timer> {
    Ok(timer_from_secs(
        context
            .get_config_int(Config::EphemeralDefaultSeconds)
            .await?
            .into(),
    ))
}

/// Sets the default timer for new conversations.
pub async fn set_default_timer(context: &Context, timer: Timer) -> Result<()> {
    context
        .set_config(
            Config::EphemeralDefaultSeconds,
            Some(&timer.to_u32().to_string()),
        )
        .await?;
    Ok(())
}

fn timer_from_secs(secs: i64) -> Timer {
    match u32::try_from(secs).ok().and_then(NonZero::new) {
        Some(duration) => Timer::Enabled { duration },
        // Zero, negative and out-of-range all mean "no timer". A bad value must
        // not turn into an arbitrarily short one that deletes mail.
        None => Timer::Disabled,
    }
}

/// The timer a conversation with `contacts` should start with: the shortest of
/// any per-contact override, falling back to the global default.
///
/// Shortest rather than longest, because an override is a statement about that
/// correspondent. If you have asked for messages to a lawyer to vanish in an
/// hour, a group that includes them should not keep them for a week.
pub async fn effective_default_timer(context: &Context, contacts: &[ContactId]) -> Result<Timer> {
    let mut timer = default_timer(context).await?;
    for &id in contacts {
        if id == ContactId::SELF {
            continue;
        }
        if let Some(override_timer) = timer_for_contact(context, id).await? {
            timer = shorter(timer, override_timer);
        }
    }
    Ok(timer)
}

/// The more aggressive of two timers. `Disabled` is the longest of all.
fn shorter(a: Timer, b: Timer) -> Timer {
    match (a, b) {
        (Timer::Disabled, other) | (other, Timer::Disabled) => other,
        (Timer::Enabled { duration: x }, Timer::Enabled { duration: y }) => {
            Timer::Enabled { duration: x.min(y) }
        }
    }
}

/// Applies the default ephemeral timer to a conversation being written to for
/// the first time.
///
/// Called from the send hook. Does nothing when the conversation already has a
/// timer, or when anything has been sent to it before -- otherwise turning the
/// timer off would not stick, because the next message would put it back.
///
/// Sets the timer without announcing it separately: the message being sent
/// carries the `Ephemeral-Timer` header itself, which is what tells the other
/// side.
pub(crate) async fn apply_default_timer(
    context: &Context,
    chat_id: ChatId,
    msg_id: crate::message::MsgId,
    contacts: &[ContactId],
) -> Result<()> {
    if chat_id.get_ephemeral_timer(context).await? != Timer::Disabled {
        return Ok(());
    }
    let timer = effective_default_timer(context, contacts).await?;
    if timer == Timer::Disabled {
        return Ok(());
    }
    // Anything sent before this message means the conversation is not new, and
    // its timer is whatever the user has since chosen -- including off.
    let sent_before = context
        .sql
        .exists(
            "SELECT COUNT(*) FROM msgs WHERE chat_id=? AND from_id=? AND id<? AND hidden=0",
            (chat_id, ContactId::SELF, msg_id),
        )
        .await?;
    if sent_before {
        return Ok(());
    }
    chat_id.inner_set_ephemeral_timer(context, timer).await?;
    Ok(())
}

async fn upsert_contact_policy(
    context: &Context,
    id: ContactId,
    column: &str,
    value: Option<i64>,
) -> Result<()> {
    // The column name is a literal from this module, never user input.
    context
        .sql
        .execute(
            &format!(
                "INSERT INTO contact_policy (contact_id, {column}) VALUES (?1, ?2)
                 ON CONFLICT(contact_id) DO UPDATE SET {column}=excluded.{column}"
            ),
            (id, value),
        )
        .await?;
    Ok(())
}

/// Removes overrides for contacts that no longer exist.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM contact_policy WHERE contact_id NOT IN (SELECT id FROM contacts)",
            (),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod receipts_tests;
