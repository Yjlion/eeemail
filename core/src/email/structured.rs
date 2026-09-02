//! Machine-readable data a message carries about itself.
//!
//! [Structured email] is a second representation of a message meant for the
//! client rather than the reader: a parcel as a delivery, a booking as an
//! itinerary. The IETF's [SML] working group carries it in an
//! `application/ld+json` part marked `Content-Purpose: Machine-readable`, in
//! one of three arrangements that say how it relates to the human-readable
//! body. Senders deployed today mostly do none of that and instead put a
//! `<script type="application/ld+json">` inside the HTML body, which is what
//! Gmail reads, so both are parsed.
//!
//! # Parsing is unconditional; acting is not
//!
//! Structured data exists to drive **affordances**, and an affordance rendered
//! from attacker-controlled data is a phishing primitive with better typography
//! than the attacker could manage alone. So every message is parsed — refusing
//! would hide what a message claims about itself, which is information a user
//! may want precisely because they distrust the sender — and each object is
//! stored with a `trusted` verdict that decides how it may be rendered.
//!
//! # The verdict
//!
//! ```text
//! trusted = gating::is_trusted(sender) && encrypted && signed
//! ```
//!
//! Both halves are needed and neither is sufficient. Gating says *the user
//! chose to engage with this person* — the same rule that decides whether their
//! mail reaches the inbox at all, which is what [ADR 0016] means by using the
//! rule the client already has rather than inventing a second one. Encryption
//! with a good signature says *this message really is from them*: cleartext is
//! trivially `From`-spoofed, and core checks no DKIM.
//!
//! Full SecureJoin verification is deliberately **not** required. Requiring it
//! would make the trusted branch dead code — no parcel or booking sender will
//! ever scan a QR code — and a rule that never fires protects nobody.
//!
//! Note what [ADR 0021] costs here: a signature now only proves continuity with
//! a key that may itself have been learned from an unauthenticated header. The
//! verdict is therefore "the user engaged with this correspondent, and this
//! message is cryptographically the same correspondent", not "this sender is
//! who they say they are". It is why nothing in this phase renders an
//! affordance that touches the network.
//!
//! # Never at the cost of the message
//!
//! Malformed JSON is dropped with a log line. An enhancement must not be able
//! to lose a message — the same rule raw MIME retention follows.
//!
//! [Structured email]: https://structured.email/
//! [SML]: https://datatracker.ietf.org/wg/sml/about/
//! [ADR 0016]: ../../../docs/adr/0016-structured-email.md
//! [ADR 0021]: ../../../docs/adr/0021-autocrypt-key-contacts.md

use anyhow::Result;
use mailparse::{MailHeaderMap, ParsedMail};

use crate::context::Context;
use crate::message::MsgId;
use crate::mimeparser::MimeMessage;

/// Where an object came from, and so what it claims to represent.
///
/// The three multipart arrangements are SML's way of saying whether the
/// structured data is a full representation of the human-readable body, a
/// partial one, or unrelated. It cannot be recovered after receive, so it is
/// stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `multipart/alternative`: a full representation of the body.
    Alternative,
    /// `multipart/related`: a partial representation.
    Related,
    /// `multipart/mixed`, or anything else: neither.
    Mixed,
    /// A `<script type="application/ld+json">` in the HTML body.
    HtmlScript,
}

impl Source {
    fn to_i64(self) -> i64 {
        match self {
            Source::Alternative => 0,
            Source::Related => 1,
            Source::Mixed => 2,
            Source::HtmlScript => 3,
        }
    }

    fn from_i64(value: i64) -> Self {
        match value {
            0 => Source::Alternative,
            1 => Source::Related,
            3 => Source::HtmlScript,
            _ => Source::Mixed,
        }
    }

    fn from_multipart(subtype: &str) -> Self {
        match subtype {
            "alternative" => Source::Alternative,
            "related" => Source::Related,
            _ => Source::Mixed,
        }
    }
}

/// One machine-readable object extracted from a message.
#[derive(Debug, Clone)]
pub struct StructuredObject {
    /// Order within the message.
    pub seq: u32,
    /// The object exactly as it arrived.
    pub json: String,
    /// Whether it may drive an affordance. See the module docs.
    pub trusted: bool,
    /// Where it came from.
    pub source: Source,
}

/// Extracts and stores a message's structured data.
///
/// Called from the receive path, best-effort like everything there.
pub(crate) async fn store(
    context: &Context,
    msg_id: MsgId,
    mime: &MimeMessage,
    imf_raw: &[u8],
) -> Result<()> {
    let found = extract(mime, imf_raw);
    if found.is_empty() {
        return Ok(());
    }
    let trusted = is_trusted(context, msg_id).await?;

    let rows: Vec<(u32, String, Source)> = found;
    context
        .sql
        .transaction(move |transaction| {
            for (seq, json, source) in &rows {
                transaction.execute(
                    "INSERT INTO structured_data (msg_id, seq, json, trusted, source)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(msg_id, seq) DO NOTHING",
                    (msg_id, seq, json, trusted as i64, source.to_i64()),
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Whether this message's structured data may be acted on.
async fn is_trusted(context: &Context, msg_id: MsgId) -> Result<bool> {
    let crypto = super::policy::message_crypto(context, msg_id).await?;
    if !crypto.encrypted || !crypto.signed {
        return Ok(false);
    }
    let from_id: Option<crate::contact::ContactId> = context
        .sql
        .query_row_optional("SELECT from_id FROM msgs WHERE id=?", (msg_id,), |row| {
            Ok(row.get(0)?)
        })
        .await?;
    match from_id {
        Some(from_id) => super::gating::is_trusted(context, from_id).await,
        None => Ok(false),
    }
}

/// The structured objects in a message, in order.
pub async fn of_msg(context: &Context, msg_id: MsgId) -> Result<Vec<StructuredObject>> {
    context
        .sql
        .query_map_vec(
            "SELECT seq, json, trusted, source FROM structured_data
             WHERE msg_id=? ORDER BY seq",
            (msg_id,),
            |row| {
                Ok(StructuredObject {
                    seq: row.get(0)?,
                    json: row.get(1)?,
                    trusted: row.get::<_, i64>(2)? != 0,
                    source: Source::from_i64(row.get(3)?),
                })
            },
        )
        .await
}

/// Drops rows whose message is gone. Runs in housekeeping.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM structured_data WHERE msg_id NOT IN (SELECT id FROM msgs)",
            (),
        )
        .await?;
    Ok(())
}

/// Pulls structured objects out of a parsed message.
///
/// Walks the MIME structure directly rather than reading `MimeMessage::parts`:
/// an `application/*` part with no filename is dropped there as a "Missing
/// attachment" (`mimeparser.rs`), which is exactly the shape an SML part has.
///
/// `decoded_data` holds the *decrypted* structure and is the only place an
/// encrypted message's parts can be read from — but core leaves it empty when
/// there was nothing to decrypt, so cleartext mail is read from the original
/// bytes instead.
fn extract(mime: &MimeMessage, imf_raw: &[u8]) -> Vec<(u32, String, Source)> {
    let raw = if mime.decoded_data.is_empty() {
        imf_raw
    } else {
        &mime.decoded_data
    };
    objects_in(raw)
}

/// The structured objects in a raw MIME message, in order.
///
/// Split out from [`extract`] so the parsing rules can be tested as what they
/// are -- a pure function of some bytes -- rather than through a receive path
/// whose own quirks are the reason this walks the raw message at all.
fn objects_in(raw: &[u8]) -> Vec<(u32, String, Source)> {
    let Ok(mail) = mailparse::parse_mail(raw) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    walk(&mail, Source::Mixed, &mut found);
    if found.is_empty() {
        // Only as a fallback, per ADR 0016: a sender who emits both should be
        // read through the mechanism they specified, not the one they improvised.
        html_scripts(&mail, &mut found);
    }
    found
        .into_iter()
        .enumerate()
        .map(|(i, (json, source))| (i as u32, json, source))
        .collect()
}

/// Recursively collects `application/ld+json` parts marked machine-readable.
fn walk(part: &ParsedMail<'_>, arrangement: Source, found: &mut Vec<(String, Source)>) {
    let ctype = part.ctype.mimetype.to_ascii_lowercase();
    if let Some(subtype) = ctype.strip_prefix("multipart/") {
        let inner = Source::from_multipart(subtype);
        for child in &part.subparts {
            walk(child, inner, found);
        }
        return;
    }
    if ctype != "application/ld+json" || !is_machine_readable(part) {
        return;
    }
    match part.get_body() {
        Ok(body) => push_json(&body, arrangement, found),
        Err(_) => {
            // A part we cannot decode is not a message we may fail.
        }
    }
}

/// Whether a part carries `Content-Purpose: Machine-readable`.
///
/// Compared case-insensitively with parameters stripped: it is a header value
/// like any other, and a sender who writes `machine-readable; v=1` means the
/// same thing as one who writes `Machine-Readable`.
fn is_machine_readable(part: &ParsedMail<'_>) -> bool {
    part.headers
        .get_first_value("Content-Purpose")
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("machine-readable")
        })
}

/// Collects `<script type="application/ld+json">` blocks from HTML parts.
fn html_scripts(part: &ParsedMail<'_>, found: &mut Vec<(String, Source)>) {
    if part.ctype.mimetype.eq_ignore_ascii_case("text/html")
        && let Ok(body) = part.get_body()
    {
        for block in script_blocks(&body) {
            push_json(&block, Source::HtmlScript, found);
        }
    }
    for child in &part.subparts {
        html_scripts(child, found);
    }
}

/// The contents of every `<script type="application/ld+json">` in `html`.
///
/// Scanned rather than parsed: `dehtml` throws script bodies away, and a
/// tolerant scan is what reads mail from senders whose markup is not
/// well-formed -- which is most of them.
///
/// Works on bytes and never slices the string by a computed index. The
/// delimiters are all ASCII, but a lowercased *copy* of the text can differ in
/// length from the original, so offsets taken in one and applied to the other
/// would land mid-character on exactly the mail most likely to be
/// interesting.
fn script_blocks(html: &str) -> Vec<String> {
    let bytes = html.as_bytes();
    let mut blocks = Vec::new();
    let mut cursor = 0usize;
    while let Some(tag_start) = find_ascii_ci(bytes, b"<script", cursor) {
        let Some(open_end) = find_ascii_ci(bytes, b">", tag_start).map(|i| i.saturating_add(1))
        else {
            break;
        };
        let is_ld_json =
            find_ascii_ci(bytes, b"application/ld+json", tag_start).is_some_and(|at| at < open_end);
        let Some(body_end) = find_ascii_ci(bytes, b"</script", open_end) else {
            break;
        };
        if is_ld_json && let Some(body) = html.get(open_end..body_end) {
            blocks.push(body.to_string());
        }
        cursor = body_end;
    }
    blocks
}

/// Case-insensitive ASCII search for `needle` at or after `from`.
fn find_ascii_ci(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let tail = haystack.get(from..)?;
    if needle.is_empty() || needle.len() > tail.len() {
        return None;
    }
    tail.windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|at| from.saturating_add(at))
}

/// Validates one JSON-LD document and appends what it contains.
///
/// A top-level `@graph` is flattened to one entry per member, because that is
/// what it means: several objects in one envelope, not one object with a list
/// inside it.
fn push_json(text: &str, source: Source, found: &mut Vec<(String, Source)>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        // Dropped, never fatal. See the module docs.
        return;
    };
    let members = value
        .get("@graph")
        .and_then(|graph| graph.as_array())
        .cloned();
    match members {
        Some(members) => {
            for member in members {
                found.push((member.to_string(), source));
            }
        }
        None => found.push((value.to_string(), source)),
    }
}

#[cfg(test)]
mod structured_tests;
