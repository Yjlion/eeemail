//! Mail from a blocked sender is thrown away on arrival.
//!
//! Core already has a notion of a blocked contact -- [`Contact::block`],
//! `contacts.blocked`, `get_all_blocked` -- and it does not stop mail. A
//! blocked sender's message is still fetched, decrypted and stored; the only
//! effect is that its chat is a blocked chat and the message is marked read
//! (`receive_imf.rs`, where `from_id_blocked` is computed and then discarded).
//! For a messenger that is reasonable: the contact request is the feature. For
//! an email client, "blocked" that leaves the mail in the mailbox is a
//! promise the user can see being broken.
//!
//! # Trashed, not destroyed
//!
//! Blocked mail goes to `Trash` with a [`Reason::Blocked`] and waits out
//! [`Config::TrashPurgeDays`] like everything else. Destroying it outright was
//! the alternative and is worse in the case that matters: a pattern typed with
//! a typo, or a domain blocked more broadly than intended, silently eats mail
//! that nobody can then discover was missing. `Trash` is the only place in
//! eeemail that destroys mail, and it waits first
//! ([ADR 0019](../../../docs/adr/0019-recoverable-ephemeral-expiry.md)).
//!
//! Refusing at IMAP fetch time -- never downloading it -- was also considered
//! and rejected: it leaves the message on the server, which contradicts
//! [ADR 0003](../../../docs/adr/0003-imap-as-transport.md), and it makes "let
//! me see what I blocked" unanswerable.
//!
//! # What a pattern matches
//!
//! An address (`spam@example.com`) matches that address, case-insensitively.
//! A pattern beginning with `@` (`@example.com`) matches every address in
//! **exactly** that domain. It does not match subdomains: `@example.com` leaves
//! `mail.example.com` alone.
//!
//! That is narrower than a user might expect, and it is deliberate. A
//! subdomain rule cannot be undone selectively -- blocking `@example.com` to
//! stop one marketing subdomain would also block the person's actual mail --
//! and a blocklist whose reach the user cannot predict is one they cannot use
//! confidently on their own mailbox. Predictable beats broad.
//!
//! See [ADR 0027](../../../docs/adr/0027-a-blocklist-that-trashes-on-arrival.md).

use anyhow::{Result, bail};

use crate::contact::{Contact, ContactId};
use crate::context::Context;
use crate::message::MsgId;
use crate::tools::time;

use super::ephemeral::{self, Reason};

/// One entry in the blocklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The row id, so a client can remove one without round-tripping the text.
    pub id: i64,
    /// As the user typed it, for display.
    pub pattern: String,
    /// When it was added.
    pub added: i64,
    /// Why, if the user said. Never interpreted.
    pub reason: String,
}

/// The normalised form a pattern is matched and deduplicated by.
///
/// Lowercased and trimmed. A domain pattern keeps its leading `@`, which is
/// what distinguishes `@example.com` from an address in the stored form.
fn normalize(pattern: &str) -> String {
    pattern.trim().to_lowercase()
}

/// Checks a pattern is something that can match an address at all.
///
/// Rejected rather than stored-and-never-matched: a blocklist entry that can
/// never fire is one the user believes is protecting them.
fn validate(pattern: &str) -> Result<String> {
    let norm = normalize(pattern);
    if norm.is_empty() {
        bail!("a blocklist entry needs an address or a domain");
    }
    if let Some(domain) = norm.strip_prefix('@') {
        if domain.is_empty() || !domain.contains('.') || domain.contains('@') {
            bail!("`{pattern}` is not a domain; write it as `@example.com`");
        }
    } else {
        let mut halves = norm.split('@');
        let local = halves.next().unwrap_or_default();
        let domain = halves.next().unwrap_or_default();
        if local.is_empty() || domain.is_empty() || halves.next().is_some() || !domain.contains('.')
        {
            bail!(
                "`{pattern}` is neither an address nor a domain; a domain is written `@example.com`"
            );
        }
    }
    Ok(norm)
}

/// Adds a pattern. Adding one that is already there is not an error.
pub async fn add(context: &Context, pattern: &str, reason: &str) -> Result<()> {
    let norm = validate(pattern)?;
    context
        .sql
        .execute(
            "INSERT INTO blocklist (pattern, pattern_norm, added, reason)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(pattern_norm) DO NOTHING",
            (pattern.trim(), norm, time(), reason),
        )
        .await?;
    Ok(())
}

/// Removes a pattern, and unblocks any contact it was blocking.
///
/// Removing one that is not there is not an error.
///
/// The contact half matters as much as the row. [`block_contact`] deliberately
/// writes both, so that a blocked person is blocked in core's sense *and* has
/// their mail rejected; if removal undid only the blocklist row, the contact
/// would stay `blocked=1` -- still hidden from `get_contacts` and still
/// filtered out of `email::search` -- while their mail started arriving again.
/// That is a half-undone block, and it is the state this whole module is
/// arranged to make unrepresentable.
///
/// A domain pattern unblocks every contact in that domain, for the same
/// reason: those rows were blocked by *something*, and leaving them blocked
/// after the rule that justified them is gone is the same inconsistency.
pub async fn remove(context: &Context, pattern: &str) -> Result<()> {
    let norm = normalize(pattern);
    context
        .sql
        .execute("DELETE FROM blocklist WHERE pattern_norm=?", (&norm,))
        .await?;

    // Resolved before unblocking rather than in one statement: `Contact::unblock`
    // has to run per contact, because it also unblocks their 1:1 chat.
    let blocked_ids = Contact::get_all_blocked(context).await?;
    for contact_id in blocked_ids {
        let Ok(contact) = Contact::get_by_id(context, contact_id).await else {
            continue;
        };
        if pattern_covers(&norm, &normalize(contact.get_addr())) {
            Contact::unblock(context, contact_id).await?;
        }
    }
    Ok(())
}

/// Whether a normalised pattern describes a normalised address.
///
/// The single place the matching rule lives, so [`matches`] and [`remove`]
/// cannot drift apart -- two copies of one rule disagreeing is what
/// `gating::same_person` was extracted to stop.
fn pattern_covers(pattern_norm: &str, addr_norm: &str) -> bool {
    match pattern_norm.strip_prefix('@') {
        Some(domain) => addr_norm
            .split_once('@')
            .is_some_and(|(_, addr_domain)| addr_domain == domain),
        None => pattern_norm == addr_norm,
    }
}

/// Every entry, most recently added first.
pub async fn list(context: &Context) -> Result<Vec<Entry>> {
    context
        .sql
        .query_map(
            "SELECT id, pattern, added, reason FROM blocklist ORDER BY added DESC, id DESC",
            (),
            |row| {
                Ok(Entry {
                    id: row.get(0)?,
                    pattern: row.get(1)?,
                    added: row.get(2)?,
                    reason: row.get(3)?,
                })
            },
            |rows| rows.collect::<Result<Vec<_>, _>>().map_err(Into::into),
        )
        .await
}

/// Whether an address is blocked, by itself or by its domain.
///
/// Expresses the same rule as [`pattern_covers`], as two indexed equality
/// tests rather than a scan: this is consulted for every message that arrives,
/// and a `LIKE` over the whole table per message is a cost that grows with a
/// list the user only ever adds to. The two are kept honest by
/// `test_the_query_and_the_predicate_agree`, because a matching rule with two
/// implementations is a matching rule that will eventually have two answers.
pub async fn matches(context: &Context, addr: &str) -> Result<bool> {
    let addr = normalize(addr);
    let Some((_, domain)) = addr.split_once('@') else {
        // Not an address, so no pattern in this table can describe it.
        return Ok(false);
    };
    // Two exact lookups rather than a `LIKE` over the table: the blocklist is
    // consulted for every message that arrives, and an index-less scan per
    // message is a cost that grows with a list the user only ever adds to.
    let hit: Option<i64> = context
        .sql
        .query_row_optional(
            "SELECT id FROM blocklist WHERE pattern_norm=?1 OR pattern_norm=?2 LIMIT 1",
            (&addr, format!("@{domain}")),
            |row| row.get(0),
        )
        .await?;
    Ok(hit.is_some())
}

/// Blocks a person: the contact row *and* the blocklist, together.
///
/// One function rather than two calls, because the two halves do different
/// jobs and a caller that does one of them has a bug that looks like a working
/// feature. `Contact::block` is what stops the chat appearing and what
/// `email::search` already filters on; the blocklist entry is what actually
/// rejects the mail. Neither is sufficient alone.
///
/// This is the same shape as the `create_contact` / `release_held_contact`
/// pairing recorded in `docs/handoff.md`, and it is written as one call here
/// precisely so it cannot be half-done a third time.
pub async fn block_contact(context: &Context, contact_id: ContactId) -> Result<()> {
    let contact = Contact::get_by_id(context, contact_id).await?;
    let addr = contact.get_addr().to_string();
    Contact::block(context, contact_id).await?;
    if !addr.is_empty() {
        add(context, &addr, "").await?;
    }
    Ok(())
}

/// Unblocks a person: the contact row and the blocklist, together.
pub async fn unblock_contact(context: &Context, contact_id: ContactId) -> Result<()> {
    let contact = Contact::get_by_id(context, contact_id).await?;
    let addr = contact.get_addr().to_string();
    Contact::unblock(context, contact_id).await?;
    if !addr.is_empty() {
        remove(context, &addr).await?;
    }
    Ok(())
}

/// Trashes a just-received message if its sender is blocked.
///
/// Returns whether it did. Called from the best-effort block at the single
/// success exit of `receive_imf_inner`, after [`super::gating::apply`]: gating
/// decides whether a stranger's mail waits in `Unverified`, and this decides
/// whether it should be in the mailbox at all, so it has the last word. A
/// blocked sender who is also untrusted ends up in `Trash` rather than in
/// `Unverified`, which is what blocking them meant.
///
/// Incoming only. A blocked contact is somebody the user still might write to
/// -- blocking is about what arrives -- and trashing the user's own outgoing
/// mail because of who it is addressed to would be a surprise with no undo.
pub async fn apply(context: &Context, msg_id: MsgId) -> Result<bool> {
    let row: Option<(ContactId, ContactId)> = context
        .sql
        .query_row_optional(
            "SELECT from_id, to_id FROM msgs WHERE id=?",
            (msg_id,),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .await?;
    let Some((from_id, to_id)) = row else {
        return Ok(false);
    };
    // Outgoing is `to_id != SELF`; the same test `gating::sender_of` makes.
    if to_id != ContactId::SELF || from_id == ContactId::SELF {
        return Ok(false);
    }
    let Ok(contact) = Contact::get_by_id(context, from_id).await else {
        return Ok(false);
    };
    if !matches(context, contact.get_addr()).await? {
        return Ok(false);
    }
    // Not synced. Every device runs the same blocklist and reaches the same
    // conclusion about the same message, so pushing the decision would only
    // race it -- the same reasoning as the unverified sweep.
    ephemeral::to_trash(context, &[msg_id], Reason::Blocked, time()).await?;
    Ok(true)
}

#[cfg(test)]
mod blocklist_tests;
