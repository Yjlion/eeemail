//! Tests for learning a key from an Autocrypt header.

use anyhow::Result;

use super::*;
use crate::aheader::{Aheader, EncryptPreference};
use crate::config::Config;
use crate::key::load_self_public_key;
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};

/// A cleartext message from `sender`, advertising `sender`'s key.
///
/// Built by hand rather than sent through core: a message core encrypts is
/// signed, and a signed message is exactly the case this module leaves alone.
async fn autocrypt_mail(sender: &TestContext, mid: &str) -> Result<Vec<u8>> {
    let addr = sender.get_config(Config::Addr).await?.unwrap();
    let header = Aheader {
        addr: addr.clone(),
        public_key: load_self_public_key(sender).await?,
        prefer_encrypt: EncryptPreference::Mutual,
        verified: false,
    };
    Ok(format!(
        "From: <{addr}>\n\
         To: <alice@example.org>\n\
         Message-ID: <{mid}>\n\
         Subject: hello\n\
         Date: Mon, 1 Sep 2026 12:00:00 +0000\n\
         Autocrypt: {header}\n\
         Content-Type: text/plain\n\
         \n\
         cleartext body\n"
    )
    .into_bytes())
}

/// The same message with no `Autocrypt:` header at all.
fn plain_mail(addr: &str, mid: &str) -> Vec<u8> {
    format!(
        "From: <{addr}>\n\
         To: <alice@example.org>\n\
         Message-ID: <{mid}>\n\
         Subject: hello\n\
         Date: Mon, 1 Sep 2026 12:00:00 +0000\n\
         Content-Type: text/plain\n\
         \n\
         cleartext body\n"
    )
    .into_bytes()
}

/// The key-contacts we hold for an address.
async fn key_contacts(t: &TestContext, addr: &str) -> Result<Vec<ContactId>> {
    t.sql
        .query_map_vec(
            "SELECT id FROM contacts WHERE addr=? COLLATE NOCASE AND fingerprint!=''",
            (addr,),
            |row| Ok(row.get::<_, ContactId>(0)?),
        )
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_autocrypt_header_creates_a_key_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    assert!(
        key_contacts(&alice, &bob_addr).await?.is_empty(),
        "no key-contact should exist before the message arrives"
    );
    receive_imf(
        &alice,
        &autocrypt_mail(&bob, "ac1@example.net").await?,
        false,
    )
    .await?;

    assert_eq!(
        key_contacts(&alice, &bob_addr).await?.len(),
        1,
        "an advertised key must produce a key-contact, or opportunistic \
         encryption can never start"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_learned_contact_is_not_verified() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    receive_imf(
        &alice,
        &autocrypt_mail(&bob, "ac2@example.net").await?,
        false,
    )
    .await?;

    // The distinction this ADR rests on. An unauthenticated header may buy
    // encryption; it must never buy the claim that survives an active attacker.
    for contact_id in key_contacts(&alice, &bob_addr).await? {
        let contact = Contact::get_by_id(&alice, contact_id).await?;
        assert!(
            !contact.is_verified(&alice).await?,
            "a key learned from a header must not count as verified"
        );
        assert!(
            !contact.origin.is_known(),
            "learning a key is not the same as choosing to know someone, and \
             gating reads exactly this"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_without_a_key_creates_nothing() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    receive_imf(
        &alice,
        &plain_mail("stranger@example.net", "p1@example.net"),
        false,
    )
    .await?;

    assert!(
        key_contacts(&alice, "stranger@example.net")
            .await?
            .is_empty(),
        "a message advertising no key must not produce a key-contact"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_signed_message_is_left_to_core() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;

    // Core's own path: encrypted and signed, so the fingerprint is checked
    // against the signature rather than taken on trust.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    assert_eq!(
        key_contacts(&alice, &bob_addr).await?.len(),
        1,
        "core makes the key-contact for a signed message; we must not make a \
         second one beside it"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_same_sender_twice_is_one_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    receive_imf(
        &alice,
        &autocrypt_mail(&bob, "ac3@example.net").await?,
        false,
    )
    .await?;
    receive_imf(
        &alice,
        &autocrypt_mail(&bob, "ac4@example.net").await?,
        false,
    )
    .await?;

    assert_eq!(
        key_contacts(&alice, &bob_addr).await?.len(),
        1,
        "a second message from the same sender must not fork the contact"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mail_after_an_autocrypt_header_is_encrypted() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    receive_imf(
        &alice,
        &autocrypt_mail(&bob, "ac5@example.net").await?,
        false,
    )
    .await?;

    // The property the whole ADR exists for: the reply goes out encrypted,
    // with nobody having scanned anything.
    let msg_id = crate::email::compose::send(
        &alice,
        &crate::email::compose::RecipientSet {
            to: vec![bob_addr],
            ..Default::default()
        },
        "Subject",
        "the body",
        None,
    )
    .await?;
    let sent = alice.pop_sent_msg().await;

    assert!(
        sent.payload().contains("BEGIN PGP MESSAGE"),
        "the reply went out in cleartext:\n{}",
        sent.payload()
    );
    assert!(
        !sent.payload().contains("the body"),
        "the plaintext body is on the wire:\n{}",
        sent.payload()
    );
    let msg = crate::message::Message::load_from_db(&alice, msg_id).await?;
    assert!(msg.get_showpadlock(), "encrypted but not reported as such");
    Ok(())
}
