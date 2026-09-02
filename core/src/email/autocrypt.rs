//! Learning a correspondent's key from the header they advertised it in.
//!
//! Core decides encryption by the *kind* of contact a message is addressed to,
//! not by whether a key is on hand: a `Single` chat is encrypted exactly when
//! its contact row carries a fingerprint ([`crate::chat::Chat::is_encrypted`]).
//! Upstream mints such a contact from a signed message or from SecureJoin, and
//! from nothing else — Autocrypt peerstates were removed.
//!
//! That leaves an email client unable to start. Two correspondents write to
//! each other in cleartext, each advertising a key in an `Autocrypt:` header,
//! each importing the other's key and attaching it to nobody; neither can send
//! a first encrypted message, so neither ever sends a signed one, so neither
//! ever becomes a key-contact. [ADR 0006]'s opportunistic default is
//! unreachable and the mail stays in plaintext forever.
//!
//! So eeemail attaches the key. [`adopt`] turns the fingerprint core has
//! already imported into a key-contact, which is the rung the rest of the
//! engine is built on.
//!
//! # What this key is worth
//!
//! Less than a verified one, and the difference is the whole point. An
//! `Autocrypt:` header is unauthenticated — anyone who can write the `From`
//! line can write it — so this defends against someone reading stored mail, and
//! not against someone rewriting mail in flight. That is Autocrypt's own threat
//! model, stated in its spec.
//!
//! Nothing here creates a *verified* contact. Verification remains SecureJoin,
//! in person, and the reading pane keeps showing "encrypted" and "verified" as
//! two separate claims. See [ADR 0021].
//!
//! [ADR 0006]: ../../../docs/adr/0006-encryption-policy.md
//! [ADR 0021]: ../../../docs/adr/0021-autocrypt-key-contacts.md

use anyhow::Result;

use crate::contact::{Contact, ContactId, Origin};
use crate::context::Context;
use crate::mimeparser::MimeMessage;

/// Creates a key-contact for the sender's advertised key, if there is one.
///
/// Returns the contact when one was created or found, and `None` when the
/// message gave us nothing to work with. Called from the receive path and
/// best-effort like everything there: a message that has already arrived must
/// not be lost because we could not learn a key from it.
pub(crate) async fn adopt(context: &Context, mime: &MimeMessage) -> Result<Option<ContactId>> {
    // A signature gives core a fingerprint it can check against the message
    // itself, which is better evidence than a header claiming to speak for the
    // sender. Where core has that, it has already made the contact.
    if mime.signature.is_some() {
        return Ok(None);
    }
    let Some(fingerprint) = mime.autocrypt_fingerprint.as_deref() else {
        return Ok(None);
    };
    if fingerprint.is_empty() {
        return Ok(None);
    }

    let addr = mime.from.addr.trim();
    if addr.is_empty() || context.is_self_addr(addr).await? {
        return Ok(None);
    }

    // `IncomingUnknownFrom` deliberately: holding someone's key is not the same
    // as choosing to know them, and `gating::is_trusted` reads exactly this
    // origin. Learning a key must not open the inbox to a stranger.
    let (contact_id, _modified) = Contact::add_or_lookup_ext(
        context,
        mime.from.display_name.as_deref().unwrap_or_default(),
        addr,
        fingerprint,
        Origin::IncomingUnknownFrom,
    )
    .await?;

    info!(
        context,
        "Adopted an Autocrypt key for {addr} as contact {contact_id}."
    );
    Ok(Some(contact_id))
}

#[cfg(test)]
mod autocrypt_tests;
