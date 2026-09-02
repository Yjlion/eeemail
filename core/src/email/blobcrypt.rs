//! Per-blob encryption at rest.
//!
//! [ADR 0015] shipped database encryption and reported it as **partial**,
//! because the blobdir holds every attachment and the raw MIME of every
//! retained message in cleartext beside an encrypted database. Raw MIME
//! retention is eeemail's own addition, so that gap is one we widened. This
//! closes it. See [ADR 0020].
//!
//! # Where the key lives
//!
//! Not derived from the database passphrase, which is what [ADR 0020]
//! originally said. Core does not keep the passphrase after opening the
//! database -- SQLCipher holds it inside the connection and `Sql` retains only
//! whether one was used -- so deriving from it would mean patching upstream to
//! keep a secret in memory for the whole session, for no gain.
//!
//! Instead the key is 32 random bytes stored **in the database**, which
//! SQLCipher already encrypts. That gives the same property by a shorter route:
//! one secret, the passphrase, transitively protecting the blobs. Changing the
//! passphrase re-encrypts the database and the key rides along, so nothing has
//! to be re-encrypted on the disk.
//!
//! It also makes one thing impossible that would otherwise be a trap: blob
//! encryption **requires** an encrypted database. Storing the key in a
//! cleartext database would protect nothing while reporting that it did, which
//! is precisely the false belief [ADR 0015] refused to create. [`enable`]
//! refuses rather than pretending.
//!
//! # On-disk format
//!
//! ```text
//! "EEEBLOB1"  8 bytes   magic and version
//! nonce      24 bytes   random per blob
//! ciphertext  n bytes   XChaCha20-Poly1305, 16-byte tag appended
//! ```
//!
//! The magic is what makes [`read`] **transparent**: a file that does not start
//! with it is returned as-is. Every read path can therefore be switched to this
//! function unconditionally, and a mailbox part-way through a migration, or one
//! that never enabled encryption at all, reads correctly either way.
//!
//! # Dedup still hashes plaintext
//!
//! Content addressing is what makes two accounts receiving the same message
//! store one copy ([ADR 0004]). Hashing ciphertext with a random nonce would
//! give every copy a different name and quietly double disk usage, so
//! [`crate::blob::BlobObject::create_and_deduplicate`] hashes the plaintext and
//! encrypts afterwards. The cost is that identical blobs are visibly identical
//! by filename -- a metadata leak we accept for the property.
//!
//! [ADR 0004]: ../../../docs/adr/0004-local-store-and-raw-mime.md
//! [ADR 0015]: ../../../docs/adr/0015-at-rest-and-backup.md
//! [ADR 0020]: ../../../docs/adr/0020-blobdir-encryption.md

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;

use crate::config::Config;
use crate::context::Context;
use crate::log::warn;

/// Magic and version. Present on every encrypted blob and on nothing else.
const MAGIC: &[u8; 8] = b"EEEBLOB1";

const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = MAGIC.len() + NONCE_LEN;

/// Where the blob key is kept, inside the (encrypted) database.
const KEY_CONFIG: &str = "eeemail_blob_key";

/// Whether blob encryption is on, from [`Config::BlobEncryption`].
pub async fn is_enabled(context: &Context) -> Result<bool> {
    context.get_config_bool(Config::BlobEncryption).await
}

/// Reads the blob key, if one has been generated.
async fn key_of(context: &Context) -> Result<Option<Key>> {
    let Some(hex) = context.sql.get_raw_config(KEY_CONFIG).await? else {
        return Ok(None);
    };
    let bytes = hex_decode(&hex).context("blob key is not valid hex")?;
    ensure!(bytes.len() == 32, "blob key has the wrong length");
    Ok(Some(*Key::from_slice(&bytes)))
}

/// Generates the blob key if there is not one yet, and returns it.
async fn ensure_key(context: &Context) -> Result<Key> {
    if let Some(key) = key_of(context).await? {
        return Ok(key);
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    context
        .sql
        .set_raw_config(KEY_CONFIG, Some(&hex_encode(&bytes)))
        .await?;
    Ok(*Key::from_slice(&bytes))
}

/// Whether `data` is an encrypted blob.
///
/// The whole reason the format carries a magic: this is what lets every read
/// site call [`read`] unconditionally.
fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && data.starts_with(MAGIC)
}

fn seal(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new(key);
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|err| anyhow::anyhow!("cannot encrypt blob: {err}"))?;

    let mut out = Vec::with_capacity(HEADER_LEN.saturating_add(ciphertext.len()));
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn open(key: &Key, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(is_encrypted(data), "not an encrypted blob");
    let (header, ciphertext) = data.split_at(HEADER_LEN);
    let nonce = header.get(MAGIC.len()..).context("truncated blob header")?;
    let cipher = XChaCha20Poly1305::new(key);
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|err| anyhow::anyhow!("cannot decrypt blob: {err}"))
}

/// Reads a blob, decrypting it if it is encrypted.
///
/// **Transparent.** A cleartext blob is returned unchanged, so this is safe to
/// call from every read path whether or not encryption is on, and safe on a
/// blobdir part-way through [`enable`].
pub async fn read(context: &Context, path: &Path) -> Result<Vec<u8>> {
    let data = tokio::fs::read(path).await?;
    if !is_encrypted(&data) {
        return Ok(data);
    }
    let key = key_of(context)
        .await?
        .context("blob is encrypted but no key is available; unlock the database")?;
    open(&key, &data).with_context(|| format!("cannot decrypt {}", path.display()))
}

/// Encrypts a file in place, if encryption is on.
///
/// Called after a blob has been written and named. A no-op when encryption is
/// off or the file is already encrypted, so it is safe to call more than once.
pub async fn protect(context: &Context, path: &Path) -> Result<()> {
    if !is_enabled(context).await? {
        return Ok(());
    }
    let key = ensure_key(context).await?;
    convert(path, |data| {
        if is_encrypted(data) {
            Ok(None)
        } else {
            seal(&key, data).map(Some)
        }
    })
    .await
}

/// Rewrites one file through `f`, atomically.
///
/// `f` returning `None` means "already in the wanted state", and nothing is
/// written. The write goes to a temporary beside the target and is renamed over
/// it, which is what makes a crash leave the blob either wholly converted or
/// wholly not -- never half.
async fn convert(path: &Path, f: impl FnOnce(&[u8]) -> Result<Option<Vec<u8>>>) -> Result<()> {
    let data = tokio::fs::read(path).await?;
    let Some(out) = f(&data)? else {
        return Ok(());
    };
    let temp = temp_beside(path);
    tokio::fs::write(&temp, &out).await?;
    tokio::fs::rename(&temp, path).await?;
    Ok(())
}

fn temp_beside(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", rand::random::<u64>()));
    path.with_file_name(name)
}

/// Turns blob encryption on, and encrypts the blobs already on disk.
///
/// Encrypting only *new* blobs was considered and rejected: a mailbox whose
/// history stays in cleartext while tomorrow's mail is protected would let
/// [`super::vault::protection`] report `blobs_encrypted` with the interesting
/// bytes still readable, which is the false belief this whole area exists to
/// avoid. See [ADR 0020].
///
/// Requires an encrypted database, because that is what protects the key.
///
/// Resumable: rerunning after an interruption converts whatever is left, since
/// [`protect`] skips what is already done.
pub async fn enable(context: &Context) -> Result<usize> {
    if !context.sql.is_encrypted().await.unwrap_or(false) {
        bail!(
            "set a database passphrase first: the blob key is stored in the database, \
             so encrypting blobs without encrypting the database protects nothing"
        );
    }
    ensure_key(context).await?;
    context
        .set_config_bool(Config::BlobEncryption, true)
        .await?;
    let converted = migrate(context, true).await?;
    info!(context, "Encrypted {converted} blob(s) at rest.");
    Ok(converted)
}

/// Turns blob encryption off, and decrypts the blobs on disk.
///
/// The key is kept until every blob is back in cleartext; dropping it first
/// would make anything the pass had not reached yet unreadable forever.
pub async fn disable(context: &Context) -> Result<usize> {
    context
        .set_config_bool(Config::BlobEncryption, false)
        .await?;
    let converted = migrate(context, false).await?;
    context.sql.set_raw_config(KEY_CONFIG, None).await?;
    info!(context, "Decrypted {converted} blob(s).");
    Ok(converted)
}

/// Converts every blob in the blobdir in one direction. Returns how many changed.
async fn migrate(context: &Context, encrypt: bool) -> Result<usize> {
    let Some(key) = key_of(context).await? else {
        return Ok(0);
    };
    let blobdir = context.get_blobdir().to_path_buf();
    let mut entries = match tokio::fs::read_dir(&blobdir).await {
        Ok(entries) => entries,
        // A fresh account may not have a blobdir yet, which is not a failure.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err).context("cannot read the blobdir"),
    };

    let mut converted = 0usize;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !entry.file_type().await?.is_file() {
            continue;
        }
        // Leave our own half-written temporaries alone: a crashed pass may have
        // left one, and it is not a blob anything references.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains(".tmp-"))
        {
            continue;
        }

        let result = convert(&path, |data| match (encrypt, is_encrypted(data)) {
            (true, false) => seal(&key, data).map(Some),
            (false, true) => open(&key, data).map(Some),
            _ => Ok(None),
        })
        .await;

        match result {
            Ok(()) => converted = converted.saturating_add(1),
            // One unreadable file must not stop the pass and leave the rest of
            // the mailbox in a mixed state nobody asked for.
            Err(err) => warn!(context, "Cannot convert blob {}: {err:#}", path.display()),
        }
    }
    Ok(converted)
}

/// How many bytes in the blobdir are still cleartext.
///
/// What [`super::vault::protection`] reports, and the number that decides
/// whether at-rest protection is partial. Counted by reading each file's header
/// rather than trusting the setting, because the setting says what was asked
/// for and this says what is true.
pub async fn cleartext_bytes(context: &Context) -> Result<u64> {
    let blobdir = context.get_blobdir().to_path_buf();
    let mut entries = match tokio::fs::read_dir(&blobdir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err).context("cannot read the blobdir"),
    };

    let mut total = 0u64;
    let mut header = [0u8; HEADER_LEN];
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if !metadata.is_file() {
            continue;
        }
        let size = metadata.len();
        if size == 0 {
            continue;
        }
        // Only the header is read: this runs over a whole mailbox, and reading
        // every attachment to answer "is it encrypted" would make a settings
        // screen wait on the disk.
        let mut file = match tokio::fs::File::open(entry.path()).await {
            Ok(file) => file,
            Err(_) => continue,
        };
        use tokio::io::AsyncReadExt as _;
        let read = file.read(&mut header).await.unwrap_or(0);
        if !is_encrypted(header.get(..read).unwrap_or_default()) {
            total = total.saturating_add(size);
        }
    }
    Ok(total)
}

fn hex_encode(bytes: &[u8]) -> String {
    // Written without a dependency: this is 32 bytes, once.
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>> {
    ensure!(hex.len().is_multiple_of(2), "odd-length hex");
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair)?;
            Ok(u8::from_str_radix(s, 16)?)
        })
        .collect()
}

#[cfg(test)]
mod blobcrypt_tests;
