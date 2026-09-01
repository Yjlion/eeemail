//! Addressing a message to a recipient set.
//!
//! This closes the gap that Phases 2, 4 and 7 each deferred. In core, who
//! receives a message follows entirely from chat membership: `MimeFactory`
//! derives the `To` header, the SMTP envelope and the encryption key set from
//! `chats_contacts`, and emits no `Cc` header at all. A composer with To/Cc/Bcc
//! fields built on that would have fields the engine ignores.
//!
//! So a message may carry **extra recipients** of its own, stored in
//! `msg_recipients` before it is sent. `MimeFactory` reads them and adds them
//! to the header, the envelope and the key set.
//!
//! # Cc and Bcc recipients are contacts
//!
//! [`set_recipients`] resolves every address to a `ContactId`, creating one if
//! needed. That is not bookkeeping: it is what makes "do we have a key for this
//! person?" the same question it already is for a chat member, so the
//! encryption policy in [`crate::email::policy`] applies unchanged rather than
//! growing a second, subtly different code path for Cc.
//!
//! # What happens when a Cc recipient has no key
//!
//! Exactly what happens for a chat member without one, which is the point of
//! routing them through the same machinery:
//!
//! * **Strict** refuses to send.
//! * **Lenient** sends the whole message unencrypted rather than dropping them.
//! * **Opportunistic** encrypts to those who have keys, and the send path
//!   records the rest in `msg_undelivered` so the client can say who never
//!   received it.
//!
//! # Bcc
//!
//! Bcc addresses go into the SMTP envelope and the encryption key set, and into
//! **no header at all**. That is the whole meaning of Bcc, and getting it wrong
//! is a disclosure, so it is asserted directly.
//!
//! See `docs/adr/0014-recipient-sets-on-the-wire.md`.

use anyhow::Result;
use deltachat_contact_tools::{ContactAddress, addr_normalize};

use crate::contact::{Contact, Origin};
use crate::context::Context;
use crate::key::{DcKey, SignedPublicKey};
use crate::message::MsgId;

use super::recipients::{Recipient, RecipientKind};

/// Who a message is addressed to.
#[derive(Debug, Default, Clone)]
pub struct RecipientSet {
    /// `To:` addresses.
    pub to: Vec<String>,
    /// `Cc:` addresses.
    pub cc: Vec<String>,
    /// `Bcc:` addresses. Never written to a header.
    pub bcc: Vec<String>,
}

impl RecipientSet {
    /// True if nothing is addressed.
    pub fn is_empty(&self) -> bool {
        self.to.is_empty() && self.cc.is_empty() && self.bcc.is_empty()
    }
}

/// Records the recipient set of a draft, resolving each address to a contact.
///
/// Call before sending. Addresses that already appear in the message's chat are
/// still recorded, so the stored set is a faithful record of what the user
/// typed; [`extra_recipients`] is what filters them at send time.
pub async fn set_recipients(
    context: &Context,
    msg_id: MsgId,
    recipients: &RecipientSet,
) -> Result<()> {
    let mut rows = Vec::new();
    for (kind, addrs) in [
        (RecipientKind::To, &recipients.to),
        (RecipientKind::Cc, &recipients.cc),
        (RecipientKind::Bcc, &recipients.bcc),
    ] {
        for addr in addrs {
            let addr = addr.trim();
            if addr.is_empty() {
                continue;
            }
            let (name, addr) = split_addr(addr);
            // Creating the contact is what makes key lookup for a Cc identical
            // to key lookup for a chat member.
            let contact_addr = ContactAddress::new(&addr)?;
            Contact::add_or_lookup(context, &name, &contact_addr, Origin::OutgoingCc).await?;
            rows.push(Recipient::new(kind, addr_normalize(&addr), name));
        }
    }
    super::recipients::store(context, msg_id, &rows).await
}

/// Splits `Name <addr@example.org>` into its parts. A bare address has no name.
///
/// Uses `rsplit_once` rather than byte indices: the crate forbids
/// `clippy::string_slice`, and a display name can contain anything, including
/// multi-byte characters that a byte index could land inside.
fn split_addr(input: &str) -> (String, String) {
    if let Some((name, rest)) = input.rsplit_once('<')
        && let Some((addr, _)) = rest.rsplit_once('>')
    {
        return (
            name.trim().trim_matches('"').to_string(),
            addr.trim().to_string(),
        );
    }
    // Malformed: return it whole rather than guess, so a bad address fails
    // visibly instead of being delivered somewhere unintended.
    (String::new(), input.to_string())
}

/// One addressee that is not a member of the message's chat.
#[derive(Debug, Clone)]
pub(crate) struct ExtraRecipient {
    /// Display name, empty if the address was bare.
    pub name: String,
    /// Normalized address.
    pub addr: String,
    /// Their public key, if we hold one.
    pub key: Option<SignedPublicKey>,
}

/// Addressees of a message that its chat does not already cover.
///
/// `already` is the set of addresses `MimeFactory` has handled from chat
/// membership; anything else the user addressed is returned here so it can be
/// added to the header, the envelope and the key set.
#[derive(Debug, Default)]
pub(crate) struct ExtraRecipients {
    /// Extra `Cc:` addressees.
    pub cc: Vec<ExtraRecipient>,
    /// Extra `Bcc:` addressees. Envelope and keys only, never a header.
    pub bcc: Vec<ExtraRecipient>,
}

pub(crate) async fn extra_recipients(
    context: &Context,
    msg_id: MsgId,
    already: &[String],
) -> Result<ExtraRecipients> {
    let stored = super::recipients::load(context, msg_id).await?;
    if stored.is_empty() {
        return Ok(ExtraRecipients::default());
    }
    let already: std::collections::HashSet<String> =
        already.iter().map(|a| addr_normalize(a)).collect();

    let mut extra = ExtraRecipients::default();
    let mut seen = std::collections::HashSet::new();
    for recipient in stored {
        // `To` is chat membership's job; we only add what it cannot express.
        if recipient.kind == RecipientKind::To {
            continue;
        }
        let addr = addr_normalize(&recipient.addr);
        if already.contains(&addr) || !seen.insert(addr.clone()) {
            continue;
        }
        let key = lookup_key(context, &addr).await?;
        let entry = ExtraRecipient {
            name: recipient.name,
            addr,
            key,
        };
        match recipient.kind {
            RecipientKind::Cc => extra.cc.push(entry),
            RecipientKind::Bcc => extra.bcc.push(entry),
            RecipientKind::To => unreachable!("filtered above"),
        }
    }
    Ok(extra)
}

/// The public key we hold for an address, if any.
async fn lookup_key(context: &Context, addr: &str) -> Result<Option<SignedPublicKey>> {
    let bytes: Option<Vec<u8>> = context
        .sql
        .query_row_optional(
            "SELECT k.public_key FROM contacts c
             LEFT JOIN public_keys k ON k.fingerprint=c.fingerprint
             WHERE c.addr=? AND c.fingerprint IS NOT NULL AND c.fingerprint!=''
             LIMIT 1",
            (addr,),
            |row| row.get::<_, Option<Vec<u8>>>(0),
        )
        .await?
        .flatten();
    match bytes {
        Some(bytes) => Ok(Some(SignedPublicKey::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod compose_tests;
