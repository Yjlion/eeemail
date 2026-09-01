//! Tests for at-rest protection reporting.

use anyhow::Result;

use super::*;
use crate::test_utils::TestContext;

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(2048), "2.0 KB");
    assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    // Must not overflow or produce a unit we do not have.
    assert!(format_bytes(u64::MAX).ends_with("GB"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unencrypted_account_says_so_plainly() -> Result<()> {
    let t = TestContext::new_alice().await;
    let protection = protection(&t).await?;
    assert!(!protection.database_encrypted);
    assert!(!protection.partial);
    assert!(protection.summary().contains("Not encrypted at rest"));
    assert!(
        protection.summary().contains("full-disk"),
        "the recommendation has to be in the message the user actually reads"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blobs_are_never_reported_as_encrypted() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Not a placeholder: this is the whole reason the module exists. Nothing in
    // this codebase encrypts the blobdir, so nothing may report that it does.
    assert!(!protection(&t).await?.blobs_encrypted);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_partial_protection_is_named() -> Result<()> {
    let t = TestContext::new_alice().await;
    // An encrypted database with cleartext blobs beside it is the state most
    // likely to be misread as "my mail is safe".
    let partial = Protection {
        database_encrypted: true,
        blobs_encrypted: false,
        cleartext_bytes: 5 * 1024 * 1024,
        partial: true,
    };
    let summary = partial.summary();
    assert!(summary.contains("5.0 MB"));
    assert!(summary.contains("original message"));
    assert!(
        !summary.starts_with("Database encrypted;"),
        "a partial state must not read like a complete one"
    );
    let _ = t;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cleartext_bytes_counts_retained_raw_mime() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let before = protection(&t).await?.cleartext_bytes;

    let raw = b"From: alice@example.org\r\n\
To: bob@example.net\r\n\
Subject: a secret\r\n\
Message-ID: <vault-1@example.org>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
\r\n\
this body is in the blobdir in cleartext\r\n";
    crate::receive_imf::receive_imf(&t, raw, false)
        .await?
        .unwrap();

    let after = protection(&t).await?.cleartext_bytes;
    assert!(
        after > before,
        "retained raw MIME lands in the blobdir, which is exactly the exposure \
         this report exists to name"
    );
    Ok(())
}
