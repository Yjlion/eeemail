//! How important a message claims to be.
//!
//! Email has said this on the wire since the eighties, and says it three
//! different ways because nobody agreed on one:
//!
//! * `Importance: high | normal | low` — RFC 2156, and the one RFC 4021
//!   registers. What Outlook writes and reads.
//! * `X-Priority: 1 (Highest)` … `5 (Lowest)` — no RFC at all, and universally
//!   implemented. Thunderbird's.
//! * `Priority: urgent | normal | non-urgent` — RFC 2156 again, and about
//!   *delivery* rather than about how the reader should feel. Read, never
//!   written, because writing it would be asking a relay for something.
//!
//! eeemail writes the first two and reads all three. Writing one and not the
//! other would make the mark invisible to about half of everyone.
//!
//! # Absent when normal
//!
//! A `Normal` message stores no row and emits no header, so an ordinary
//! message is byte-identical to what upstream produces. That is not tidiness:
//! upstream has tests that compare rendered MIME, and a header on every message
//! would move them all.
//!
//! # Not a chat feature
//!
//! Upstream has no notion of this and no field to put it in, so it lives in a
//! side table keyed by `msgs.id` rather than in `msgs.param`. A side table is
//! also what lets `get_message_rows` join it once for a list rather than parse
//! a param blob per row.
//!
//! See [ADR 0029](../../../docs/adr/0029-importance-travels-on-the-wire.md).

use anyhow::Result;

use crate::context::Context;
use crate::message::MsgId;

/// How important a message claims to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Importance {
    /// The sender marked it down. Rare, and worth showing when it happens.
    Low = -1,
    /// What almost every message is. Never stored, never emitted.
    #[default]
    Normal = 0,
    /// The sender marked it up.
    High = 1,
}

impl Importance {
    fn from_i64(value: i64) -> Self {
        match value {
            v if v > 0 => Importance::High,
            v if v < 0 => Importance::Low,
            _ => Importance::Normal,
        }
    }

    /// The `Importance:` header value, or `None` when there is nothing to say.
    pub fn header_importance(self) -> Option<&'static str> {
        match self {
            Importance::High => Some("high"),
            Importance::Low => Some("low"),
            Importance::Normal => None,
        }
    }

    /// The `X-Priority:` header value, or `None` when there is nothing to say.
    ///
    /// The parenthesised word is conventional and some clients show it, so it
    /// is written the way the clients that read this header write it.
    pub fn header_x_priority(self) -> Option<&'static str> {
        match self {
            Importance::High => Some("1 (Highest)"),
            Importance::Low => Some("5 (Lowest)"),
            Importance::Normal => None,
        }
    }
}

/// Reads an `Importance:` value. Unknown words are `None`, not `Normal`.
///
/// The distinction matters: `None` means "this header told us nothing, try the
/// next one", and `Normal` means "this header said normal", which stops the
/// search. A message with `Importance: garbage` and `X-Priority: 1` is high.
fn parse_importance(value: &str) -> Option<Importance> {
    match value.trim().to_lowercase().as_str() {
        "high" => Some(Importance::High),
        "normal" => Some(Importance::Normal),
        "low" => Some(Importance::Low),
        _ => None,
    }
}

/// Reads an `X-Priority:` value: `1`–`5`, optionally followed by a word.
fn parse_x_priority(value: &str) -> Option<Importance> {
    // "1 (Highest)" -- take the leading digits and ignore the commentary.
    let digits: String = value
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    match digits.parse::<i64>().ok()? {
        1 | 2 => Some(Importance::High),
        3 => Some(Importance::Normal),
        4 | 5 => Some(Importance::Low),
        _ => None,
    }
}

/// Reads a `Priority:` value.
fn parse_priority(value: &str) -> Option<Importance> {
    match value.trim().to_lowercase().as_str() {
        "urgent" => Some(Importance::High),
        "normal" => Some(Importance::Normal),
        "non-urgent" => Some(Importance::Low),
        _ => None,
    }
}

/// Decides a message's importance from its headers.
///
/// `Importance:` wins, then `X-Priority:`, then `Priority:`. The order is by
/// how deliberate each one is: `Importance` is what a client sets when the user
/// ticks a box, `X-Priority` is often set by a mailer on the user's behalf, and
/// `Priority` is about delivery handling rather than about the reader.
pub fn from_headers(
    importance: Option<&str>,
    x_priority: Option<&str>,
    priority: Option<&str>,
) -> Importance {
    importance
        .and_then(parse_importance)
        .or_else(|| x_priority.and_then(parse_x_priority))
        .or_else(|| priority.and_then(parse_priority))
        .unwrap_or_default()
}

/// Sets a message's importance. `Normal` removes the row rather than storing it.
pub async fn set(context: &Context, msg_id: MsgId, importance: Importance) -> Result<()> {
    if importance == Importance::Normal {
        context
            .sql
            .execute("DELETE FROM msg_importance WHERE msg_id=?", (msg_id,))
            .await?;
        return Ok(());
    }
    context
        .sql
        .execute(
            "INSERT INTO msg_importance (msg_id, level) VALUES (?1, ?2)
             ON CONFLICT(msg_id) DO UPDATE SET level=excluded.level",
            (msg_id, importance as i64),
        )
        .await?;
    Ok(())
}

/// A message's importance. No row means [`Importance::Normal`].
pub async fn of_msg(context: &Context, msg_id: MsgId) -> Result<Importance> {
    let level: Option<i64> = context
        .sql
        .query_row_optional(
            "SELECT level FROM msg_importance WHERE msg_id=?",
            (msg_id,),
            |row| row.get(0),
        )
        .await?;
    Ok(level.map_or(Importance::Normal, Importance::from_i64))
}

#[cfg(test)]
mod importance_tests;
