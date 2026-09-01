//! Conversation threading.
//!
//! An email client groups messages into conversations by following the
//! `References` and `In-Reply-To` chains, per Jamie Zawinski's threading
//! algorithm. Core has no equivalent: it groups by *chat*, which is decided by
//! sender and group membership, not by what a message replies to.
//!
//! Both halves of the algorithm are here:
//!
//! * [`assign`] maintains the **grouping** -- which messages form one
//!   conversation. It runs once per message, as it arrives or is sent, and its
//!   result is persisted in `msg_threads`.
//! * [`tree`] builds the **shape** -- the parent/child structure within one
//!   conversation. It runs on demand, when a reading pane needs it, from data
//!   already on the messages.
//!
//! # Grouping out-of-order arrivals
//!
//! Messages do not arrive in causal order. A reply can be fetched before the
//! message it replies to, and the middle of a thread may never arrive at all.
//! So a thread is not identified by its root; it is identified by the *set of
//! Message-IDs it is known by*, held in `thread_refs`. Each message contributes
//! its own ID and every ID it references.
//!
//! That gives the three cases in [`assign`]: a message whose IDs match no
//! thread starts one, a message matching one thread joins it, and a message
//! matching several is the missing link that proves they were always one
//! conversation, so they are merged. Merging is what makes the result
//! independent of arrival order.
//!
//! # Subject-based merging is deliberately not implemented
//!
//! JWZ's step 5 merges threads whose subjects match after stripping `Re:`.
//! It is the step that produces false merges -- every unrelated "Hi" or
//! "Question" collapses into one conversation -- and it is worse here than in a
//! typical client, because encrypted mail routinely carries a generic subject
//! (`[...]`) so as not to leak the real one. Grouping strictly by reference
//! chains can leave a conversation split when a client fails to set
//! `References`; that is a visible and recoverable annoyance, whereas a false
//! merge silently shows two people each other's unrelated mail.

use anyhow::Result;
use std::collections::{HashMap, HashSet};

use crate::context::Context;
use crate::message::MsgId;
use crate::mimeparser::parse_message_ids;
use crate::tools::remove_subject_prefix;

/// Identifier of a conversation, a row in `threads`.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ThreadId(i64);

impl ThreadId {
    /// Wraps a raw database id.
    pub fn new(id: i64) -> Self {
        ThreadId(id)
    }

    /// Returns the raw database id.
    pub fn to_i64(self) -> i64 {
        self.0
    }
}

impl rusqlite::types::ToSql for ThreadId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Integer(self.0),
        ))
    }
}

impl rusqlite::types::FromSql for ThreadId {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        i64::column_result(value).map(ThreadId)
    }
}

/// A message's threading headers, as they arrived or as we sent them.
#[derive(Debug, Default, Clone)]
pub struct ThreadHeaders<'a> {
    /// The message's own `Message-ID`, without angle brackets.
    pub rfc724_mid: &'a str,

    /// Raw `In-Reply-To` header value, if any.
    pub in_reply_to: &'a str,

    /// Raw `References` header value, if any.
    pub references: &'a str,

    /// `Subject`, used only to label the thread.
    pub subject: &'a str,

    /// Sort timestamp, used to order threads by recency.
    pub timestamp: i64,
}

impl ThreadHeaders<'_> {
    /// Every Message-ID this message links the thread to: its own, plus every
    /// ID it references.
    ///
    /// Duplicates are collapsed and order is not meaningful -- these are used
    /// only to find the threads to join.
    fn linked_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        for id in std::iter::once(self.rfc724_mid.to_string())
            .chain(parse_message_ids(self.references))
            .chain(parse_message_ids(self.in_reply_to))
        {
            if !id.is_empty() && seen.insert(id.clone()) {
                ids.push(id);
            }
        }
        ids
    }
}

/// Places `msg_id` in a conversation, creating or merging threads as needed.
///
/// Idempotent: re-running for a message already assigned re-derives the same
/// thread, so a message that is reprocessed is not duplicated or orphaned.
pub async fn assign(
    context: &Context,
    msg_id: MsgId,
    headers: ThreadHeaders<'_>,
) -> Result<ThreadId> {
    let linked_ids = headers.linked_ids();
    let subject_norm = remove_subject_prefix(headers.subject);
    let timestamp = headers.timestamp;

    context
        .sql
        .transaction(move |transaction| {
            // Which threads do these Message-IDs already belong to?
            let mut existing: Vec<i64> = Vec::new();
            {
                let mut stmt =
                    transaction.prepare("SELECT thread_id FROM thread_refs WHERE rfc724_mid=?")?;
                for id in &linked_ids {
                    let mut rows = stmt.query((id,))?;
                    if let Some(row) = rows.next()? {
                        let thread_id: i64 = row.get(0)?;
                        if !existing.contains(&thread_id) {
                            existing.push(thread_id);
                        }
                    }
                }
            }
            existing.sort_unstable();

            // The oldest thread survives a merge, so a long-running
            // conversation keeps its identity when a stray reply joins it to a
            // newer fragment.
            let thread_id = match existing.first() {
                Some(&survivor) => {
                    for &loser in existing.iter().skip(1) {
                        transaction.execute(
                            "UPDATE msg_threads SET thread_id=?1 WHERE thread_id=?2",
                            (survivor, loser),
                        )?;
                        transaction.execute(
                            "UPDATE thread_refs SET thread_id=?1 WHERE thread_id=?2",
                            (survivor, loser),
                        )?;
                        // Keep the merged-away thread's label if the survivor
                        // never got one, and carry over its recency.
                        transaction.execute(
                            "UPDATE threads SET
                                subject_norm=IIF(subject_norm='',
                                    (SELECT subject_norm FROM threads WHERE id=?2), subject_norm),
                                last_activity=MAX(last_activity,
                                    IFNULL((SELECT last_activity FROM threads WHERE id=?2), 0))
                             WHERE id=?1",
                            (survivor, loser),
                        )?;
                        transaction.execute("DELETE FROM threads WHERE id=?", (loser,))?;
                    }
                    survivor
                }
                None => {
                    transaction.execute(
                        "INSERT INTO threads (subject_norm, last_activity) VALUES (?1, ?2)",
                        (&subject_norm, timestamp),
                    )?;
                    transaction.last_insert_rowid()
                }
            };

            for id in &linked_ids {
                transaction.execute(
                    "INSERT INTO thread_refs (rfc724_mid, thread_id) VALUES (?1, ?2)
                     ON CONFLICT(rfc724_mid) DO UPDATE SET thread_id=excluded.thread_id",
                    (id, thread_id),
                )?;
            }

            transaction.execute(
                "INSERT INTO msg_threads (msg_id, thread_id) VALUES (?1, ?2)
                 ON CONFLICT(msg_id) DO UPDATE SET thread_id=excluded.thread_id",
                (msg_id, thread_id),
            )?;

            // A thread created from a reply is labelled with the reply's
            // subject; the real one arrives with the root. Only fill a blank.
            transaction.execute(
                "UPDATE threads SET
                    subject_norm=IIF(subject_norm='', ?2, subject_norm),
                    last_activity=MAX(last_activity, ?3)
                 WHERE id=?1",
                (thread_id, &subject_norm, timestamp),
            )?;

            Ok(ThreadId(thread_id))
        })
        .await
}

/// Places a stored message in a conversation, reading its threading headers
/// from its `msgs` row.
///
/// This is what both the receive and the send path call. Deriving the headers
/// from the stored row rather than passing them in means the thread can never
/// disagree with the message: `tree` resolves references against
/// `msgs.rfc724_mid`, so grouping must use the same value the row carries.
///
/// Returns `None` if the message does not exist.
pub async fn assign_stored(context: &Context, msg_id: MsgId) -> Result<Option<ThreadId>> {
    let row = context
        .sql
        .query_row_optional(
            // None of these columns is NOT NULL, and rows written before their
            // defaults were added still carry NULLs. Guard every one: a NULL
            // here would fail the whole assignment.
            "SELECT IFNULL(rfc724_mid, ''), IFNULL(mime_in_reply_to, ''),
                    IFNULL(mime_references, ''), IFNULL(subject, ''),
                    IFNULL(timestamp, 0)
             FROM msgs WHERE id=?",
            (msg_id,),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .await?;
    let Some((rfc724_mid, in_reply_to, references, subject, timestamp)) = row else {
        return Ok(None);
    };

    let thread_id = assign(
        context,
        msg_id,
        ThreadHeaders {
            rfc724_mid: &rfc724_mid,
            in_reply_to: &in_reply_to,
            references: &references,
            subject: &subject,
            timestamp,
        },
    )
    .await?;
    Ok(Some(thread_id))
}

/// Returns the thread `msg_id` belongs to, if it has been assigned one.
pub async fn thread_of(context: &Context, msg_id: MsgId) -> Result<Option<ThreadId>> {
    context
        .sql
        .query_get_value(
            "SELECT thread_id FROM msg_threads WHERE msg_id=?",
            (msg_id,),
        )
        .await
}

/// Returns the messages of a thread, oldest first.
///
/// Trashed messages are excluded: they are tombstones kept to suppress
/// re-download, not content.
pub async fn messages(context: &Context, thread_id: ThreadId) -> Result<Vec<MsgId>> {
    context
        .sql
        .query_map_vec(
            "SELECT m.id FROM msg_threads t
             JOIN msgs m ON m.id=t.msg_id
             WHERE t.thread_id=? AND m.chat_id!=?
             ORDER BY m.timestamp, m.id",
            (thread_id, crate::constants::DC_CHAT_ID_TRASH),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await
}

/// A message and its replies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadNode {
    /// The message at this position.
    pub msg_id: MsgId,

    /// Replies to it, oldest first.
    pub children: Vec<ThreadNode>,
}

/// Builds the reply tree of a thread, returning its roots oldest first.
///
/// A thread usually has one root, but can have several: if the message that
/// started the conversation was never received, each of its surviving replies
/// becomes a root rather than being hidden under a placeholder. Referenced
/// messages we do not have contribute structure but never appear as nodes.
pub async fn tree(context: &Context, thread_id: ThreadId) -> Result<Vec<ThreadNode>> {
    let rows: Vec<(MsgId, String, String, String)> = context
        .sql
        .query_map_vec(
            "SELECT m.id, IFNULL(m.rfc724_mid, ''), IFNULL(m.mime_in_reply_to, ''),
                    IFNULL(m.mime_references, '')
             FROM msg_threads t
             JOIN msgs m ON m.id=t.msg_id
             WHERE t.thread_id=? AND m.chat_id!=?
             ORDER BY m.timestamp, m.id",
            (thread_id, crate::constants::DC_CHAT_ID_TRASH),
            |row| {
                Ok((
                    row.get::<_, MsgId>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .await?;

    // Message-ID -> message, for resolving references. Later duplicates lose:
    // if two messages claim one ID, the older is the original.
    let mut by_mid: HashMap<&str, MsgId> = HashMap::new();
    for (msg_id, rfc724_mid, _, _) in &rows {
        if !rfc724_mid.is_empty() {
            by_mid.entry(rfc724_mid.as_str()).or_insert(*msg_id);
        }
    }

    let order: HashMap<MsgId, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, (msg_id, _, _, _))| (*msg_id, i))
        .collect();

    // Nearest ancestor we actually hold. Walking the References chain from the
    // end skips over messages that were never received, which is JWZ's
    // "prune empty containers" without needing the containers.
    let mut parent: HashMap<MsgId, MsgId> = HashMap::new();
    for (msg_id, _, in_reply_to, references) in &rows {
        let mut candidates = parse_message_ids(references);
        candidates.extend(parse_message_ids(in_reply_to));
        for candidate in candidates.iter().rev() {
            let Some(&candidate_id) = by_mid.get(candidate.as_str()) else {
                continue;
            };
            if candidate_id == *msg_id {
                continue;
            }
            // A message can only descend from an older one. This is what stops
            // a forged or looping References header building a cycle.
            if order.get(&candidate_id) >= order.get(msg_id) {
                continue;
            }
            parent.insert(*msg_id, candidate_id);
            break;
        }
    }

    let mut children: HashMap<MsgId, Vec<MsgId>> = HashMap::new();
    let mut roots: Vec<MsgId> = Vec::new();
    for (msg_id, _, _, _) in &rows {
        match parent.get(msg_id) {
            Some(parent_id) => children.entry(*parent_id).or_default().push(*msg_id),
            None => roots.push(*msg_id),
        }
    }

    Ok(roots
        .into_iter()
        .map(|root| build_node(root, &children))
        .collect())
}

/// Builds one subtree.
///
/// Iterative rather than recursive: depth follows the reply chain, which is
/// attacker-influenced, and a deep enough thread would overflow the stack.
fn build_node(root: MsgId, children: &HashMap<MsgId, Vec<MsgId>>) -> ThreadNode {
    // Depth-first, emitting each node only once all its children are built.
    enum Step {
        Enter(MsgId),
        Exit(MsgId, usize),
    }
    let mut stack = vec![Step::Enter(root)];
    let mut done: Vec<ThreadNode> = Vec::new();

    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(msg_id) => {
                let kids = children.get(&msg_id).map(Vec::as_slice).unwrap_or_default();
                stack.push(Step::Exit(msg_id, kids.len()));
                for &kid in kids.iter().rev() {
                    stack.push(Step::Enter(kid));
                }
            }
            Step::Exit(msg_id, count) => {
                let at = done.len().saturating_sub(count);
                let node_children = done.split_off(at);
                done.push(ThreadNode {
                    msg_id,
                    children: node_children,
                });
            }
        }
    }

    done.pop().unwrap_or(ThreadNode {
        msg_id: root,
        children: Vec::new(),
    })
}

/// Removes threading data for messages that no longer exist, and then any
/// thread left with no messages.
///
/// Called from housekeeping, mirroring how `msgs_mdns` is pruned.
/// `thread_refs` rows for a dropped thread go too, so a much later reply to a
/// deleted conversation starts a fresh thread rather than resurrecting an empty
/// one.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .transaction(|transaction| {
            transaction.execute(
                "DELETE FROM msg_threads WHERE msg_id NOT IN \
                 (SELECT id FROM msgs WHERE chat_id!=?)",
                (crate::constants::DC_CHAT_ID_TRASH,),
            )?;
            transaction.execute(
                "DELETE FROM threads WHERE id NOT IN (SELECT thread_id FROM msg_threads)",
                (),
            )?;
            transaction.execute(
                "DELETE FROM thread_refs WHERE thread_id NOT IN (SELECT id FROM threads)",
                (),
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod threading_tests;
