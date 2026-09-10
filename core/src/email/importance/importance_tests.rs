//! Tests for message importance.

use anyhow::Result;

use super::*;
use crate::email::compose::{RecipientSet, send};
use crate::email::policy::EncryptionMode;
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};

/// A message from `addr` carrying whatever priority headers are given.
fn mail_with(addr: &str, mid: &str, headers: &str) -> Vec<u8> {
    format!(
        "From: <{addr}>\r\n\
         To: <alice@example.org>\r\n\
         Subject: hello\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 8 Sep 2026 10:00:00 +0000\r\n\
         {headers}\
         \r\n\
         body\r\n"
    )
    .into_bytes()
}

async fn recv_with(t: &TestContext, mid: &str, headers: &str) -> Result<MsgId> {
    t.allow_unencrypted().await?;
    let received = receive_imf(t, &mail_with("someone@example.net", mid, headers), false)
        .await?
        .unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

/// Sends one message at `importance` and returns what went on the wire.
async fn sent_with(importance: Importance) -> Result<String> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // A bare `TestContext` carries upstream's strict default and refuses to
    // send to somebody whose key we do not hold.
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    let bob_addr = bob.get_config(crate::config::Config::Addr).await?.unwrap();
    send(
        &alice,
        &RecipientSet {
            to: vec![bob_addr],
            ..Default::default()
        },
        "Subject",
        "the body",
        None,
        None,
        importance,
    )
    .await?;
    Ok(alice.pop_sent_msg().await.payload().to_string())
}

#[test]
fn test_an_unknown_word_does_not_stop_the_search() {
    // `None` means "this header told us nothing, try the next one" and
    // `Normal` means "this header said normal", which stops. A message with a
    // garbage `Importance` and a meaningful `X-Priority` is not normal.
    assert_eq!(parse_importance("nonsense"), None);
    assert_eq!(parse_importance("normal"), Some(Importance::Normal));
    assert_eq!(
        from_headers(Some("nonsense"), Some("1"), None),
        Importance::High
    );
    // But a header that *does* say normal wins over a lower-priority one.
    assert_eq!(
        from_headers(Some("normal"), Some("1"), None),
        Importance::Normal
    );
}

#[test]
fn test_x_priority_ignores_the_commentary() {
    assert_eq!(parse_x_priority("1 (Highest)"), Some(Importance::High));
    assert_eq!(parse_x_priority("2"), Some(Importance::High));
    assert_eq!(parse_x_priority("3 (Normal)"), Some(Importance::Normal));
    assert_eq!(parse_x_priority("5 (Lowest)"), Some(Importance::Low));
    assert_eq!(parse_x_priority("banana"), None);
}

#[test]
fn test_importance_wins_over_x_priority() {
    // Deliberate order: `Importance` is what a client writes when the user
    // ticks a box, `X-Priority` is often set by a mailer on their behalf.
    assert_eq!(
        from_headers(Some("low"), Some("1 (Highest)"), None),
        Importance::Low
    );
    assert_eq!(from_headers(None, None, Some("urgent")), Importance::High);
    assert_eq!(from_headers(None, None, None), Importance::Normal);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_normal_message_carries_no_priority_header() -> Result<()> {
    let payload = sent_with(Importance::Normal).await?;
    // This is the property that keeps every unmarked message byte-identical to
    // what upstream emits, and so keeps upstream's MIME tests passing.
    assert!(
        !payload.contains("Importance:") && !payload.contains("X-Priority:"),
        "an unmarked message grew a priority header:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_high_message_carries_both_headers() -> Result<()> {
    let payload = sent_with(Importance::High).await?;
    // Both, because clients read different ones: Outlook reads `Importance`,
    // most everything else reads `X-Priority`.
    assert!(
        payload.contains("Importance: high"),
        "no Importance header:\n{payload}"
    );
    assert!(
        payload.contains("X-Priority: 1 (Highest)"),
        "no X-Priority header:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_low_message_carries_both_headers() -> Result<()> {
    let payload = sent_with(Importance::Low).await?;
    assert!(payload.contains("Importance: low"), "{payload}");
    assert!(payload.contains("X-Priority: 5 (Lowest)"), "{payload}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incoming_priority_headers_are_recorded() -> Result<()> {
    let t = TestContext::new_alice().await;

    let high = recv_with(&t, "high@example.net", "Importance: high\r\n").await?;
    assert_eq!(of_msg(&t, high).await?, Importance::High);

    let by_priority = recv_with(&t, "xp@example.net", "X-Priority: 1 (Highest)\r\n").await?;
    assert_eq!(of_msg(&t, by_priority).await?, Importance::High);

    let low = recv_with(&t, "low@example.net", "Importance: low\r\n").await?;
    assert_eq!(of_msg(&t, low).await?, Importance::Low);

    // Nothing said, so nothing stored -- which is most mail.
    let plain = recv_with(&t, "plain@example.net", "").await?;
    assert_eq!(of_msg(&t, plain).await?, Importance::Normal);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_marking_normal_removes_the_row() -> Result<()> {
    let t = TestContext::new_alice().await;
    let msg_id = recv_with(&t, "one@example.net", "Importance: high\r\n").await?;
    assert_eq!(of_msg(&t, msg_id).await?, Importance::High);

    set(&t, msg_id, Importance::Normal).await?;
    assert_eq!(of_msg(&t, msg_id).await?, Importance::Normal);
    // Normal is the absence of a row, not a row saying zero: otherwise the
    // table grows a row per message for the state almost every message is in.
    let rows: Option<i64> = t
        .sql
        .query_row_optional(
            "SELECT COUNT(*) FROM msg_importance WHERE msg_id=?",
            (msg_id,),
            |row| row.get(0),
        )
        .await?;
    assert_eq!(rows, Some(0));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_marking_is_idempotent_and_reversible() -> Result<()> {
    let t = TestContext::new_alice().await;
    let msg_id = recv_with(&t, "one@example.net", "").await?;
    set(&t, msg_id, Importance::High).await?;
    set(&t, msg_id, Importance::High).await?;
    assert_eq!(of_msg(&t, msg_id).await?, Importance::High);
    set(&t, msg_id, Importance::Low).await?;
    assert_eq!(of_msg(&t, msg_id).await?, Importance::Low);
    Ok(())
}
