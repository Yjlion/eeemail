//! Tests for addressing a message to a recipient set.

use anyhow::Result;

use super::*;
use crate::config::Config;
use crate::email::policy::{EncryptionMode, undelivered};
use crate::email::recipients::{RecipientKind, load_kind};
use crate::message::{Message, Viewtype};
use crate::test_utils::{TestContext, TestContextManager};

#[test]
fn test_split_addr() {
    assert_eq!(
        split_addr("Carol Danvers <carol@example.com>"),
        ("Carol Danvers".to_string(), "carol@example.com".to_string())
    );
    assert_eq!(
        split_addr("carol@example.com"),
        (String::new(), "carol@example.com".to_string())
    );
    assert_eq!(
        split_addr("\"Danvers, Carol\" <carol@example.com>"),
        (
            "Danvers, Carol".to_string(),
            "carol@example.com".to_string()
        )
    );
    // A malformed address must come back whole rather than be silently
    // truncated into something that would be delivered to the wrong place.
    assert_eq!(
        split_addr("not <an address"),
        (String::new(), "not <an address".to_string())
    );
    // A display name is arbitrary text, including multi-byte characters.
    assert_eq!(
        split_addr("Zoë Ünicode <zoe@example.com>"),
        ("Zoë Ünicode".to_string(), "zoe@example.com".to_string())
    );
}

/// Creates a draft with its id assigned.
///
/// A draft is how a composer works, and `send_msg` keeps a draft's `msg_id`,
/// so a recipient set attached to it survives all the way to the wire.
async fn draft(t: &TestContext, chat_id: crate::chat::ChatId, text: &str) -> Result<Message> {
    let mut msg = Message::new(Viewtype::Text);
    msg.set_text(text.to_string());
    chat_id.set_draft(t, Some(&mut msg)).await?;
    Ok(msg)
}

/// The SMTP envelope of the queued message.
///
/// Read **before** `pop_sent_msg`, which consumes the `smtp` row.
async fn envelope(t: &TestContext) -> Result<String> {
    Ok(t.sql
        .query_get_value("SELECT recipients FROM smtp LIMIT 1", ())
        .await?
        .unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_verified_contact_gets_an_encrypted_chat() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // Gives alice a *key*-contact for bob, which is what an encrypted incoming
    // message or a QR verification produces. The same address also has an
    // address-contact, and picking the wrong one is the whole bug.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    let msg_id = send(
        &alice,
        &RecipientSet {
            to: vec![bob_addr],
            ..Default::default()
        },
        "Subject",
        "the body",
        None,
    )
    .await?;
    let sent = alice.pop_sent_msg().await;

    // `Chat::is_encrypted` keys off the contact row's fingerprint, so a chat
    // built from the address-contact renders cleartext however many keys we
    // hold for that person -- including one the user verified by QR.
    assert!(
        sent.payload().contains("BEGIN PGP MESSAGE"),
        "mail to a key-contact went out unencrypted:\n{}",
        sent.payload()
    );
    assert!(
        !sent.payload().contains("the body"),
        "the plaintext body is on the wire:\n{}",
        sent.payload()
    );
    let msg = Message::load_from_db(&alice, msg_id).await?;
    assert!(
        msg.get_showpadlock(),
        "the message is encrypted but does not say so"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cc_reaches_the_wire() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            to: vec!["bob@example.net".to_string()],
            cc: vec!["Carol Danvers <carol@example.com>".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    let sent = alice.pop_sent_msg().await;

    // The gap this phase exists to close: upstream emits no Cc header at all.
    assert!(
        sent.payload().contains("Cc:"),
        "no Cc header on the wire:\n{}",
        sent.payload()
    );
    assert!(sent.payload().contains("carol@example.com"));
    assert!(sent.payload().contains("Carol Danvers"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_that_copies_nobody_is_unchanged() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("body".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    assert!(
        !sent.payload().contains("Cc:"),
        "an absent Cc must stay absent, not become an empty header"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_bcc_never_appears_in_a_header() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            to: vec!["bob@example.net".to_string()],
            bcc: vec!["secret@example.com".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    let envelope = envelope(&alice).await?;
    let sent = alice.pop_sent_msg().await;

    // Getting this wrong is a disclosure, not a formatting bug.
    assert!(
        !sent.payload().contains("secret@example.com"),
        "a Bcc address must not appear anywhere in the message:\n{}",
        sent.payload()
    );
    // But it must still be delivered to.
    assert!(
        envelope.contains("secret@example.com"),
        "a Bcc recipient must still be in the envelope, or they get nothing"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cc_is_in_the_envelope() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            cc: vec!["carol@example.com".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    let envelope = envelope(&alice).await?;
    alice.pop_sent_msg().await;

    assert!(
        envelope.contains("carol@example.com"),
        "a Cc in the header but not the envelope is a message nobody sends: {envelope}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_set_recipients_creates_contacts() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;
    let msg = draft(&alice, chat.id, "body").await?;

    set_recipients(
        &alice,
        msg.id,
        &RecipientSet {
            cc: vec!["Carol <carol@example.com>".to_string()],
            ..Default::default()
        },
    )
    .await?;

    // Resolving to a contact is what makes "do we have a key for them?" the
    // same question it is for a chat member.
    let contact_id = crate::contact::Contact::lookup_id_by_addr(
        &alice,
        "carol@example.com",
        crate::contact::Origin::Unknown,
    )
    .await?;
    assert!(contact_id.is_some());

    let cc = load_kind(&alice, msg.id, RecipientKind::Cc).await?;
    assert_eq!(cc.len(), 1);
    assert_eq!(cc[0].addr, "carol@example.com");
    assert_eq!(cc[0].name, "Carol");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blank_and_duplicate_addresses_are_ignored() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;
    let msg = draft(&alice, chat.id, "body").await?;

    set_recipients(
        &alice,
        msg.id,
        &RecipientSet {
            cc: vec![
                "  ".to_string(),
                "carol@example.com".to_string(),
                "CAROL@example.com".to_string(),
            ],
            ..Default::default()
        },
    )
    .await?;

    let cc = load_kind(&alice, msg.id, RecipientKind::Cc).await?;
    assert_eq!(cc.len(), 1, "case differences are the same address");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_chat_member_is_not_added_twice() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    // Cc'ing someone already in the conversation is a normal thing to type.
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            cc: vec!["bob@example.net".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    let envelope = envelope(&alice).await?;
    let sent = alice.pop_sent_msg().await;

    assert!(
        !sent.payload().contains("Cc:"),
        "someone the chat already covers must not be duplicated into Cc:\n{}",
        sent.payload()
    );
    assert_eq!(
        envelope.matches("bob@example.net").count(),
        1,
        "and must appear once in the envelope, not twice: {envelope}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_keyless_cc_is_recorded_as_undelivered_when_encrypting() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // Establish a key for Bob so the message really does go out encrypted.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;

    let chat = alice.create_chat(&bob).await;
    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            cc: vec!["stranger@example.com".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    alice.pop_sent_msg().await;

    // Core drops a keyless recipient from the envelope but leaves them in the
    // headers. Routing Cc through the same path means the same reporting
    // applies, so the user can be told rather than left to find out.
    assert_eq!(
        undelivered(&alice, msg_id).await?,
        vec!["stranger@example.com"],
        "a Cc we could not encrypt to must be reported, not silently dropped"
    );
    alice.assert_warn("Missing key").await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_strict_refuses_a_keyless_cc() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    EncryptionMode::set(&alice, EncryptionMode::Strict).await?;

    let chat = alice.create_chat(&bob).await;
    let mut msg = draft(&alice, chat.id, "secret").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            cc: vec!["stranger@example.com".to_string()],
            ..Default::default()
        },
    )
    .await?;

    // Strict means end-to-end only, and that has to cover people the user
    // copied, not just people the chat knows about. The message still goes to
    // those who *do* have keys; the person with none is dropped from the
    // envelope rather than sent cleartext.
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    let envelope = envelope(&alice).await?;
    let sent = alice.pop_sent_msg().await;

    assert!(
        !envelope.contains("stranger@example.com"),
        "strict must not deliver to someone we cannot encrypt to: {envelope}"
    );
    assert!(
        !sent.payload().contains("secret"),
        "and the body must be encrypted, not readable on the wire"
    );
    assert_eq!(
        undelivered(&alice, msg_id).await?,
        vec!["stranger@example.com"],
        "the user has to be told who was left out"
    );
    alice.assert_warn("Missing key").await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_recipient_set_survives_sending() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = draft(&alice, chat.id, "body").await?;
    let msg_id = msg.id;
    set_recipients(
        &alice,
        msg_id,
        &RecipientSet {
            cc: vec!["carol@example.com".to_string()],
            bcc: vec!["secret@example.com".to_string()],
            ..Default::default()
        },
    )
    .await?;
    crate::chat::send_msg(&alice, chat.id, &mut msg).await?;
    alice.pop_sent_msg().await;

    // The send path records what actually went out. Bcc is in no header, so
    // rebuilding the set from the rendered message alone would erase the fact
    // that anyone was blind-copied -- and "who did I send this to?" is a
    // question a sent message has to be able to answer.
    let cc = load_kind(&alice, msg_id, RecipientKind::Cc).await?;
    assert_eq!(cc.len(), 1);
    assert_eq!(cc[0].addr, "carol@example.com");

    let bcc = load_kind(&alice, msg_id, RecipientKind::Bcc).await?;
    assert_eq!(bcc.len(), 1, "the Bcc record must survive sending");
    assert_eq!(bcc[0].addr, "secret@example.com");

    let to = load_kind(&alice, msg_id, RecipientKind::To).await?;
    assert_eq!(to.len(), 1);
    assert_eq!(to[0].addr, "bob@example.net");
    Ok(())
}
