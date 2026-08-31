//! Tests for raw MIME retention.
//!
//! Note on time: `SystemTime::shift` is process-global in `deltachat-time`
//! (see docs/testing.md), so these run under `cargo nextest`, which gives each
//! test its own process.

use anyhow::Result;

use super::*;
use crate::message::{Message, Viewtype};
use crate::receive_imf::receive_imf;
use crate::sql;
use crate::test_utils::TestContext;
use deltachat_time::SystemTimeTools as SystemTime;
use std::time::Duration;

const RAW: &[u8] = b"From: alice@example.org\r\n\
To: bob@example.net\r\n\
Subject: raw retention\r\n\
Message-ID: <raw-retention@example.org>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
Chat-Version: 1.0\r\n\
\r\n\
body text\r\n";

#[test]
fn test_retention_from_days() {
    assert_eq!(Retention::from_days(0), Retention::Disabled);
    assert_eq!(Retention::from_days(30), Retention::Days(30));
    assert_eq!(Retention::from_days(1), Retention::Days(1));
    assert_eq!(Retention::from_days(-1), Retention::Forever);
    // Any negative value means forever; -1 is merely canonical.
    assert_eq!(Retention::from_days(-999), Retention::Forever);
    // i32::MIN must not panic via unsigned negation.
    assert_eq!(Retention::from_days(i32::MIN), Retention::Forever);
}

#[test]
fn test_expires_at() {
    assert_eq!(Retention::Forever.expires_at(1000), None);
    assert_eq!(Retention::Days(1).expires_at(1000), Some(1000 + 86_400));
    // Disabled is treated as immediately expired, never as "keep forever".
    assert_eq!(Retention::Disabled.expires_at(1000), Some(1000));
    // A far-future retention must saturate rather than overflow.
    assert!(Retention::Days(u32::MAX).expires_at(i64::MAX - 1).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_default_is_thirty_days() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert_eq!(
        t.get_config_int(Config::RawMimeRetentionDays).await?,
        30,
        "default retention should cover transport plus a reply window"
    );
    assert_eq!(Retention::load(&t).await?, Retention::Days(30));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incoming_message_retains_byte_identical_mime() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let stored = load(&t, msg_id)
        .await?
        .expect("raw MIME should be retained");
    assert_eq!(
        stored, RAW,
        "retained bytes must be byte-identical to what arrived"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_message_retains_mime() -> Result<()> {
    let alice = TestContext::new_alice().await;
    // eeemail's default is opportunistic (ADR 0006); upstream core defaults
    // ForceEncryption to on, which would refuse to send to a keyless contact.
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("outgoing body".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    let stored = load(&alice, sent.sender_msg_id)
        .await?
        .expect("outgoing raw MIME should be retained");
    let text = String::from_utf8_lossy(&stored);
    assert!(
        text.starts_with("From:") || text.contains("Message-ID:"),
        "stored bytes should be the rendered message, got: {}",
        &text[..text.len().min(200)]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_retention_disabled_stores_nothing() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    t.set_config(Config::RawMimeRetentionDays, Some("0"))
        .await?;

    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    assert!(!is_retained(&t, msg_id).await?);
    assert!(load(&t, msg_id).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_expires_on_schedule() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    t.set_config(Config::RawMimeRetentionDays, Some("7"))
        .await?;

    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    assert!(is_retained(&t, msg_id).await?);

    // Six days on: still inside the window.
    SystemTime::shift(Duration::from_secs(6 * 86_400));
    assert_eq!(expire(&t).await?, 0);
    assert!(
        load(&t, msg_id).await?.is_some(),
        "must not expire before the configured retention elapses"
    );

    // Past seven days: gone.
    SystemTime::shift(Duration::from_secs(2 * 86_400));
    assert_eq!(expire(&t).await?, 1);
    assert!(!is_retained(&t, msg_id).await?);
    assert!(load(&t, msg_id).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_forever_never_expires() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    t.set_config(Config::RawMimeRetentionDays, Some("-1"))
        .await?;

    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    SystemTime::shift(Duration::from_secs(3650 * 86_400)); // ten years
    assert_eq!(expire(&t).await?, 0);
    assert_eq!(load(&t, msg_id).await?.as_deref(), Some(RAW));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_housekeeping_keeps_retained_blob_but_reclaims_expired() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    t.set_config(Config::RawMimeRetentionDays, Some("7"))
        .await?;

    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    let blobname: String = t
        .sql
        .query_get_value("SELECT blobname FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await?
        .unwrap();
    let blob_path = super::blob_path(&t, &blobname);
    assert!(blob_path.exists());

    // While retained, housekeeping must treat the blob as in use.
    sql::housekeeping(&t).await?;
    assert!(
        blob_path.exists(),
        "housekeeping deleted a blob that is still retained"
    );
    assert_eq!(load(&t, msg_id).await?.as_deref(), Some(RAW));

    // Once expired, the same pass should both drop the row and reclaim the
    // blob -- that is why expire() runs before remove_unused_files().
    SystemTime::shift(Duration::from_secs(8 * 86_400));
    sql::housekeeping(&t).await?;
    assert!(!is_retained(&t, msg_id).await?);
    assert!(
        !blob_path.exists(),
        "expired raw MIME blob should be reclaimed in the same housekeeping pass"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_load_is_none_after_delete() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    assert!(is_retained(&t, msg_id).await?);

    delete(&t, msg_id).await?;
    assert!(!is_retained(&t, msg_id).await?);
    assert!(load(&t, msg_id).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_load_tolerates_missing_blob() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let blobname: String = t
        .sql
        .query_get_value("SELECT blobname FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await?
        .unwrap();
    tokio::fs::remove_file(super::blob_path(&t, &blobname)).await?;

    // A row outliving its blob must read as "not retained", not as an error,
    // and the dangling row should be cleaned up.
    assert!(load(&t, msg_id).await?.is_none());
    assert!(!is_retained(&t, msg_id).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_identical_messages_share_one_blob() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let first = *received.msg_ids.last().unwrap();

    // Same bytes, different Message-ID, so it is a distinct message.
    let second_raw = RAW.to_vec();
    let second_raw = String::from_utf8(second_raw)?.replace(
        "<raw-retention@example.org>",
        "<raw-retention-2@example.org>",
    );
    let received2 = receive_imf(&t, second_raw.as_bytes(), false)
        .await?
        .unwrap();
    let second = *received2.msg_ids.last().unwrap();

    assert_ne!(first, second);

    // Storing the *same* bytes twice must deduplicate to one blob.
    store(&t, second, RAW).await?;
    let a: String = t
        .sql
        .query_get_value("SELECT blobname FROM raw_mime WHERE msg_id=?", (first,))
        .await?
        .unwrap();
    let b: String = t
        .sql
        .query_get_value("SELECT blobname FROM raw_mime WHERE msg_id=?", (second,))
        .await?
        .unwrap();
    assert_eq!(
        a, b,
        "identical bytes should be content-addressed to one blob"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_store_is_idempotent() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    // Re-storing must update in place, not violate the primary key.
    store(&t, msg_id, RAW).await?;
    store(&t, msg_id, b"different bytes entirely").await?;

    let rows: isize = t
        .sql
        .query_get_value("SELECT COUNT(*) FROM raw_mime WHERE msg_id=?", (msg_id,))
        .await?
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        load(&t, msg_id).await?.as_deref(),
        Some(&b"different bytes entirely"[..])
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_changing_retention_does_not_retroactively_expire() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    t.set_config(Config::RawMimeRetentionDays, Some("-1"))
        .await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    // Shortening the setting applies to future messages. Already-stored MIME
    // keeps the expiry it was given, so a user who shortens retention does not
    // silently destroy history they already have.
    t.set_config(Config::RawMimeRetentionDays, Some("1"))
        .await?;
    SystemTime::shift(Duration::from_secs(10 * 86_400));
    assert_eq!(expire(&t).await?, 0);
    assert!(load(&t, msg_id).await?.is_some());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_deleted_chat_message_drops_raw_mime() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    assert!(is_retained(&t, msg_id).await?);

    // The original must not outlive the message it belongs to.
    delete(&t, msg_id).await?;
    sql::housekeeping(&t).await?;
    assert!(load(&t, msg_id).await?.is_none());
    Ok(())
}
