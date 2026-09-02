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
//! * [`set_passphrase`] and friends expose the capability, and every entry
//!   point says what it does and does not cover.
//!
//! **The blobdir gap is now closable.** [`super::blobcrypt`] encrypts every
//! blob under a key held in the encrypted database; it is opt-in and off by
//! default ([ADR 0020]). [`protection`] measures the blobdir rather than
//! reading the setting, so it reports the truth during and after a migration,
//! and reports `partial` for as long as any cleartext remains. Where blob
//! encryption is off, filesystem or full-disk encryption is still the honest
//! recommendation, and the summary still says so.
//!
//! [ADR 0020]: ../../../docs/adr/0020-blobdir-encryption.md
//!
//! [ADR 0004]: ../../../docs/adr/0004-local-store-and-raw-mime.md

use anyhow::{Context as _, Result};

use crate::context::Context;

/// What at-rest protection is actually in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protection {
    /// The SQLite database is encrypted with SQLCipher.
    pub database_encrypted: bool,

    /// Attachments and retained message sources are encrypted on disk.
    ///
    /// True only when blob encryption is on **and** no cleartext is left, so an
    /// interrupted migration reports `false` rather than a half-truth.
    ///
    /// Present as a field rather than left implicit so that a UI reading this
    /// struct cannot show "encrypted" without also having been handed the fact
    /// that half the data may not be. See
    /// `docs/adr/0020-blobdir-encryption.md`.
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
    // Measured from the files themselves rather than read off the setting. The
    // setting says what was asked for; this says what is true, and after an
    // interrupted migration those are different. It is also what stops this
    // struct ever claiming more than the disk supports.
    let cleartext_bytes = super::blobcrypt::cleartext_bytes(context).await?;
    Ok(Protection {
        database_encrypted,
        blobs_encrypted: super::blobcrypt::is_enabled(context).await? && cleartext_bytes == 0,
        cleartext_bytes,
        partial: database_encrypted && cleartext_bytes > 0,
    })
}

/// Sets, changes or removes the database passphrase.
///
/// An empty passphrase turns encryption off. On its own this protects the
/// database and **not** the blobdir; [`super::blobcrypt::enable`] is the other
/// half, and requires this to have been called first. Callers must surface
/// [`protection`] alongside this rather than presenting it as "encrypt my
/// mail".
///
/// The database must already be open.
///
/// # Why this is not just `change_passphrase`
///
/// Core's [`Context::change_passphrase`] is `PRAGMA rekey`, and SQLCipher's
/// rekey **only works on a database that is already encrypted**. Pointed at a
/// plaintext database it fails with "PRAGMA rekey can only be run on an
/// existing encrypted database", which meant the one operation a user actually
/// wants -- *encrypt my existing mailbox* -- was the one that did not work.
/// Upstream says as much in `Sql::change_passphrase`'s own docs and refers you
/// to import/export.
///
/// So crossing between encrypted and plaintext goes through
/// `sqlcipher_export()`, which writes a complete copy of the database under the
/// new key. The copy is built first and renamed over the original only once it
/// is complete, so an interruption leaves the mailbox as it was rather than
/// half-converted.
pub async fn set_passphrase(context: &Context, passphrase: &str) -> Result<()> {
    let encrypted_now = context.sql.is_encrypted().await.unwrap_or(false);
    let encrypted_after = !passphrase.is_empty();

    match (encrypted_now, encrypted_after) {
        // Already encrypted and staying that way: rekey in place, which is the
        // one thing SQLCipher's rekey is for and is far cheaper than a copy.
        (true, true) => context.change_passphrase(passphrase.to_string()).await,
        (false, false) => Ok(()),
        _ => rewrite_under_key(context, passphrase).await,
    }
}

/// Rewrites the whole database under a new key, or under none.
///
/// Works in both directions, and needs no knowledge of the *current*
/// passphrase: the connection is already open and keyed, so `sqlcipher_export`
/// reads through it.
async fn rewrite_under_key(context: &Context, passphrase: &str) -> Result<()> {
    let dbfile = context.sql.dbfile.clone();
    let target = dbfile.with_extension("db-converting");
    // A leftover from an interrupted run is stale by definition.
    tokio::fs::remove_file(&target).await.ok();

    let target_str = target
        .to_str()
        .context("database path is not valid unicode")?
        .to_string();
    let key = passphrase.to_string();

    context
        .sql
        .call_write(move |conn| {
            conn.execute(
                "ATTACH DATABASE ? AS eeemail_rekey KEY ?",
                (&target_str, &key),
            )
            .context("cannot attach the converted database")?;
            // Detached whatever happens: leaving it attached would hold the
            // file open and make the rename below fail on Windows.
            let exported = conn
                .query_row(
                    "SELECT sqlcipher_export('eeemail_rekey')",
                    [],
                    |_row| Ok(()),
                )
                .context("cannot copy the database under the new passphrase");
            let detached = conn
                .execute("DETACH DATABASE eeemail_rekey", [])
                .context("cannot detach the converted database");
            exported?;
            detached?;
            Ok(())
        })
        .await?;

    context.sql.close().await;

    // The copy is a complete database in its own right, so any write-ahead log
    // belonging to the old one is not just stale but actively dangerous: SQLite
    // would replay it over the new file.
    for suffix in ["-wal", "-shm"] {
        let mut side = dbfile.clone().into_os_string();
        side.push(suffix);
        tokio::fs::remove_file(std::path::PathBuf::from(side))
            .await
            .ok();
    }

    // The first irreversible step, and the last one. Everything before this
    // point can be abandoned without touching the user's mailbox.
    tokio::fs::rename(&target, &dbfile)
        .await
        .context("cannot replace the database with the converted copy")?;

    context
        .sql
        .open(context, passphrase.to_string())
        .await
        .context("converted the database but cannot reopen it")
}

#[cfg(test)]
mod vault_tests;
