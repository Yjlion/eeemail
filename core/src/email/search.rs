//! Search across body, subject, recipients and labels.
//!
//! Core's [`Context::search_msgs`] matches the message *body* only, which is
//! all a chat client needs. An email client is expected to find a message by
//! who it was addressed to, by its subject, and within a label.
//!
//! This is a separate entry point rather than a change to
//! [`Context::search_msgs`]: upstream's version keeps working and keeps its
//! tests, and we take no merge conflict on a function upstream tunes for
//! performance. See `docs/fork-patches.md`.
//!
//! # Matching
//!
//! `LIKE` with a lowercased needle, the same approach and the same 1000-row cap
//! upstream uses. Not FTS: an index over decrypted mail is a plaintext copy of
//! the mailbox in a second place, which matters more here than search latency,
//! and `LIKE` is what upstream's own benchmarks are tuned against. If search
//! becomes too slow to use, that is the point to revisit it deliberately.

use anyhow::Result;
use rusqlite::types::Value;

use crate::chat::ChatId;
use crate::context::Context;
use crate::message::MsgId;

use super::labels::LabelId;

/// What to search for. An empty [`SearchQuery`] matches nothing, not
/// everything, so an empty search box does not return the mailbox.
#[derive(Debug, Default, Clone)]
pub struct SearchQuery {
    /// Matched case-insensitively against body, subject and recipient
    /// addresses and names. Empty means "do not filter by text".
    pub text: String,

    /// Restrict to messages carrying this label.
    pub label: Option<LabelId>,

    /// `Some(true)` for archived only, `Some(false)` for the inbox, `None` for
    /// both.
    pub archived: Option<bool>,

    /// Restrict to one conversation.
    pub chat_id: Option<ChatId>,
}

impl SearchQuery {
    /// A plain text search, the common case.
    pub fn text(needle: &str) -> Self {
        Self {
            text: needle.to_string(),
            ..Default::default()
        }
    }

    /// Restricts the search to a label.
    pub fn with_label(mut self, label: LabelId) -> Self {
        self.label = Some(label);
        self
    }

    /// Restricts the search to archived or unarchived messages.
    pub fn with_archived(mut self, archived: bool) -> Self {
        self.archived = Some(archived);
        self
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.label.is_none()
            && self.archived.is_none()
            && self.chat_id.is_none()
    }
}

/// Result cap, matching upstream's. Incremental search issues a query per
/// keystroke, and the early ones match far too much to be worth ranking.
const LIMIT: usize = 1000;

/// Searches for messages, newest first.
pub async fn search(context: &Context, query: &SearchQuery) -> Result<Vec<MsgId>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut sql = String::from(
        "SELECT m.id FROM msgs m
         LEFT JOIN contacts ct ON m.from_id=ct.id
         LEFT JOIN chats c ON m.chat_id=c.id
         WHERE m.chat_id>9
           AND m.hidden=0
           AND IFNULL(c.blocked, 0)!=1
           AND IFNULL(ct.blocked, 0)=0",
    );
    // `Value` rather than boxed `ToSql`: the query runs on the SQL thread, and
    // trait objects are not `Sync`.
    let mut params: Vec<Value> = Vec::new();

    let needle = query.text.trim().to_lowercase();
    if !needle.is_empty() {
        let like = format!("%{needle}%");
        // Recipients are matched with EXISTS rather than a join, so a message
        // with several matching recipients is returned once.
        sql.push_str(
            " AND (IFNULL(m.txt_normalized, m.txt) LIKE ?
                   OR LOWER(IFNULL(m.subject, '')) LIKE ?
                   OR EXISTS (SELECT 1 FROM msg_recipients r
                              WHERE r.msg_id=m.id
                                AND (r.addr LIKE ? OR LOWER(r.name) LIKE ?)))",
        );
        for _ in 0..4 {
            params.push(Value::Text(like.clone()));
        }
    }

    if let Some(label) = query.label {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM msg_labels ml WHERE ml.msg_id=m.id AND ml.label_id=?)",
        );
        params.push(Value::Integer(label.to_i64()));
    }

    if let Some(archived) = query.archived {
        sql.push_str(if archived {
            " AND EXISTS (SELECT 1 FROM msg_labels ml JOIN labels l ON l.id=ml.label_id
                          WHERE ml.msg_id=m.id AND l.name_norm=?)"
        } else {
            " AND NOT EXISTS (SELECT 1 FROM msg_labels ml JOIN labels l ON l.id=ml.label_id
                              WHERE ml.msg_id=m.id AND l.name_norm=?)"
        });
        params.push(Value::Text(super::labels::ARCHIVE.to_lowercase()));
    }

    if let Some(chat_id) = query.chat_id {
        sql.push_str(" AND m.chat_id=?");
        params.push(Value::Integer(i64::from(chat_id.to_u32())));
    }

    sql.push_str(&format!(" ORDER BY m.id DESC LIMIT {LIMIT}"));

    context
        .sql
        .query_map_vec(&sql, rusqlite::params_from_iter(params), |row| {
            Ok(row.get::<_, MsgId>(0)?)
        })
        .await
}

#[cfg(test)]
mod search_tests;
