//! Tests for per-message recipient sets.

use anyhow::Result;

use super::*;
use crate::config::Config;
use crate::message::{Message, Viewtype};
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;

/// A classic (unencrypted) mail with both To and Cc, several addresses each.
const CLASSIC: &[u8] = b"From: alice@example.org\r\n\
To: Bob <bob@example.net>, carol@example.com\r\n\
Cc: Dave <dave@example.com>, eve@example.org\r\n\
Subject: quarterly numbers\r\n\
Message-ID: <classic-1@example.org>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
\r\n\
body text\r\n";

fn addrs(recipients: &[Recipient], kind: RecipientKind) -> Vec<String> {
    recipients
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| r.addr.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incoming_to_and_cc_are_kept_apart() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let recipients = load(&t, msg_id).await?;
    assert_eq!(
        addrs(&recipients, RecipientKind::To),
        vec!["bob@example.net", "carol@example.com"],
        "To must be preserved in header order"
    );
    assert_eq!(
        addrs(&recipients, RecipientKind::Cc),
        vec!["dave@example.com", "eve@example.org"],
        "Cc must be preserved separately, in header order"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_display_names_are_recorded() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let to = load_kind(&t, msg_id, RecipientKind::To).await?;
    assert_eq!(to[0].name, "Bob");
    // A bare address has no display name; we must not invent one.
    assert_eq!(to[1].name, "");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_bcc_is_never_read_from_an_incoming_message() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    // A Bcc header on a received message is either impossible or added by an
    // intermediary. Trusting it would let a sender make a recipient believe
    // someone else was blind-copied.
    let raw = b"From: alice@example.org\r\n\
To: bob@example.net\r\n\
Bcc: mallory@example.com\r\n\
Subject: with a bcc header\r\n\
Message-ID: <bcc-1@example.org>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
\r\n\
body\r\n";
    let received = receive_imf(&t, raw, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    assert!(
        load_kind(&t, msg_id, RecipientKind::Bcc).await?.is_empty(),
        "an incoming Bcc header must be ignored"
    );
    assert_eq!(
        addrs(&load(&t, msg_id).await?, RecipientKind::To),
        vec!["bob@example.net"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_encrypted_message_uses_protected_recipients() -> Result<()> {
    let mut tcm = crate::test_utils::TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // Establish keys in both directions, then send encrypted.
    tcm.send_recv_accept(&alice, &bob, "hi").await;

    let chat = bob.create_chat(&alice).await;
    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("encrypted reply".to_string());
    let sent = bob.send_msg(chat.id, &mut msg).await;
    let received = alice.recv_msg(&sent).await;

    let to = load_kind(&alice, received.id, RecipientKind::To).await?;
    assert_eq!(
        to.iter().map(|r| r.addr.as_str()).collect::<Vec<_>>(),
        vec!["alice@example.org"],
        "recipients must come from the protected headers inside the encryption"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_records_what_it_was_addressed_to() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("outgoing".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    let to = load_kind(&alice, sent.sender_msg_id, RecipientKind::To).await?;
    assert_eq!(
        to.iter().map(|r| r.addr.as_str()).collect::<Vec<_>>(),
        vec!["bob@example.net"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_store_replaces_and_deduplicates() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    store(
        &t,
        msg_id,
        &[
            Recipient::new(RecipientKind::To, "x@example.org", "First"),
            // Same address again: the first name wins, because that is the one
            // the sender typed.
            Recipient::new(RecipientKind::To, "x@example.org", "Second"),
            Recipient::new(RecipientKind::Bcc, "y@example.org", ""),
        ],
    )
    .await?;

    let recipients = load(&t, msg_id).await?;
    assert_eq!(
        recipients,
        vec![
            Recipient::new(RecipientKind::To, "x@example.org", "First"),
            Recipient::new(RecipientKind::Bcc, "y@example.org", ""),
        ],
        "store must replace the previous set entirely and collapse duplicates"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_delete_and_prune() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    assert!(!load(&t, msg_id).await?.is_empty());

    delete(&t, msg_id).await?;
    assert!(load(&t, msg_id).await?.is_empty());

    // Recipients of a message that no longer exists must not survive
    // housekeeping.
    store(
        &t,
        msg_id,
        &[Recipient::new(RecipientKind::To, "z@example.org", "")],
    )
    .await?;
    crate::message::delete_msgs(&t, &[msg_id]).await?;
    prune(&t).await?;
    assert!(
        load(&t, msg_id).await?.is_empty(),
        "prune must drop recipients of deleted messages"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_ordering_is_to_then_cc_then_bcc() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    store(
        &t,
        msg_id,
        &[
            Recipient::new(RecipientKind::Bcc, "b@example.org", ""),
            Recipient::new(RecipientKind::Cc, "c@example.org", ""),
            Recipient::new(RecipientKind::To, "t@example.org", ""),
        ],
    )
    .await?;
    let kinds: Vec<RecipientKind> = load(&t, msg_id).await?.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![RecipientKind::To, RecipientKind::Cc, RecipientKind::Bcc],
        "load must return a stable To/Cc/Bcc order regardless of insertion order"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_subject_survives_as_a_first_class_field() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, CLASSIC, false).await?.unwrap();
    let msg = Message::load_from_db(&t, *received.msg_ids.last().unwrap()).await?;
    assert_eq!(
        msg.get_subject(),
        "quarterly numbers",
        "the subject must be stored verbatim, not folded into the body"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_subject_is_settable() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_subject("explicit subject".to_string());
    msg.set_text("body".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    let stored = Message::load_from_db(&alice, sent.sender_msg_id).await?;
    assert_eq!(stored.get_subject(), "explicit subject");
    assert!(
        sent.payload().contains("Subject: explicit subject"),
        "an explicitly set subject must reach the wire"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_config_default_still_loads() -> Result<()> {
    // Guards against the migration breaking config reads, which would be an
    // easy way for a new table to take the whole database down.
    let t = TestContext::new_alice().await;
    assert!(t.get_config(Config::Addr).await?.is_some());
    Ok(())
}
