//! Encryption policy and server retention.
//!
//! Two settings decide what leaves the device and what is left behind on the
//! server. They are here together because they are the two questions a user
//! actually asks about privacy, and because both are read on the same paths.
//!
//! # Encryption strictness
//!
//! Upstream core defaults `ForceEncryption` to on: unencrypted mail is neither
//! fetched nor accepted, and sending to a correspondent with no key fails. That
//! is right for a chatmail deployment and wrong as a default for an email
//! client, whose user has correspondents who have never heard of OpenPGP.
//!
//! So we offer the three modes of [`EncryptionMode`], defaulting to
//! opportunistic, which is what original Delta Chat did. `ForceEncryption`
//! stays authoritative for strict-vs-not, so our setting and core's can never
//! disagree about the security-relevant bit; [`Config::EncryptionMode`] only
//! distinguishes the two non-strict modes.
//!
//! # Per-contact overrides compose toward the strictest
//!
//! A message addressed to several people takes the strictest mode among the
//! global setting and every recipient's override. A per-contact setting is a
//! statement about that correspondent, and one correspondent you have marked
//! strict must not be sent cleartext because someone else on the message is
//! lenient.
//!
//! # The silent recipient drop
//!
//! When a message goes out encrypted, core removes recipients whose key is
//! missing from the envelope and sends to the rest:
//!
//! ```text
//! recipients.retain(|addr| !missing_key_addresses.contains(addr));
//! ```
//!
//! In a group chat that is defensible -- membership is the chat's own state. In
//! email it means you address a message to three people, one of them never
//! receives it, and nothing says so.
//!
//! We do not change that behaviour here: it is woven through group handling and
//! upstream's tests depend on it. Instead [`record_undelivered`] compares what
//! the message was addressed to against what was actually sent, records the
//! difference in `msg_undelivered`, and warns. A client can then say "Dave did
//! not receive this" instead of leaving the user to find out from Dave.
//!
//! See `docs/adr/0006-encryption-policy.md`.

use anyhow::{Result, ensure};

use crate::config::Config;
use crate::contact::ContactId;
use crate::context::Context;
use crate::message::MsgId;
use crate::mimeparser::SystemMessage;
use crate::param::Param;
use crate::tools::time;

/// How strictly to encrypt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i64)]
pub enum EncryptionMode {
    /// End-to-end only. Cleartext is neither sent nor accepted.
    ///
    /// Ordered first because [`Ord`] is used to compose overrides, and the
    /// strictest must win.
    Strict = 0,

    /// Encrypt whenever the keys are there, and say so when they are not.
    ///
    /// The default, and what original Delta Chat did.
    Opportunistic = 1,

    /// Prefer delivery. Rather than send encrypted to some recipients and
    /// silently drop the rest, send to everyone in cleartext.
    Lenient = 2,
}

impl EncryptionMode {
    fn from_i64(value: i64) -> Self {
        match value {
            0 => EncryptionMode::Strict,
            2 => EncryptionMode::Lenient,
            // Anything unrecognized is treated as the default rather than as
            // the most permissive mode.
            _ => EncryptionMode::Opportunistic,
        }
    }

    /// Reads the global mode.
    ///
    /// `ForceEncryption` wins on strict-vs-not: it is core's own setting, it is
    /// device-synced, and another client may have changed it.
    pub async fn load(context: &Context) -> Result<Self> {
        if context.get_config_bool(Config::ForceEncryption).await? {
            return Ok(EncryptionMode::Strict);
        }
        let mode = Self::from_i64(context.get_config_int(Config::EncryptionMode).await?.into());
        Ok(match mode {
            // Strict here but ForceEncryption off: core's setting is
            // authoritative, so fall back to the default rather than claim a
            // strictness we are not enforcing.
            EncryptionMode::Strict => EncryptionMode::Opportunistic,
            other => other,
        })
    }

    /// Sets the global mode, keeping `ForceEncryption` in step.
    pub async fn set(context: &Context, mode: EncryptionMode) -> Result<()> {
        context
            .set_config_bool(Config::ForceEncryption, mode == EncryptionMode::Strict)
            .await?;
        context
            .set_config(Config::EncryptionMode, Some(&(mode as i64).to_string()))
            .await?;
        Ok(())
    }

    /// Reads a contact's override, if it has one.
    pub async fn for_contact(context: &Context, id: ContactId) -> Result<Option<Self>> {
        // Two levels of `Option`: the outer is "no row", the inner is "row with
        // a NULL override". Both mean "follow the global setting", but
        // `query_get_value` only handles the outer one and errors on the NULL.
        let value: Option<Option<i64>> = context
            .sql
            .query_row_optional(
                "SELECT encryption_mode FROM contact_policy WHERE contact_id=?",
                (id,),
                |row| row.get::<_, Option<i64>>(0),
            )
            .await?;
        Ok(value.flatten().map(Self::from_i64))
    }

    /// Sets or clears a contact's override.
    pub async fn set_for_contact(
        context: &Context,
        id: ContactId,
        mode: Option<Self>,
    ) -> Result<()> {
        match mode {
            Some(mode) => {
                context
                    .sql
                    .execute(
                        "INSERT INTO contact_policy (contact_id, encryption_mode) VALUES (?1, ?2)
                         ON CONFLICT(contact_id) DO UPDATE SET encryption_mode=excluded.encryption_mode",
                        (id, mode as i64),
                    )
                    .await?;
            }
            None => {
                context
                    .sql
                    .execute(
                        "UPDATE contact_policy SET encryption_mode=NULL WHERE contact_id=?",
                        (id,),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    /// The mode that applies to a message addressed to `contacts`: the
    /// strictest of the global setting and every override.
    pub async fn effective(context: &Context, contacts: &[ContactId]) -> Result<Self> {
        let mut mode = Self::load(context).await?;
        for &id in contacts {
            if let Some(override_mode) = Self::for_contact(context, id).await? {
                mode = mode.min(override_mode);
            }
        }
        Ok(mode)
    }
}

/// Applies eeemail's defaults to a freshly created account.
///
/// Only the ones that differ from upstream's, and only for an account that has
/// not been configured yet. Two rules make this safe:
///
/// * **Never touch a configured account.** An existing Delta Chat profile
///   opened with eeemail keeps its settings. `ForceEncryption` is device-synced,
///   so writing it here would propagate a weaker policy to the user's other
///   clients -- a silent security downgrade we have no business making.
/// * **Never overwrite an explicit choice.** Only values the user has never set
///   are filled in.
///
/// This runs at setup rather than as a compile-time default because changing
/// `ForceEncryption`'s default in `config.rs` breaks 22 upstream tests that
/// assert upstream's policy. Carrying those patches forever is a poor trade for
/// a value that can be written once. The consequence is that a bare `Context`
/// opened by something other than eeemail keeps upstream's strict default;
/// every eeemail entry point calls this.
pub async fn apply_defaults(context: &Context) -> Result<()> {
    if context.is_configured().await? {
        return Ok(());
    }
    if context
        .get_config_bool_opt(Config::ForceEncryption)
        .await?
        .is_none()
    {
        // Opportunistic: an email client has correspondents who have never
        // heard of OpenPGP, and refusing to talk to them is the difference
        // between an email client and a closed messenger.
        EncryptionMode::set(context, EncryptionMode::Opportunistic).await?;
    }
    if context
        .get_config_bool_opt(Config::InboxGating)
        .await?
        .is_none()
    {
        // On by default for eeemail, off in upstream's compile-time default: a
        // gate the user has to find and enable protects only the people who
        // already knew to look for it. See docs/adr/0018-contact-gating.md.
        context.set_config_bool(Config::InboxGating, true).await?;
    }
    if context
        .get_config_opt(Config::EphemeralTrashDays)
        .await?
        .is_none()
    {
        // A fired timer moves the message to `Trash` and leaves it readable,
        // rather than destroying it. See
        // docs/adr/0019-recoverable-ephemeral-expiry.md.
        context
            .set_config(
                Config::EphemeralTrashDays,
                Some(&super::ephemeral::DEFAULT_PURGE_DAYS.to_string()),
            )
            .await?;
    }
    if context
        .get_config_bool_opt(Config::SubjectInBody)
        .await?
        .is_none()
    {
        // Upstream prepends the subject into the body of classic mail, which
        // suits a chat bubble with no subject line and corrupts the body of a
        // message an email client displays the subject of separately.
        context
            .set_config_bool(Config::SubjectInBody, false)
            .await?;
    }
    Ok(())
}

/// Applies the effective encryption mode to a message about to be sent.
///
/// Deliberately does as little as possible. When the effective mode equals the
/// global one, core already enforces it through `ForceEncryption`, and it does
/// so *better* than we could: `create_send_msg_jobs` produces a message the
/// user can act on ("your provider requires end-to-end encryption which is not
/// set up yet") and adds an `InvalidUnencryptedMail` info message to the chat.
///
/// Setting `Param::GuaranteeE2ee` here regardless would pre-empt all of that:
/// `MimeFactory` would bail first with "No recipient keys are available", and
/// the info message would never appear. It also broke legacy SecureJoin, whose
/// handshake messages are deliberately unencrypted. So we only step in where
/// core would otherwise do the wrong thing for us.
///
/// Returns the mode that was applied.
pub(crate) async fn prepare_send(
    context: &Context,
    msg: &mut crate::message::Message,
    contacts: &[ContactId],
) -> Result<EncryptionMode> {
    let global = EncryptionMode::load(context).await?;
    let mode = EncryptionMode::effective(context, contacts).await?;

    match mode {
        // Global strictness is core's job; see above.
        EncryptionMode::Strict if global == EncryptionMode::Strict => {}

        // Strict only because a per-contact override says so. Core has no
        // notion of that, so enforce it here.
        EncryptionMode::Strict => {
            let missing = missing_keys(context, contacts).await?;
            ensure!(
                missing.is_empty(),
                "cannot send: {missing:?} have no key, and this conversation is set to \
                 end-to-end only"
            );
            // Every recipient has a key, so this cannot trip MimeFactory's
            // "no recipient keys" bail. It just pins this message to encrypted
            // even though the global setting would allow cleartext.
            msg.param.set_int(Param::GuaranteeE2ee, 1);
        }

        EncryptionMode::Opportunistic => {}

        EncryptionMode::Lenient => {
            // Only fall back to cleartext when encrypting would actually lose
            // someone. If everyone has a key, encrypt: lenient means "prefer
            // delivery", not "prefer plaintext".
            //
            // Never for system messages. SecureJoin handshakes and group
            // membership updates have their own encryption rules, and
            // overriding them would break the protocol rather than a
            // preference.
            if msg.param.get_cmd() == SystemMessage::Unknown
                && !missing_keys(context, contacts).await?.is_empty()
            {
                msg.param.set_int(Param::ForcePlaintext, 1);
            }
        }
    }
    Ok(mode)
}

/// Addresses among `contacts` for which we hold no public key.
pub async fn missing_keys(context: &Context, contacts: &[ContactId]) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for &id in contacts {
        if id == ContactId::SELF {
            continue;
        }
        let row: Option<(String, Option<String>)> = context
            .sql
            .query_row_optional(
                "SELECT addr, fingerprint FROM contacts WHERE id=?",
                (id,),
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .await?;
        if let Some((addr, fingerprint)) = row
            && fingerprint.is_none_or(|f| f.is_empty())
        {
            missing.push(addr);
        }
    }
    Ok(missing)
}

/// Records recipients a message was addressed to but not sent to.
///
/// `addressed` is the `To` header, `sent_to` the SMTP envelope. When a message
/// goes out encrypted, core drops recipients whose key is missing from the
/// envelope but leaves them in the header, so the difference is exactly who
/// will never see it.
pub(crate) async fn record_undelivered(
    context: &Context,
    msg_id: MsgId,
    addressed: &[String],
    sent_to: &[String],
) -> Result<Vec<String>> {
    let sent: std::collections::HashSet<String> =
        sent_to.iter().map(|a| a.to_lowercase()).collect();
    let dropped: Vec<String> = addressed
        .iter()
        .filter(|addr| !sent.contains(&addr.to_lowercase()))
        .cloned()
        .collect();
    if dropped.is_empty() {
        return Ok(dropped);
    }

    let rows = dropped.clone();
    context
        .sql
        .transaction(move |transaction| {
            for addr in &rows {
                transaction.execute(
                    "INSERT INTO msg_undelivered (msg_id, addr) VALUES (?1, ?2)
                     ON CONFLICT(msg_id, addr) DO NOTHING",
                    (msg_id, addr),
                )?;
            }
            Ok(())
        })
        .await?;
    info!(
        context,
        "Message {msg_id} was addressed to {dropped:?} but not sent to them: no key, \
         and the message went out encrypted. Recorded in `msg_undelivered`."
    );
    Ok(dropped)
}

/// Recipients a message was addressed to but never sent to.
pub async fn undelivered(context: &Context, msg_id: MsgId) -> Result<Vec<String>> {
    context
        .sql
        .query_map_vec(
            "SELECT addr FROM msg_undelivered WHERE msg_id=? ORDER BY addr",
            (msg_id,),
            |row| Ok(row.get::<_, String>(0)?),
        )
        .await
}

/// The cryptographic standing of one message, as a client should show it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageCrypto {
    /// The message was end-to-end encrypted.
    pub encrypted: bool,

    /// The message carried a valid signature from a key we hold.
    pub signed: bool,

    /// The signing key belongs to a contact verified through SecureJoin.
    ///
    /// This is the only one of the three that survives an active attacker who
    /// can substitute keys, so it is the only one worth a strong indicator.
    pub verified: bool,
}

/// Reads the cryptographic standing of a message.
pub async fn message_crypto(context: &Context, msg_id: MsgId) -> Result<MessageCrypto> {
    let msg = crate::message::Message::load_from_db(context, msg_id).await?;
    let encrypted = msg.get_showpadlock();

    let verified = if encrypted {
        crate::contact::Contact::get_by_id(context, msg.get_from_id())
            .await?
            .is_verified(context)
            .await?
    } else {
        false
    };

    Ok(MessageCrypto {
        encrypted,
        // Core discards a message whose signature does not check out, so
        // anything that arrived encrypted and was stored is also signed. There
        // is no separate signed-but-unencrypted state to report.
        signed: encrypted,
        verified,
    })
}

// ---------------------------------------------------------------------------
// Server retention
// ---------------------------------------------------------------------------

/// How long a downloaded message is left on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRetention {
    /// Delete as soon as the message is safely stored locally. The default.
    DeleteAfterDownload,

    /// Keep on the server for this many days.
    Days(u32),

    /// Never delete. The coexistence mode, for an account also read by another
    /// client.
    Never,
}

impl ServerRetention {
    /// Reads the configured retention.
    pub async fn load(context: &Context) -> Result<Self> {
        Ok(Self::from_days(
            context.get_config_int(Config::ServerRetentionDays).await?,
        ))
    }

    fn from_days(days: i32) -> Self {
        match days {
            0 => ServerRetention::DeleteAfterDownload,
            d if d < 0 => ServerRetention::Never,
            d => ServerRetention::Days(d.unsigned_abs()),
        }
    }

    /// Sets the retention.
    pub async fn set(context: &Context, retention: Self) -> Result<()> {
        let days = match retention {
            ServerRetention::DeleteAfterDownload => 0,
            ServerRetention::Never => -1,
            ServerRetention::Days(d) => i32::try_from(d).unwrap_or(i32::MAX),
        };
        context
            .set_config(Config::ServerRetentionDays, Some(&days.to_string()))
            .await?;
        Ok(())
    }
}

/// Applies the retention policy to a message that has just been received.
///
/// Deletion goes through core's existing mechanism: setting `imap.target` to
/// the empty string is what makes the IMAP loop mark the message `\Deleted`. We
/// only decide *when*.
///
/// Deliberately driven from the receive path, so the policy applies to messages
/// as they arrive and never retroactively. Pointing eeemail at an existing
/// mailbox therefore cannot destroy mail that was already there, whatever the
/// setting says.
pub(crate) async fn apply_server_retention(context: &Context, rfc724_mid: &str) -> Result<()> {
    if rfc724_mid.is_empty() {
        return Ok(());
    }
    match ServerRetention::load(context).await? {
        ServerRetention::Never => {}
        ServerRetention::DeleteAfterDownload => {
            delete_from_server(context, rfc724_mid).await?;
        }
        ServerRetention::Days(days) => {
            let delete_at = time().saturating_add(i64::from(days).saturating_mul(86_400));
            context
                .sql
                .execute(
                    "INSERT INTO server_retention (rfc724_mid, delete_at) VALUES (?1, ?2)
                     ON CONFLICT(rfc724_mid) DO NOTHING",
                    (rfc724_mid, delete_at),
                )
                .await?;
        }
    }
    Ok(())
}

async fn delete_from_server(context: &Context, rfc724_mid: &str) -> Result<()> {
    let affected = context
        .sql
        .execute(
            "UPDATE imap SET target='' WHERE rfc724_mid=?",
            (rfc724_mid,),
        )
        .await?;
    if affected > 0 {
        context.scheduler.interrupt_inbox().await;
    }
    Ok(())
}

/// Deletes messages from the server whose retention has elapsed. Returns how
/// many were scheduled for deletion.
///
/// Called from housekeeping.
pub(crate) async fn expire_on_server(context: &Context) -> Result<usize> {
    let due: Vec<String> = context
        .sql
        .query_map_vec(
            "SELECT rfc724_mid FROM server_retention WHERE delete_at <= ?",
            (time(),),
            |row| Ok(row.get::<_, String>(0)?),
        )
        .await?;
    if due.is_empty() {
        return Ok(0);
    }
    for rfc724_mid in &due {
        delete_from_server(context, rfc724_mid).await?;
        context
            .sql
            .execute(
                "DELETE FROM server_retention WHERE rfc724_mid=?",
                (rfc724_mid,),
            )
            .await?;
    }
    Ok(due.len())
}

/// Removes rows for messages that no longer exist locally.
pub(crate) async fn prune(context: &Context) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM msg_undelivered WHERE msg_id NOT IN \
             (SELECT id FROM msgs WHERE chat_id!=?)",
            (crate::constants::DC_CHAT_ID_TRASH,),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod policy_tests;
