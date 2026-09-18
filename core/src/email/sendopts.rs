//! Choices the composer makes about one outgoing message.
//!
//! Two of them, both decided while writing and both needed long after the
//! composer has handed the message over:
//!
//! * **Encryption.** The composer's padlock. [`EncryptionChoice::Auto`] is
//!   what every message did before the padlock existed; the other two are the
//!   user overriding it for this message, within limits the policy sets.
//! * **Whether the signature is already in the body.** The composer shows the
//!   signature where the user can see and edit it, as other clients do, and
//!   the renderer must then not append a second copy.
//!
//! # A signature in the body is split off it
//!
//! Upstream escapes every `-- ` line in a message's text to `-\u{200B}- `
//! (`simplify::escape_message_footer_marks`), so that Delta Chat, which strips
//! everything after a footer mark, does not eat text the user wrote. That is
//! right for a line in the middle of a message and wrong for the one line that
//! *is* the separator: a signature left in the body would reach every
//! recipient with a zero-width space in its separator, which no client
//! recognises. So [`super::compose::send`] splits the text at its last
//! separator, sends what came before as the body, and stores what came after
//! here; [`super::signature::load_for`] then hands it to the renderer as this
//! message's footer, which goes out after a real `-- `. The HTML part is left
//! alone -- nothing escapes it, and it already carries the signature the user
//! saw.
//!
//! # Why not `msgs.param`
//!
//! `ForcePlaintext` and `GuaranteeE2ee` are params, and would be the obvious
//! place to say "plaintext" and "encrypted". But [`crate::chat::send_msg`]
//! strips both from any message that is not brand new, and
//! [`super::compose::send`] persists every message as a draft first -- that is
//! what gives it an id to hang the recipient set on. A param set at compose
//! time would be erased before anything read it. So the choice is stored
//! here, keyed by `msgs.id`, and [`super::policy::prepare_send`] turns it into
//! the param at the point core reads it.
//!
//! # Absent when default
//!
//! As with [`super::importance`]: the default stores no row, so a message sent
//! without these choices is handled exactly as before. `msgs.id` is
//! `AUTOINCREMENT` and never reused, so a row cannot outlive its message into
//! someone else's.
//!
//! See [ADR 0032](../../../docs/adr/0032-composer-send-options.md).

use anyhow::{Result, bail};
use deltachat_contact_tools::addr_normalize;

use crate::contact::ContactId;
use crate::context::Context;
use crate::message::MsgId;

use super::compose::RecipientSet;
use super::policy::EncryptionMode;

/// What the composer's padlock says about one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncryptionChoice {
    /// Whatever the encryption policy decides. What every message did before
    /// the padlock existed, and what a caller that does not ask gets.
    #[default]
    Auto = 0,
    /// End-to-end or not at all. Refused if any recipient has no key, rather
    /// than sent encrypted to some and dropped for the rest.
    Required = 1,
    /// Cleartext, even to recipients whose key we hold. Refused where the
    /// policy says end-to-end only: a padlock cannot outvote a setting the
    /// user made about a correspondent.
    Plaintext = 2,
}

impl EncryptionChoice {
    fn from_i64(value: i64) -> Self {
        match value {
            1 => EncryptionChoice::Required,
            2 => EncryptionChoice::Plaintext,
            _ => EncryptionChoice::Auto,
        }
    }
}

/// The composer's choices for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SendOptions {
    /// The padlock.
    pub encryption: EncryptionChoice,
    /// The composer decided the signature: whatever follows the body's last
    /// `-- ` line is it, and a body with no such line is unsigned because the
    /// user removed it. Either way the configured signature is not appended.
    pub signature_in_body: bool,
}

/// Records a message's options, and the signature [`split_signature`] took off
/// its body. The default removes the row rather than storing it.
pub async fn set(
    context: &Context,
    msg_id: MsgId,
    options: &SendOptions,
    signature: Option<&str>,
) -> Result<()> {
    if *options == SendOptions::default() {
        context
            .sql
            .execute("DELETE FROM msg_send_options WHERE msg_id=?", (msg_id,))
            .await?;
        return Ok(());
    }
    context
        .sql
        .execute(
            "INSERT INTO msg_send_options (msg_id, encryption, signature_in_body, signature)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(msg_id) DO UPDATE SET
               encryption=excluded.encryption,
               signature_in_body=excluded.signature_in_body,
               signature=excluded.signature",
            (
                msg_id,
                options.encryption as i64,
                options.signature_in_body,
                signature.filter(|_| options.signature_in_body),
            ),
        )
        .await?;
    Ok(())
}

/// A message's options. No row means [`SendOptions::default`].
pub async fn load(context: &Context, msg_id: MsgId) -> Result<SendOptions> {
    let row: Option<(i64, bool)> = context
        .sql
        .query_row_optional(
            "SELECT encryption, signature_in_body FROM msg_send_options WHERE msg_id=?",
            (msg_id,),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .await?;
    Ok(
        row.map_or_else(SendOptions::default, |(encryption, signature_in_body)| {
            SendOptions {
                encryption: EncryptionChoice::from_i64(encryption),
                signature_in_body,
            }
        }),
    )
}

/// The signature split off a message's body, if the composer placed one.
pub async fn signature(context: &Context, msg_id: MsgId) -> Result<Option<String>> {
    Ok(context
        .sql
        .query_row_optional(
            "SELECT signature FROM msg_send_options WHERE msg_id=?",
            (msg_id,),
            |row| row.get::<_, Option<String>>(0),
        )
        .await?
        .flatten())
}

/// Splits a plain body at its last signature separator: the text above it,
/// and the signature below it if there is anything there.
///
/// The separator is a line of `--` and trailing whitespace, which is `-- `
/// as the composer writes it and as RFC 3676 spells it. A body with none is
/// returned whole.
pub fn split_signature(text: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(at) = lines.iter().rposition(|line| line.trim_end() == "--") else {
        return (text.to_string(), None);
    };
    let (above, below) = lines.split_at_checked(at).unwrap_or_default();
    let body = above.join("\n");
    // `below` starts with the separator line itself.
    let signature = below.get(1..).unwrap_or_default().join("\n");
    let signature = signature.trim_end();
    (
        body.trim_end().to_string(),
        (!signature.trim().is_empty()).then(|| signature.to_string()),
    )
}

/// What the padlock should say for a recipient set, before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    /// The strictest mode among the global setting and every recipient's
    /// override.
    pub mode: EncryptionMode,
    /// Addresses we hold no key for, as typed.
    pub missing: Vec<String>,
    /// The padlock may not be opened: the policy says end-to-end only.
    pub locked: bool,
}

/// Answers the padlock's question for `recipients` without changing anything.
///
/// **Creates no contacts.** It is asked on every keystroke in an address
/// field, and a contact per half-typed address would fill the address book
/// with fragments. So addresses are only looked up, and an address we have
/// never seen is simply one we hold no key for.
pub async fn readiness(context: &Context, recipients: &RecipientSet) -> Result<Readiness> {
    let mut ids = Vec::new();
    let mut missing = Vec::new();
    for typed in recipients
        .to
        .iter()
        .chain(&recipients.cc)
        .chain(&recipients.bcc)
    {
        let typed = typed.trim();
        if typed.is_empty() {
            continue;
        }
        let addr = addr_normalize(&super::compose::split_addr(typed).1);
        ids.extend(contacts_for(context, &addr).await?);
        if super::compose::key_contact_for(context, &addr)
            .await?
            .is_none()
        {
            missing.push(typed.to_string());
        }
    }
    let mode = EncryptionMode::effective(context, &ids).await?;
    Ok(Readiness {
        mode,
        missing,
        locked: mode == EncryptionMode::Strict,
    })
}

/// Refuses options the policy does not allow for `recipients`.
///
/// Called by [`super::compose::send`] before anything is stored, so a refused
/// message leaves no draft behind and the error reaches the composer rather
/// than a failed row in Sent.
pub(crate) async fn check(
    context: &Context,
    recipients: &RecipientSet,
    options: &SendOptions,
) -> Result<()> {
    if options.encryption == EncryptionChoice::Auto {
        return Ok(());
    }
    let ready = readiness(context, recipients).await?;
    match options.encryption {
        EncryptionChoice::Required if !ready.missing.is_empty() => bail!(
            "cannot send encrypted: no key for {}",
            ready.missing.join(", ")
        ),
        EncryptionChoice::Plaintext if ready.locked => bail!(
            "cannot send unencrypted: encryption is set to end-to-end only for this message's \
             recipients"
        ),
        _ => Ok(()),
    }
}

/// Every contact row for an address.
///
/// All of them rather than one, because a correspondent can have an
/// address-contact and a key-contact, and an end-to-end-only override set on
/// either must hold.
async fn contacts_for(context: &Context, addr: &str) -> Result<Vec<ContactId>> {
    context
        .sql
        .query_map_vec(
            "SELECT id FROM contacts WHERE addr=?1 COLLATE NOCASE AND id>?2",
            (addr, ContactId::LAST_SPECIAL),
            |row| {
                let id: ContactId = row.get(0)?;
                Ok(id)
            },
        )
        .await
}

#[cfg(test)]
mod sendopts_tests;
