//! JSON-RPC types for eeemail's email layer.
//!
//! Everything here is eeemail's own, not forked from `chatmail/core`. The
//! methods that return these live in a marked block at the end of
//! `impl CommandApi`, because `yerpc`'s `#[rpc]` attribute can only be applied
//! to one `impl` block; keeping the types out here holds the upstream diff to
//! thin wrappers. See `docs/fork-patches.md`.

use deltachat::email::labels::Label;
use deltachat::email::policy::{EncryptionMode, MessageCrypto, ServerRetention};
use deltachat::email::receipts::MdnPolicy;
use deltachat::email::recipients::{Recipient, RecipientKind};
use deltachat::email::threading::ThreadNode;
use serde::{Deserialize, Serialize};
use typescript_type_def::TypeDef;

/// Which address header a recipient appeared in.
#[derive(Serialize, Deserialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "RecipientKind", rename_all = "camelCase")]
pub enum JsonrpcRecipientKind {
    /// `To:`
    To,
    /// `Cc:`
    Cc,
    /// `Bcc:`, only ever on messages we sent.
    Bcc,
}

impl From<RecipientKind> for JsonrpcRecipientKind {
    fn from(kind: RecipientKind) -> Self {
        match kind {
            RecipientKind::To => Self::To,
            RecipientKind::Cc => Self::Cc,
            RecipientKind::Bcc => Self::Bcc,
        }
    }
}

/// One addressee of one message.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "Recipient", rename_all = "camelCase")]
pub struct JsonrpcRecipient {
    /// Which header the address appeared in.
    pub kind: JsonrpcRecipientKind,
    /// Normalized, lowercased address.
    pub addr: String,
    /// Display name as sent, empty if the address was bare.
    pub name: String,
}

impl From<Recipient> for JsonrpcRecipient {
    fn from(r: Recipient) -> Self {
        Self {
            kind: r.kind.into(),
            addr: r.addr,
            name: r.name,
        }
    }
}

/// A label as the user sees it.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "Label", rename_all = "camelCase")]
pub struct JsonrpcLabel {
    /// Local row id. Differs between devices; sync uses the name.
    pub id: i64,
    /// Name as the user typed it.
    pub name: String,
    /// `#rrggbb`, or `null` if no colour was chosen.
    pub color: Option<String>,
    /// True for labels we reserve, which cannot be renamed or deleted.
    pub is_system: bool,
}

impl From<Label> for JsonrpcLabel {
    fn from(l: Label) -> Self {
        Self {
            id: l.id.to_i64(),
            name: l.name,
            color: l.color.map(super::color_int_to_hex_string),
            is_system: l.is_system,
        }
    }
}

/// One message's position in a conversation.
///
/// Threads are returned **flat**, in display order, rather than as a nested
/// tree. A nested type cannot be expressed in the generated TypeScript (it is
/// directly recursive), and a flat list is what a threaded reading pane
/// renders anyway -- it also cannot blow the JSON nesting limit on a
/// pathologically deep reply chain.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "ThreadItem", rename_all = "camelCase")]
pub struct JsonrpcThreadItem {
    /// The message at this position.
    pub msg_id: u32,

    /// The nearest ancestor we actually hold, or `null` if this is a root.
    ///
    /// Referenced messages that were never received contribute structure but
    /// never appear, so a reply to a missing message attaches to its
    /// grandparent, or becomes a root.
    pub parent_msg_id: Option<u32>,

    /// Indentation level. `0` for a root.
    pub depth: u32,
}

/// Flattens a reply tree into display order: depth-first, roots oldest first.
pub fn flatten_thread(roots: Vec<ThreadNode>) -> Vec<JsonrpcThreadItem> {
    let mut out = Vec::new();
    // Iterative, because reply depth is bounded only by how many messages a
    // sender can get us to store.
    let mut stack: Vec<(ThreadNode, Option<u32>, u32)> = roots
        .into_iter()
        .rev()
        .map(|node| (node, None, 0))
        .collect();
    while let Some((node, parent_msg_id, depth)) = stack.pop() {
        let msg_id = node.msg_id.to_u32();
        out.push(JsonrpcThreadItem {
            msg_id,
            parent_msg_id,
            depth,
        });
        for child in node.children.into_iter().rev() {
            stack.push((child, Some(msg_id), depth.saturating_add(1)));
        }
    }
    out
}

/// How strictly to encrypt.
#[derive(Serialize, Deserialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "EncryptionMode", rename_all = "camelCase")]
pub enum JsonrpcEncryptionMode {
    /// End-to-end only. Cleartext is neither sent nor accepted.
    Strict,
    /// Encrypt whenever the keys are there. The default.
    Opportunistic,
    /// Prefer delivery over encryption when a recipient has no key.
    Lenient,
}

impl From<EncryptionMode> for JsonrpcEncryptionMode {
    fn from(mode: EncryptionMode) -> Self {
        match mode {
            EncryptionMode::Strict => Self::Strict,
            EncryptionMode::Opportunistic => Self::Opportunistic,
            EncryptionMode::Lenient => Self::Lenient,
        }
    }
}

impl From<JsonrpcEncryptionMode> for EncryptionMode {
    fn from(mode: JsonrpcEncryptionMode) -> Self {
        match mode {
            JsonrpcEncryptionMode::Strict => Self::Strict,
            JsonrpcEncryptionMode::Opportunistic => Self::Opportunistic,
            JsonrpcEncryptionMode::Lenient => Self::Lenient,
        }
    }
}

/// Who gets read receipts.
#[derive(Serialize, Deserialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "MdnPolicy", rename_all = "camelCase")]
pub enum JsonrpcMdnPolicy {
    /// Never send read receipts.
    Never,
    /// Only to SecureJoin-verified contacts who are in the address book.
    VerifiedOnly,
    /// To anyone who asks. The default.
    Always,
}

impl From<MdnPolicy> for JsonrpcMdnPolicy {
    fn from(policy: MdnPolicy) -> Self {
        match policy {
            MdnPolicy::Never => Self::Never,
            MdnPolicy::VerifiedOnly => Self::VerifiedOnly,
            MdnPolicy::Always => Self::Always,
        }
    }
}

impl From<JsonrpcMdnPolicy> for MdnPolicy {
    fn from(policy: JsonrpcMdnPolicy) -> Self {
        match policy {
            JsonrpcMdnPolicy::Never => Self::Never,
            JsonrpcMdnPolicy::VerifiedOnly => Self::VerifiedOnly,
            JsonrpcMdnPolicy::Always => Self::Always,
        }
    }
}

/// The cryptographic standing of one message.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "MessageCrypto", rename_all = "camelCase")]
pub struct JsonrpcMessageCrypto {
    /// The message was end-to-end encrypted.
    pub encrypted: bool,
    /// It carried a valid signature from a key we hold.
    pub signed: bool,
    /// The signing key belongs to a SecureJoin-verified contact.
    ///
    /// The only one of the three that survives an active attacker, so the only
    /// one worth a strong indicator in the UI.
    pub verified: bool,
}

impl From<MessageCrypto> for JsonrpcMessageCrypto {
    fn from(c: MessageCrypto) -> Self {
        Self {
            encrypted: c.encrypted,
            signed: c.signed,
            verified: c.verified,
        }
    }
}

/// How long a downloaded message is left on the server.
///
/// Expressed as days so it is one field, matching the stored config:
/// `0` deletes as soon as the message is stored locally, a positive number
/// keeps it that many days, and `-1` never deletes -- the coexistence mode for
/// an account also read by another client.
#[derive(Serialize, Deserialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "ServerRetention", rename_all = "camelCase")]
pub struct JsonrpcServerRetention {
    /// Days to keep. `0` = delete after download, `-1` = never delete.
    pub days: i32,
}

impl From<ServerRetention> for JsonrpcServerRetention {
    fn from(retention: ServerRetention) -> Self {
        Self {
            days: match retention {
                ServerRetention::DeleteAfterDownload => 0,
                ServerRetention::Never => -1,
                ServerRetention::Days(d) => i32::try_from(d).unwrap_or(i32::MAX),
            },
        }
    }
}

impl From<JsonrpcServerRetention> for ServerRetention {
    fn from(r: JsonrpcServerRetention) -> Self {
        match r.days {
            0 => ServerRetention::DeleteAfterDownload,
            d if d < 0 => ServerRetention::Never,
            d => ServerRetention::Days(d.unsigned_abs()),
        }
    }
}

/// What to search for. Every field is optional; an entirely empty query matches
/// nothing rather than everything, so an empty search box does not return the
/// mailbox.
#[derive(Deserialize, TypeDef, schemars::JsonSchema, Default)]
#[serde(rename = "EmailSearchQuery", rename_all = "camelCase")]
pub struct JsonrpcSearchQuery {
    /// Matched case-insensitively against body, subject and recipients.
    #[serde(default)]
    pub text: Option<String>,
    /// Restrict to messages carrying this label.
    #[serde(default)]
    pub label_id: Option<i64>,
    /// `true` for archived only, `false` for the inbox, absent for both.
    #[serde(default)]
    pub archived: Option<bool>,
    /// Restrict to one conversation.
    #[serde(default)]
    pub chat_id: Option<u32>,
}

/// Who a message is addressed to.
#[derive(Deserialize, TypeDef, schemars::JsonSchema, Default)]
#[serde(rename = "RecipientSet", rename_all = "camelCase")]
pub struct JsonrpcRecipientSet {
    /// `To:` addresses. `Name <addr@example.org>` or a bare address.
    #[serde(default)]
    pub to: Vec<String>,
    /// `Cc:` addresses.
    #[serde(default)]
    pub cc: Vec<String>,
    /// `Bcc:` addresses. Never written to a header.
    #[serde(default)]
    pub bcc: Vec<String>,
}

impl From<JsonrpcRecipientSet> for deltachat::email::compose::RecipientSet {
    fn from(set: JsonrpcRecipientSet) -> Self {
        Self {
            to: set.to,
            cc: set.cc,
            bcc: set.bcc,
        }
    }
}

/// What at-rest protection is actually in force.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "AtRestProtection", rename_all = "camelCase")]
pub struct JsonrpcProtection {
    /// The SQLite database is encrypted with SQLCipher.
    pub database_encrypted: bool,
    /// Always `false`. Nothing encrypts the blobdir.
    pub blobs_encrypted: bool,
    /// Bytes of cleartext in the blobdir: attachments and retained raw MIME.
    pub cleartext_bytes: f64,
    /// True when the database is encrypted but cleartext files remain beside it.
    pub partial: bool,
    /// A sentence a settings screen can show verbatim.
    pub summary: String,
}

impl From<deltachat::email::vault::Protection> for JsonrpcProtection {
    fn from(p: deltachat::email::vault::Protection) -> Self {
        let summary = p.summary();
        Self {
            database_encrypted: p.database_encrypted,
            blobs_encrypted: p.blobs_encrypted,
            // f64 rather than u64: JSON numbers are doubles, and a blobdir will
            // not reach the point where that loses precision.
            cleartext_bytes: p.cleartext_bytes as f64,
            partial: p.partial,
            summary,
        }
    }
}

/// When the last backup was taken.
#[derive(Serialize, TypeDef, schemars::JsonSchema)]
#[serde(rename = "BackupStatus", rename_all = "camelCase")]
pub struct JsonrpcBackupStatus {
    /// Unix timestamp of the last successful backup, or `null` if never.
    pub last_backup: Option<i64>,
    /// True if there has never been one, or the last is over a week old.
    pub stale: bool,
}

impl From<deltachat::email::backup::BackupStatus> for JsonrpcBackupStatus {
    fn from(s: deltachat::email::backup::BackupStatus) -> Self {
        Self {
            last_backup: s.last_backup,
            stale: s.stale,
        }
    }
}
