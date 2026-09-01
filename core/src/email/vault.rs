//! At-rest protection, and an honest account of what it does not cover.
//!
//! Core links `rusqlite` with `bundled-sqlcipher-vendored-openssl` and applies
//! `PRAGMA key` when a passphrase is set, so encrypting the database costs no
//! engine work. Upstream nonetheless **deprecated** it in 2025-11, and the
//! reason matters more to us than it does to them:
//!
//! > Db encryption does nothing with blobs, so fs/disk encryption is
//! > recommended.
//!
//! For Delta Chat that leaves attachments in the clear. For eeemail it leaves
//! attachments **and the raw MIME of every retained message** in the clear --
//! complete original messages, headers, subjects and bodies, sitting in the
//! blobdir next to an encrypted database. Raw MIME retention is eeemail's own
//! addition ([ADR 0004]), so we made this gap wider than upstream's.
//!
//! Shipping "at-rest encryption" on those terms would be worse than shipping
//! nothing: a user who turns it on would believe their mail is protected when
//! the most sensitive part of it is a `cat` away.
//!
//! So this module does two things and refuses to pretend:
//!
//! * [`protection`] reports what is *actually* protected, including how many
//!   bytes of cleartext are in the blobdir, so a UI can tell the truth.
//! * [`set_passphrase`] and friends expose the capability for users who want
//!   the database encrypted as one layer among several, and every entry point
//!   is documented as partial.
//!
//! Full at-rest protection needs the blobdir encrypted too. That is real work
//! -- per-blob AEAD, nonce management, key derivation, and a migration for
//! existing blobs -- and is tracked as its own piece rather than half-done
//! here. Until it lands, filesystem or full-disk encryption is the honest
//! recommendation, and [`protection`] says so.
//!
//! [ADR 0004]: ../../../docs/adr/0004-local-store-and-raw-mime.md

use anyhow::Result;

use crate::context::Context;

/// What at-rest protection is actually in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    /// The SQLite database is encrypted with SQLCipher.
    pub database_encrypted: bool,

    /// The blobdir is **never** encrypted by this module. Always `false`.
    ///
    /// Present as a field rather than left implicit so that a UI reading this
    /// struct cannot show "encrypted" without also having been handed the fact
    /// that half the data is not.
    pub blobs_encrypted: bool,

    /// Bytes of cleartext sitting in the blobdir: attachments, avatars, and the
    /// retained raw MIME of every message.
    pub cleartext_bytes: u64,

    /// True when the database is encrypted but the blobdir still holds
    /// cleartext, which is the state most likely to be misread as "my mail is
    /// encrypted at rest".
    pub partial: bool,
}

impl Protection {
    /// A sentence a UI can show verbatim.
    pub fn summary(&self) -> String {
        match (self.database_encrypted, self.partial) {
            (false, _) => {
                "Not encrypted at rest. Use filesystem or full-disk encryption.".to_string()
            }
            (true, true) => format!(
                "Database encrypted, but {} of attachments and original message \
                 sources remain in cleartext. Use filesystem or full-disk encryption \
                 for complete protection.",
                format_bytes(self.cleartext_bytes)
            ),
            (true, false) => "Database encrypted; no cleartext files remain.".to_string(),
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    // Written without arithmetic on the index or indexing by it: the crate
    // denies both, and a units table is exactly where an off-by-one silently
    // mislabels a number the user is meant to act on.
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut units = UNITS.iter();
    let mut unit = units.next().copied().unwrap_or("B");
    let mut scaled = false;
    for next in units {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
        scaled = true;
    }
    if scaled {
        format!("{value:.1} {unit}")
    } else {
        format!("{bytes} {unit}")
    }
}

/// Reports what at-rest protection is in force.
///
/// Deliberately reports the blobdir gap rather than leaving a caller to
/// discover it: a settings screen that renders this faithfully cannot claim
/// more than is true.
pub async fn protection(context: &Context) -> Result<Protection> {
    let database_encrypted = context.sql.is_encrypted().await.unwrap_or(false);
    let cleartext_bytes = crate::storage_usage::get_blobdir_storage_usage(context);
    Ok(Protection {
        database_encrypted,
        blobs_encrypted: false,
        cleartext_bytes,
        partial: database_encrypted && cleartext_bytes > 0,
    })
}

/// Changes the database passphrase, enabling encryption if it was off.
///
/// An empty passphrase turns encryption off. **Partial protection only**: see
/// the module docs. Callers must surface [`protection`] alongside this, not
/// present it as "encrypt my mail".
///
/// The database must already be open.
pub async fn set_passphrase(context: &Context, passphrase: &str) -> Result<()> {
    context.change_passphrase(passphrase.to_string()).await
}

#[cfg(test)]
mod vault_tests;
