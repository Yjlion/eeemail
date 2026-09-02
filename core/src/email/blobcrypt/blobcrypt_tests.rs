//! Tests for per-blob encryption at rest.

use anyhow::Result;

use super::*;
use crate::blob::BlobObject;
use crate::test_utils::TestContext;

/// An account whose database is encrypted, which blob encryption requires.
async fn locked() -> Result<TestContext> {
    let t = TestContext::new_alice().await;
    super::super::vault::set_passphrase(&t, "correct horse battery staple").await?;
    Ok(t)
}

fn cleartext_files(context: &Context) -> Vec<Vec<u8>> {
    std::fs::read_dir(context.get_blobdir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter(|data| !data.starts_with(MAGIC))
        .collect()
}

#[test]
fn test_hex_round_trip() {
    let bytes: Vec<u8> = (0u8..=255).collect();
    assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes);
    assert!(hex_decode("abc").is_err(), "odd length must be rejected");
    assert!(hex_decode("zz").is_err(), "non-hex must be rejected");
}

#[test]
fn test_seal_open_round_trip() {
    let key = *Key::from_slice(&[7u8; 32]);
    let sealed = seal(&key, b"hello").unwrap();
    assert!(is_encrypted(&sealed));
    assert!(
        !sealed.windows(5).any(|w| w == b"hello"),
        "plaintext leaked"
    );
    assert_eq!(open(&key, &sealed).unwrap(), b"hello");
}

#[test]
fn test_a_wrong_key_does_not_decrypt() {
    let sealed = seal(Key::from_slice(&[7u8; 32]), b"hello").unwrap();
    assert!(open(Key::from_slice(&[8u8; 32]), &sealed).is_err());
}

#[test]
fn test_nonces_differ_between_blobs() {
    let key = *Key::from_slice(&[7u8; 32]);
    // Identical plaintext under a reused nonce would leak their equality and,
    // worse, their xor. Each blob gets its own.
    let a = seal(&key, b"same").unwrap();
    let b = seal(&key, b"same").unwrap();
    assert_ne!(a, b);
}

#[test]
fn test_cleartext_is_recognised_as_cleartext() {
    assert!(!is_encrypted(b""));
    assert!(!is_encrypted(b"From: alice@example.org"));
    // A file that happens to start with the magic but is too short is not a
    // valid blob, and must not be treated as one.
    assert!(!is_encrypted(MAGIC));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blob_encryption_ships_off() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert!(!is_enabled(&t).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_enabling_requires_an_encrypted_database() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Storing the key in a cleartext database would protect nothing while
    // reporting that it did, which is the false belief ADR 0015 refused to
    // create.
    let err = enable(&t).await.unwrap_err().to_string();
    assert!(err.contains("passphrase"), "unhelpful error: {err}");
    assert!(!is_enabled(&t).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_existing_blobs_are_encrypted_when_enabled() -> Result<()> {
    let t = locked().await?;
    let blob = BlobObject::create_and_deduplicate_from_bytes(&t, b"secret mail", "a.eml")?;
    let path = blob.to_abs_path();
    assert_eq!(std::fs::read(&path)?, b"secret mail", "not yet encrypted");

    // Encrypting only *new* blobs would leave the interesting bytes readable
    // while `protection()` claimed otherwise.
    enable(&t).await?;
    let on_disk = std::fs::read(&path)?;
    assert!(is_encrypted(&on_disk));
    assert!(!on_disk.windows(6).any(|w| w == b"secret"));

    // And it still reads back.
    assert_eq!(read(&t, &path).await?, b"secret mail");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_new_blobs_are_encrypted_on_write() -> Result<()> {
    let t = locked().await?;
    enable(&t).await?;

    let blob = BlobObject::create_and_deduplicate_from_bytes(&t, b"later mail", "b.eml")?;
    let path = blob.to_abs_path();
    assert!(is_encrypted(&std::fs::read(&path)?));
    assert_eq!(read(&t, &path).await?, b"later mail");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dedup_still_hashes_plaintext() -> Result<()> {
    let t = locked().await?;
    enable(&t).await?;

    // Two copies of the same message must still be one file. Hashing ciphertext
    // with a random nonce would give them different names and double the disk.
    let first = BlobObject::create_and_deduplicate_from_bytes(&t, b"identical", "c.eml")?;
    let second = BlobObject::create_and_deduplicate_from_bytes(&t, b"identical", "c.eml")?;
    assert_eq!(first.as_name(), second.as_name());
    assert_eq!(read(&t, &first.to_abs_path()).await?, b"identical");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reading_is_transparent_on_a_mixed_blobdir() -> Result<()> {
    let t = locked().await?;
    let plain = BlobObject::create_and_deduplicate_from_bytes(&t, b"before", "d.eml")?;
    enable(&t).await?;
    let sealed = BlobObject::create_and_deduplicate_from_bytes(&t, b"after", "e.eml")?;

    // Every read path calls this unconditionally, so it has to be right for a
    // blobdir part-way through a migration as well as one that never had one.
    assert_eq!(read(&t, &plain.to_abs_path()).await?, b"before");
    assert_eq!(read(&t, &sealed.to_abs_path()).await?, b"after");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_disable_puts_everything_back() -> Result<()> {
    let t = locked().await?;
    let blob = BlobObject::create_and_deduplicate_from_bytes(&t, b"round trip", "f.eml")?;
    enable(&t).await?;
    disable(&t).await?;

    assert!(!is_enabled(&t).await?);
    assert_eq!(std::fs::read(blob.to_abs_path())?, b"round trip");
    assert!(cleartext_files(&t).iter().any(|d| d == b"round trip"));
    // The key is only dropped once nothing needs it any more.
    assert!(key_of(&t).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_enable_is_resumable() -> Result<()> {
    let t = locked().await?;
    let a = BlobObject::create_and_deduplicate_from_bytes(&t, b"one", "g.eml")?;
    enable(&t).await?;
    let b = BlobObject::create_and_deduplicate_from_bytes(&t, b"two", "h.eml")?;

    // Standing in for a pass that was interrupted after some files: rerunning
    // must finish the job and must not double-encrypt what it already did.
    let converted = migrate(&t, true).await?;
    assert!(converted >= 2);
    assert_eq!(read(&t, &a.to_abs_path()).await?, b"one");
    assert_eq!(read(&t, &b.to_abs_path()).await?, b"two");
    // Double-encrypting would leave the magic doubled and the plaintext wrong.
    assert_eq!(
        open(
            &key_of(&t).await?.unwrap(),
            &std::fs::read(a.to_abs_path())?
        )?,
        b"one"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_protection_reports_the_truth() -> Result<()> {
    let t = locked().await?;
    BlobObject::create_and_deduplicate_from_bytes(&t, b"some mail", "i.eml")?;

    let before = super::super::vault::protection(&t).await?;
    assert!(before.database_encrypted);
    assert!(!before.blobs_encrypted);
    assert!(before.partial, "cleartext in the blobdir means partial");
    assert!(before.cleartext_bytes > 0);

    enable(&t).await?;
    let after = super::super::vault::protection(&t).await?;
    assert!(after.blobs_encrypted);
    assert_eq!(after.cleartext_bytes, 0);
    assert!(!after.partial);
    assert_eq!(
        after.summary(),
        "Database encrypted; no cleartext files remain."
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_corrupt_blob_is_unavailable_not_fatal() -> Result<()> {
    let t = locked().await?;
    let good = BlobObject::create_and_deduplicate_from_bytes(&t, b"intact", "j.eml")?;
    let bad = BlobObject::create_and_deduplicate_from_bytes(&t, b"damaged", "k.eml")?;
    enable(&t).await?;

    // Flip a byte of the ciphertext. AEAD must refuse it.
    let path = bad.to_abs_path();
    let mut data = std::fs::read(&path)?;
    let last = data.len() - 1;
    data[last] ^= 0xff;
    std::fs::write(&path, &data)?;

    assert!(read(&t, &path).await.is_err());
    // One corrupt attachment must not make the mailbox unopenable.
    assert_eq!(read(&t, &good.to_abs_path()).await?, b"intact");
    Ok(())
}
