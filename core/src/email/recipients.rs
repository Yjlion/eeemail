//! Per-message recipient sets.
//!
//! In core a message belongs to a *chat*, and who receives it follows from the
//! chat's membership. In email a message is addressed to a *recipient set* that
//! it carries itself: two messages in one conversation routinely have different
//! To and Cc lists, and reply-all only works if we remember what each individual
//! message was addressed to.
//!
//! So we keep the recipient set as a first-class per-message property, in
//! `msg_recipients`, rather than deriving it from chat membership.
//!
//! # What is stored, and from where
//!
//! On receive, To and Cc come from [`HeaderRecipients`], which is filled by
//! `MimeMessage::merge_headers`. Taking them there rather than re-parsing the
//! raw bytes is what makes them correct for encrypted mail: RFC 9788 protected
//! headers live inside the encrypted part, and the outer ones may be absent or
//! deliberately misleading. `merge_headers` has already resolved that.
//!
//! Bcc is never read from an incoming message. A Bcc header on a received
//! message either does not exist (the point of Bcc) or was added by an
//! intermediary, in which case it is at best a guess and at worst a way to make
//! a recipient believe someone else was blind-copied. Bcc is written only by
//! the send path, from what we ourselves addressed.
//!
//! Order within each header is preserved. It has no protocol meaning, but a
//! reply-all that reorders everyone's correspondents looks broken.

use anyhow::Result;
use mailparse::{MailHeader, SingleInfo};

use crate::context::Context;
use crate::message::MsgId;

/// Which address header a recipient appeared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i64)]
pub enum RecipientKind {
    /// `To:` -- addressed directly.
    To = 0,
    /// `Cc:` -- copied, visibly to everyone.
    Cc = 1,
    /// `Bcc:` -- copied, invisibly to the other recipients.
    ///
    /// Only ever set on messages we sent.
    Bcc = 2,
}

impl RecipientKind {
    fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(RecipientKind::To),
            1 => Some(RecipientKind::Cc),
            2 => Some(RecipientKind::Bcc),
            _ => None,
        }
    }
}

/// One addressee of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    /// The header this address appeared in.
    pub kind: RecipientKind,

    /// Normalized, lowercased address.
    pub addr: String,

    /// Display name as it was sent, or empty if the address was bare.
    pub name: String,
}

impl Recipient {
    /// Builds a recipient from a parsed address.
    pub fn new(kind: RecipientKind, addr: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind,
            addr: addr.into(),
            name: name.into(),
        }
    }

    fn from_single_info(kind: RecipientKind, info: &SingleInfo) -> Self {
        Self::new(
            kind,
            info.addr.clone(),
            info.display_name.clone().unwrap_or_default(),
        )
    }
}

/// `To` and `Cc` as they were parsed from a message's headers.
///
/// Upstream's `MimeMessage::recipients` concatenates the two, because for group
/// membership the distinction does not matter. It does matter to us, so we
/// collect them separately alongside it.
#[derive(Debug, Default)]
pub(crate) struct HeaderRecipients {
    /// Addresses from `To:`, in header order.
    pub to: Vec<SingleInfo>,

    /// Addresses from `Cc:`, in header order.
    pub cc: Vec<SingleInfo>,
}

impl HeaderRecipients {
    /// Applies the `To`/`Cc` headers of one MIME part.
    ///
    /// `has_header_protection` and the emptiness test mirror how upstream
    /// merges `recipients`: an inner protected part always wins, and an
    /// unprotected part only wins if it says something. To and Cc are replaced
    /// together as a unit for exactly that reason -- a part carrying only `To:`
    /// must not leave a `Cc:` from a different part standing beside it.
    pub(crate) fn merge(&mut self, fields: &[MailHeader], has_header_protection: bool) {
        let to = crate::mimeparser::get_all_addresses_from_header(fields, "to");
        let cc = crate::mimeparser::get_all_addresses_from_header(fields, "cc");
        if has_header_protection || !to.is_empty() || !cc.is_empty() {
            self.to = to;
            self.cc = cc;
        }
    }

    /// Converts to storable recipients, To first then Cc.
    pub(crate) fn to_recipients(&self) -> Vec<Recipient> {
        self.to
            .iter()
            .map(|i| Recipient::from_single_info(RecipientKind::To, i))
            .chain(
                self.cc
                    .iter()
                    .map(|i| Recipient::from_single_info(RecipientKind::Cc, i)),
            )
            .collect()
    }
}

/// Replaces the stored recipient set of `msg_id`.
///
/// Duplicates within a kind are collapsed, keeping the first occurrence and so
/// the first display name -- addresses repeat in practice, and the name on the
/// first mention is the one the sender typed.
pub async fn store(context: &Context, msg_id: MsgId, recipients: &[Recipient]) -> Result<()> {
    let recipients = recipients.to_vec();
    context
        .sql
        .transaction(move |transaction| {
            transaction.execute("DELETE FROM msg_recipients WHERE msg_id=?", (msg_id,))?;
            let mut stmt = transaction.prepare(
                "INSERT INTO msg_recipients (msg_id, kind, addr, name, ord)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(msg_id, kind, addr) DO NOTHING",
            )?;
            for (ord, recipient) in recipients.iter().enumerate() {
                stmt.execute((
                    msg_id,
                    recipient.kind as i64,
                    &recipient.addr,
                    &recipient.name,
                    i64::try_from(ord).unwrap_or(i64::MAX),
                ))?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Loads the full recipient set of `msg_id`, To then Cc then Bcc, each in
/// header order.
pub async fn load(context: &Context, msg_id: MsgId) -> Result<Vec<Recipient>> {
    context
        .sql
        .query_map(
            "SELECT kind, addr, name FROM msg_recipients
             WHERE msg_id=? ORDER BY kind, ord",
            (msg_id,),
            |row| {
                let kind: i64 = row.get(0)?;
                let addr: String = row.get(1)?;
                let name: String = row.get(2)?;
                Ok((kind, addr, name))
            },
            |rows| {
                let mut recipients = Vec::new();
                for row in rows {
                    let (kind, addr, name) = row?;
                    // A kind we cannot decode came from a newer schema; skip it
                    // rather than fail the whole load.
                    if let Some(kind) = RecipientKind::from_i64(kind) {
                        recipients.push(Recipient { kind, addr, name });
                    }
                }
                Ok(recipients)
            },
        )
        .await
}

/// Loads only the recipients of one kind, in header order.
pub async fn load_kind(
    context: &Context,
    msg_id: MsgId,
    kind: RecipientKind,
) -> Result<Vec<Recipient>> {
    Ok(load(context, msg_id)
        .await?
        .into_iter()
        .filter(|r| r.kind == kind)
        .collect())
}

/// Drops the recipient set of `msg_id`.
pub async fn delete(context: &Context, msg_id: MsgId) -> Result<()> {
    context
        .sql
        .execute("DELETE FROM msg_recipients WHERE msg_id=?", (msg_id,))
        .await?;
    Ok(())
}

/// Removes recipient sets of messages that no longer exist.
///
/// Called from housekeeping, mirroring how `msgs_mdns` and
/// `msgs_status_updates` are pruned. Trashed messages keep a tombstone row in
/// `msgs`, but their content is gone, so they count as deleted here too.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM msg_recipients WHERE msg_id NOT IN \
             (SELECT id FROM msgs WHERE chat_id!=?)",
            (crate::constants::DC_CHAT_ID_TRASH,),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod recipients_tests;
