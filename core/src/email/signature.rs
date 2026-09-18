//! The signature appended to outgoing mail.
//!
//! Upstream already renders a footer: [`Config::Selfstatus`] goes out after an
//! RFC 3676 `-- ` separator on every message, and has since long before this
//! fork. eeemail does not reuse it. `Selfstatus` is also a *status* -- it is
//! shown on a contact's profile, and an incoming one is stored on the sender's
//! contact row ([`crate::contact::Contact::get_status`]) -- so a five-line
//! signature with an employer and a phone number in it would end up in the
//! place a one-line status belongs, on every correspondent's screen. An email
//! client has signatures; a messenger has statuses; they are not one field.
//!
//! # Two configs, one signature
//!
//! [`Config::EmailSignature`] is the signature, as plain text, and is the only
//! one that has to be set. [`Config::EmailSignatureHtml`] is optional markup
//! for the `text/html` alternative; when it is absent the plain signature is
//! escaped into a `<pre>` so that a signature written once appears in both
//! parts. That matters because the plain part is never optional
//! ([ADR 0025](../../../docs/adr/0025-composed-html.md)) but is also not the
//! part most recipients will be shown -- a signature that appeared in only one
//! of them would be missing exactly where the user was looking.
//!
//! # Where this is not applied
//!
//! Nowhere in `email::policy::apply_defaults`. There is no sensible default
//! signature: an empty one is what an account that has never been told
//! otherwise should have, and writing anything else would be inventing content
//! on the user's behalf rather than choosing a policy for them.

use anyhow::Result;

use crate::config::Config;
use crate::context::Context;
use crate::message::MsgId;

/// A signature, in the two forms a message can carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The plain-text form. Never empty -- a [`Signature`] with nothing to say
    /// is not constructed.
    pub plain: String,
    /// The markup for the HTML alternative, derived when none was configured.
    /// Empty when the HTML part already carries the signature.
    pub html: String,
}

/// Reads the configured signature, or `None` when there is nothing to append.
///
/// Whitespace-only counts as nothing: a signature box someone cleared by
/// selecting the text and pressing space is empty, and appending it would put a
/// bare `-- ` separator on every message with nothing after it.
pub async fn load(context: &Context) -> Result<Option<Signature>> {
    let plain = context
        .get_config(Config::EmailSignature)
        .await?
        .unwrap_or_default();
    let plain = plain.trim_end().to_string();
    if plain.trim().is_empty() {
        return Ok(None);
    }
    let configured_html = context
        .get_config(Config::EmailSignatureHtml)
        .await?
        .unwrap_or_default();
    let html = if configured_html.trim().is_empty() {
        default_html(&plain)
    } else {
        configured_html
    };
    Ok(Some(Signature { plain, html }))
}

/// The signature to append to one message, or `None` when there is nothing
/// to append.
///
/// When the composer put the signature in the body, where the user could see
/// and edit it, this is *that* signature -- as edited, split off the body by
/// `compose::send` -- or `None` if the user removed it. Its `html` is empty,
/// because the HTML part already carries it and is not escaped, so
/// [`append_to_html`] leaves that part alone. Appending the configured one
/// instead would sign the message twice and undo the user's edit.
/// See [ADR 0032](../../../docs/adr/0032-composer-send-options.md).
pub async fn load_for(context: &Context, msg_id: MsgId) -> Result<Option<Signature>> {
    if super::sendopts::load(context, msg_id)
        .await?
        .signature_in_body
    {
        return Ok(super::sendopts::signature(context, msg_id)
            .await?
            .map(|plain| Signature {
                plain,
                html: String::new(),
            }));
    }
    load(context).await
}

/// The HTML form of a plain-text signature.
///
/// `<pre>` rather than `<br>`-joined text because a signature is aligned: the
/// address block and the phone number under it are laid out with spaces, and a
/// proportional font with collapsed whitespace destroys that. Escaped, because
/// the value is plain text the user typed -- an `&` in a company name is an
/// ampersand, and a `<` is a less-than sign, not the start of a tag.
fn default_html(plain: &str) -> String {
    format!(
        "<pre class=\"signature\">{}</pre>",
        plain
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    )
}

/// Appends the signature to an HTML body.
///
/// Before `</body>` when there is one, and at the end otherwise. Composed HTML
/// reaches here as a fragment (`richtext.ts` produces no document), while
/// forwarded HTML can be a whole document -- appending after `</body>` there
/// puts the signature outside the body, where a strict renderer may drop it.
pub fn append_to_html(html: &str, signature: &Signature) -> String {
    if signature.html.is_empty() {
        return html.to_string();
    }
    let separator = "<div class=\"signature-sep\">--</div>";
    let block = format!("{separator}{}", signature.html);
    match html.rfind("</body>") {
        // `rfind` reports the byte offset a match starts at, which is always a
        // char boundary, so this cannot split a multi-byte character.
        Some(at) => {
            let (before, rest) = html.split_at(at);
            format!("{before}{block}{rest}")
        }
        None => format!("{html}{block}"),
    }
}

#[cfg(test)]
mod signature_tests;
