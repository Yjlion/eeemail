//! System tags, derived where possible and stored where they must be.
//!
//! [ADR 0005] chose tags over a folder tree and named five system labels.
//! [ADR 0009] then built exactly one of them -- `Archive`, as the *presence* of
//! a reserved label -- and deferred the rest with the note that Sent, Drafts and
//! Trash "must be derived from `MessageState` and `chat_id`, which core already
//! owns. Storing them would create a second source of truth."
//!
//! That was right, and it left the client with one system tag. This module is
//! the rest of it, split by a single rule: **a system tag gets a row only if it
//! carries state core does not already have.**
//!
//! * [`SystemTag::Inbox`], [`SystemTag::Sent`] and [`SystemTag::Drafts`] are
//!   **derived** from `MessageState` and direction. No rows, so nothing to keep
//!   consistent and nothing to migrate.
//! * [`SystemTag::Archive`], [`SystemTag::Trash`] and [`SystemTag::Unverified`] are
//!   **stored**, because each is either a user action that must survive a failed
//!   hook (`Archive`, see [ADR 0009]) or carries a purge deadline (`Trash` in
//!   [`super::ephemeral`], `Unverified` in [`super::gating`]).
//!
//! The point of the whole arrangement is that the user files nothing and still
//! has a working mailbox. See [ADR 0017].
//!
//! [ADR 0005]: ../../../docs/adr/0005-labels-not-folders.md
//! [ADR 0009]: ../../../docs/adr/0009-labels-and-search.md
//! [ADR 0017]: ../../../docs/adr/0017-system-tags.md

use anyhow::Result;

use crate::context::Context;
use crate::message::{MessageState, MsgId};

use super::labels::{self, ARCHIVE, Label, TRASH, UNVERIFIED};

/// A tag every account has, without the user creating anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SystemTag {
    /// Incoming mail that has not been archived, held or trashed.
    Inbox,
    /// Mail from a sender who is neither verified nor known.
    Unverified,
    /// Outgoing mail that has left the drafts state.
    Sent,
    /// Unsent outgoing mail.
    Drafts,
    /// Mail the user has archived.
    Archive,
    /// Mail the user threw away, or whose ephemeral timer fired.
    Trash,
}

impl SystemTag {
    /// Every system tag, in the order a sidebar should show them.
    ///
    /// Inbox and Unverified lead because they are the two views with mail waiting
    /// in them; Trash is last because it is where things go to stop mattering.
    pub const ALL: [SystemTag; 6] = [
        SystemTag::Inbox,
        SystemTag::Unverified,
        SystemTag::Sent,
        SystemTag::Drafts,
        SystemTag::Archive,
        SystemTag::Trash,
    ];

    /// The reserved label backing this tag, or `None` if it is derived.
    ///
    /// This *is* the derived/stored split, in one function, so the rest of the
    /// module never has to restate it.
    pub fn stored_name(self) -> Option<&'static str> {
        match self {
            SystemTag::Archive => Some(ARCHIVE),
            SystemTag::Trash => Some(TRASH),
            SystemTag::Unverified => Some(UNVERIFIED),
            SystemTag::Inbox | SystemTag::Sent | SystemTag::Drafts => None,
        }
    }

    /// A stable identifier for the RPC surface and the CLI.
    ///
    /// Lowercase and unlocalised, like the reserved label names it mirrors.
    pub fn as_str(self) -> &'static str {
        match self {
            SystemTag::Inbox => "inbox",
            SystemTag::Unverified => "unverified",
            SystemTag::Sent => "sent",
            SystemTag::Drafts => "drafts",
            SystemTag::Archive => "archive",
            SystemTag::Trash => "trash",
        }
    }

    /// Parses [`Self::as_str`].
    ///
    /// Named `parse` rather than `from_str` so it cannot be mistaken for
    /// `FromStr::from_str`, which returns a `Result` and which this is not.
    pub fn parse(s: &str) -> Option<Self> {
        SystemTag::ALL
            .into_iter()
            .find(|tag| tag.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

/// Every tag on a message: system tags and user tags together.
///
/// Returned as one value rather than left to a caller to assemble, because
/// assembling it means knowing the derived/stored rule, and a UI that has to
/// know that rule will eventually get it wrong.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    /// System tags, in [`SystemTag::ALL`] order.
    pub system: Vec<SystemTag>,
    /// User-created tags, system ones excluded.
    pub user: Vec<Label>,
}

/// Returns every tag on a message.
pub async fn of_msg(context: &Context, msg_id: MsgId) -> Result<Tags> {
    let labels = labels::of_msg(context, msg_id).await?;
    let stored: Vec<&str> = labels
        .iter()
        .filter(|l| l.is_system)
        .map(|l| l.name.as_str())
        .collect();

    let mut system = Vec::new();
    for tag in SystemTag::ALL {
        let present = match tag.stored_name() {
            Some(name) => stored.iter().any(|s| s.eq_ignore_ascii_case(name)),
            None => derived(context, msg_id, tag, &stored).await?,
        };
        if present {
            system.push(tag);
        }
    }

    Ok(Tags {
        system,
        user: labels.into_iter().filter(|l| !l.is_system).collect(),
    })
}

/// Whether a derived tag applies, given the stored system tags already found.
async fn derived(
    context: &Context,
    msg_id: MsgId,
    tag: SystemTag,
    stored: &[&str],
) -> Result<bool> {
    let Some(state) = state_of(context, msg_id).await? else {
        return Ok(false);
    };
    Ok(match tag {
        SystemTag::Drafts => state == MessageState::OutDraft,
        SystemTag::Sent => state.is_outgoing() && state != MessageState::OutDraft,
        // The inbox is what is left over: incoming, and filed nowhere else.
        // Expressed as "no stored system tag" rather than as a list, so a
        // system tag added later cannot forget to remove its mail from here.
        SystemTag::Inbox => is_incoming(state) && stored.is_empty(),
        SystemTag::Archive | SystemTag::Trash | SystemTag::Unverified => false,
    })
}

/// The mirror of [`MessageState::is_outgoing`], which upstream does not have.
///
/// Not `!is_outgoing()`: `MessageState::Undefined` is neither, and treating it
/// as incoming would put rows with no state into the inbox.
fn is_incoming(state: MessageState) -> bool {
    state >= MessageState::InFresh && state < MessageState::OutDraft
}

/// Reads a message's state, or `None` if it no longer exists.
async fn state_of(context: &Context, msg_id: MsgId) -> Result<Option<MessageState>> {
    context
        .sql
        .query_get_value::<MessageState>("SELECT state FROM msgs WHERE id=?", (msg_id,))
        .await
}

/// Returns the messages carrying a tag, newest first.
pub async fn messages(context: &Context, tag: SystemTag) -> Result<Vec<MsgId>> {
    // A stored tag is exactly its label, so reuse the query that already
    // excludes trashed and hidden rows rather than writing a second one.
    if let Some(name) = tag.stored_name() {
        let label = labels::reserved(context, name).await?;
        return labels::msgs_with(context, label.id).await;
    }

    // A draft is stored `hidden=1`, because core hides it from its chat's
    // message list -- the draft belongs in the composer, not in the
    // conversation. The Drafts view is the one place it *should* appear, so it
    // is the one query that must not filter hidden rows.
    let base = match tag {
        SystemTag::Drafts => "SELECT m.id FROM msgs m WHERE m.chat_id!=?1",
        _ => "SELECT m.id FROM msgs m WHERE m.chat_id!=?1 AND m.hidden=0",
    };
    let filed = "EXISTS (SELECT 1 FROM msg_labels ml JOIN labels l ON l.id=ml.label_id
                         WHERE ml.msg_id=m.id AND l.system=1)";
    let order = "ORDER BY m.timestamp DESC, m.id DESC";

    // ?2 is InFresh and ?3 is OutDraft: the two boundaries that separate
    // incoming from draft from sent. Kept as parameters rather than inlined
    // numbers so that renumbering the enum cannot silently reclassify mail.
    let sql = match tag {
        SystemTag::Drafts => format!("{base} AND m.state=?3 {order}"),
        SystemTag::Sent => format!("{base} AND m.state>?3 {order}"),
        SystemTag::Inbox => {
            format!("{base} AND m.state>=?2 AND m.state<?3 AND NOT {filed} {order}")
        }
        SystemTag::Archive | SystemTag::Trash | SystemTag::Unverified => {
            unreachable!("stored above")
        }
    };

    context
        .sql
        .query_map_vec(
            &sql,
            (
                crate::constants::DC_CHAT_ID_TRASH,
                MessageState::InFresh as i64,
                MessageState::OutDraft as i64,
            ),
            |row| Ok(row.get::<_, MsgId>(0)?),
        )
        .await
}

#[cfg(test)]
mod tags_tests;
