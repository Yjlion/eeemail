//! Tests for encryption policy and server retention.

use anyhow::Result;

use super::*;
use crate::chat::ChatId;
use crate::message::{Message, Viewtype};
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};

const RAW: &[u8] = b"From: alice@example.org\r\n\
To: bob@example.net\r\n\
Subject: hello\r\n\
Message-ID: <a@x>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
\r\n\
body\r\n";

#[test]
fn test_modes_order_strictest_first() {
    // `effective` composes overrides with `min`, so the ordering is not
    // cosmetic.
    assert!(EncryptionMode::Strict < EncryptionMode::Opportunistic);
    assert!(EncryptionMode::Opportunistic < EncryptionMode::Lenient);
    assert_eq!(
        EncryptionMode::Strict.min(EncryptionMode::Lenient),
        EncryptionMode::Strict
    );
}

#[test]
fn test_unknown_mode_falls_back_to_the_default() {
    // Never to the most permissive one.
    assert_eq!(EncryptionMode::from_i64(99), EncryptionMode::Opportunistic);
    assert_eq!(EncryptionMode::from_i64(-1), EncryptionMode::Opportunistic);
}

#[test]
fn test_server_retention_from_days() {
    assert_eq!(
        ServerRetention::from_days(0),
        ServerRetention::DeleteAfterDownload
    );
    assert_eq!(ServerRetention::from_days(7), ServerRetention::Days(7));
    assert_eq!(ServerRetention::from_days(-1), ServerRetention::Never);
    // Must not panic via unsigned negation.
    assert_eq!(ServerRetention::from_days(i32::MIN), ServerRetention::Never);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_default_is_opportunistic() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Upstream defaults ForceEncryption on, which is strict. eeemail's default
    // has to be reached explicitly, so this pins that we did.
    EncryptionMode::set(&t, EncryptionMode::Opportunistic).await?;
    assert_eq!(
        EncryptionMode::load(&t).await?,
        EncryptionMode::Opportunistic
    );
    assert_eq!(
        t.get_config_int(crate::config::Config::EncryptionMode)
            .await?,
        1
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_setting_a_mode_keeps_force_encryption_in_step() -> Result<()> {
    let t = TestContext::new_alice().await;

    EncryptionMode::set(&t, EncryptionMode::Strict).await?;
    assert!(t.get_config_bool(Config::ForceEncryption).await?);
    assert_eq!(EncryptionMode::load(&t).await?, EncryptionMode::Strict);

    EncryptionMode::set(&t, EncryptionMode::Lenient).await?;
    assert!(!t.get_config_bool(Config::ForceEncryption).await?);
    assert_eq!(EncryptionMode::load(&t).await?, EncryptionMode::Lenient);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_force_encryption_wins_on_strictness() -> Result<()> {
    let t = TestContext::new_alice().await;
    EncryptionMode::set(&t, EncryptionMode::Lenient).await?;

    // Another client, or upstream code, turns strictness on behind our back.
    // Core's setting is the one actually enforced, so it must be the one we
    // report.
    t.set_config_bool(Config::ForceEncryption, true).await?;
    assert_eq!(EncryptionMode::load(&t).await?, EncryptionMode::Strict);

    // And with it off but our key claiming strict, we must not claim a
    // strictness nothing is enforcing.
    t.set_config_bool(Config::ForceEncryption, false).await?;
    t.set_config(Config::EncryptionMode, Some("0")).await?;
    assert_eq!(
        EncryptionMode::load(&t).await?,
        EncryptionMode::Opportunistic
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_per_contact_override() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    assert_eq!(EncryptionMode::for_contact(&alice, bob_id).await?, None);

    EncryptionMode::set_for_contact(&alice, bob_id, Some(EncryptionMode::Strict)).await?;
    assert_eq!(
        EncryptionMode::for_contact(&alice, bob_id).await?,
        Some(EncryptionMode::Strict)
    );

    EncryptionMode::set_for_contact(&alice, bob_id, None).await?;
    assert_eq!(EncryptionMode::for_contact(&alice, bob_id).await?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_overrides_compose_toward_the_strictest() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let fiona = tcm.fiona().await;
    EncryptionMode::set(&alice, EncryptionMode::Lenient).await?;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    let fiona_id = alice.add_or_lookup_contact_id(&fiona).await;

    assert_eq!(
        EncryptionMode::effective(&alice, &[bob_id, fiona_id]).await?,
        EncryptionMode::Lenient
    );

    // One correspondent marked strict must not be sent cleartext because
    // someone else on the message is lenient.
    EncryptionMode::set_for_contact(&alice, bob_id, Some(EncryptionMode::Strict)).await?;
    assert_eq!(
        EncryptionMode::effective(&alice, &[bob_id, fiona_id]).await?,
        EncryptionMode::Strict
    );

    // And an override can only tighten, never loosen, relative to the others.
    EncryptionMode::set_for_contact(&alice, fiona_id, Some(EncryptionMode::Lenient)).await?;
    assert_eq!(
        EncryptionMode::effective(&alice, &[bob_id, fiona_id]).await?,
        EncryptionMode::Strict
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_strict_refuses_to_send_without_a_key() -> Result<()> {
    let alice = TestContext::new_alice().await;
    EncryptionMode::set(&alice, EncryptionMode::Strict).await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("secret".to_string());
    assert!(
        crate::chat::send_msg(&alice, chat.id, &mut msg)
            .await
            .is_err(),
        "strict must fail rather than fall back to cleartext"
    );
    // Global strictness is left to core, so the user gets core's actionable
    // message rather than MimeFactory's internal one. Pinning this here because
    // an earlier version of `prepare_send` set `GuaranteeE2ee` unconditionally
    // and silently replaced it with "No recipient keys are available".
    alice
        .assert_warn("requires end-to-end encryption which is not setup yet")
        .await;
    assert_eq!(
        alice.get_last_msg().await.get_info_type(),
        crate::mimeparser::SystemMessage::InvalidUnencryptedMail,
        "core's info message must still be added to the chat"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_opportunistic_sends_cleartext_without_a_key() -> Result<()> {
    let alice = TestContext::new_alice().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("not secret".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;
    assert!(sent.payload().contains("not secret"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_per_contact_strict_override_applies_to_one_message() -> Result<()> {
    let alice = TestContext::new_alice().await;
    // Globally lenient, but this one correspondent is strict.
    EncryptionMode::set(&alice, EncryptionMode::Lenient).await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;
    let bob_id = *crate::chat::get_chat_contacts(&alice, chat.id)
        .await?
        .first()
        .unwrap();
    EncryptionMode::set_for_contact(&alice, bob_id, Some(EncryptionMode::Strict)).await?;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("secret".to_string());
    let err = crate::chat::send_msg(&alice, chat.id, &mut msg)
        .await
        .expect_err("a per-contact strict override must bind even when the global mode is lenient");
    assert!(
        format!("{err:#}").contains("end-to-end only"),
        "the override must fail with its own error, before MimeFactory runs: {err:#}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_missing_keys() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;

    // A contact we only know an address for. `add_or_lookup_contact_id` would
    // not do: it reads Bob's key out of the test fixture, so the contact would
    // have one from the start.
    let stranger =
        crate::contact::Contact::create(&alice, "Stranger", "stranger@example.com").await?;
    assert_eq!(
        missing_keys(&alice, &[stranger]).await?,
        vec!["stranger@example.com"]
    );

    // After a real exchange we hold Bob's key.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    assert!(missing_keys(&alice, &[bob_id]).await?.is_empty());

    // Our own address is never "missing": we always have our key.
    assert!(
        missing_keys(&alice, &[crate::contact::ContactId::SELF])
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_record_undelivered() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let dropped = record_undelivered(
        &t,
        msg_id,
        &[
            "bob@example.net".to_string(),
            "Dave@Example.com".to_string(),
        ],
        &["BOB@example.net".to_string()],
    )
    .await?;

    assert_eq!(
        dropped,
        vec!["Dave@Example.com"],
        "address comparison must be case-insensitive, or everyone looks dropped"
    );
    assert_eq!(undelivered(&t, msg_id).await?, vec!["Dave@Example.com"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_nothing_recorded_when_everyone_was_reached() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();

    let dropped = record_undelivered(
        &t,
        msg_id,
        &["bob@example.net".to_string()],
        &[
            "bob@example.net".to_string(),
            "self@example.org".to_string(),
        ],
    )
    .await?;
    assert!(dropped.is_empty());
    assert!(undelivered(&t, msg_id).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_ordinary_send_records_no_undelivered_recipients() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = Message::new(Viewtype::Text);
    msg.set_text("hello".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    assert!(
        undelivered(&alice, sent.sender_msg_id).await?.is_empty(),
        "a cleartext send reaches everyone it is addressed to"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_message_crypto() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;

    // A cleartext message claims nothing.
    let received = receive_imf(&alice, RAW, false).await?.unwrap();
    let crypto = message_crypto(&alice, *received.msg_ids.last().unwrap()).await?;
    assert_eq!(
        crypto,
        MessageCrypto {
            encrypted: false,
            signed: false,
            verified: false
        }
    );

    // An encrypted one from an unverified contact is encrypted and signed, but
    // not verified: only SecureJoin survives an active attacker.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let sent = bob
        .send_text(bob.create_chat(&alice).await.id, "encrypted")
        .await;
    let msg = alice.recv_msg(&sent).await;
    let crypto = message_crypto(&alice, msg.id).await?;
    assert!(crypto.encrypted);
    assert!(crypto.signed);
    assert!(
        !crypto.verified,
        "opportunistic key exchange must not be shown as verified"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Server retention
// ---------------------------------------------------------------------------

/// Pretends the message is on the server, which is what the `imap` table means.
async fn fake_on_server(t: &TestContext, rfc724_mid: &str) -> Result<()> {
    t.sql
        .execute(
            "INSERT INTO imap (transport_id, rfc724_mid, folder, uid, uidvalidity, target)
             VALUES (1, ?1, 'INBOX', ?2, 1, 'INBOX')",
            (rfc724_mid, i64::from(rfc724_mid.len() as u32)),
        )
        .await?;
    Ok(())
}

async fn target_of(t: &TestContext, rfc724_mid: &str) -> Result<Option<String>> {
    t.sql
        .query_get_value("SELECT target FROM imap WHERE rfc724_mid=?", (rfc724_mid,))
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_delete_after_download_is_the_default() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert_eq!(
        ServerRetention::load(&t).await?,
        ServerRetention::DeleteAfterDownload
    );

    fake_on_server(&t, "a@x").await?;
    apply_server_retention(&t, "a@x").await?;
    assert_eq!(
        target_of(&t, "a@x").await?.as_deref(),
        Some(""),
        "an empty target is how core's IMAP loop is told to delete"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_never_leaves_the_server_untouched() -> Result<()> {
    let t = TestContext::new_alice().await;
    ServerRetention::set(&t, ServerRetention::Never).await?;

    fake_on_server(&t, "a@x").await?;
    apply_server_retention(&t, "a@x").await?;
    assert_eq!(
        target_of(&t, "a@x").await?.as_deref(),
        Some("INBOX"),
        "coexistence mode must not touch the server mailbox at all"
    );
    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM server_retention", ())
            .await?
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_keep_days_defers_deletion() -> Result<()> {
    let t = TestContext::new_alice().await;
    ServerRetention::set(&t, ServerRetention::Days(7)).await?;

    fake_on_server(&t, "a@x").await?;
    apply_server_retention(&t, "a@x").await?;
    assert_eq!(
        target_of(&t, "a@x").await?.as_deref(),
        Some("INBOX"),
        "not deleted yet"
    );

    // Six days on: still inside the window.
    let delete_at: i64 = t
        .sql
        .query_get_value(
            "SELECT delete_at FROM server_retention WHERE rfc724_mid=?",
            ("a@x",),
        )
        .await?
        .unwrap();
    assert_eq!(delete_at, crate::tools::time() + 7 * 86_400);
    assert_eq!(expire_on_server(&t).await?, 0);
    assert_eq!(target_of(&t, "a@x").await?.as_deref(), Some("INBOX"));

    // Past the window.
    t.sql
        .execute(
            "UPDATE server_retention SET delete_at=? WHERE rfc724_mid=?",
            (crate::tools::time() - 1, "a@x"),
        )
        .await?;
    assert_eq!(expire_on_server(&t).await?, 1);
    assert_eq!(target_of(&t, "a@x").await?.as_deref(), Some(""));
    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM server_retention", ())
            .await?,
        "the row must be consumed, not deleted again on every pass"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_retention_is_not_retroactive() -> Result<()> {
    let t = TestContext::new_alice().await;
    // A mailbox that already has mail in it, from before eeemail ever ran.
    fake_on_server(&t, "old@x").await?;

    // The user now turns on delete-after-download.
    ServerRetention::set(&t, ServerRetention::DeleteAfterDownload).await?;
    apply_server_retention(&t, "new@x").await?;

    assert_eq!(
        target_of(&t, "old@x").await?.as_deref(),
        Some("INBOX"),
        "pointing eeemail at an existing mailbox must never destroy what was already there"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_received_message_is_deleted_from_the_server() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    fake_on_server(&t, "a@x").await?;

    receive_imf(&t, RAW, false).await?.unwrap();

    assert_eq!(
        target_of(&t, "a@x").await?.as_deref(),
        Some(""),
        "the local store is the mailbox, so the spool copy goes once it is stored"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prune_drops_undelivered_of_deleted_messages() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let received = receive_imf(&t, RAW, false).await?.unwrap();
    let msg_id = *received.msg_ids.last().unwrap();
    record_undelivered(&t, msg_id, &["dave@example.com".to_string()], &[]).await?;

    crate::message::delete_msgs(&t, &[msg_id]).await?;
    prune(&t).await?;
    assert!(undelivered(&t, msg_id).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_policy_appears_in_get_info() -> Result<()> {
    // Both settings decide what leaves the device; a support conversation
    // starts by asking what they are set to.
    let t = TestContext::new_alice().await;
    let info = t.get_info().await?;
    assert!(info.contains_key("encryption_mode"));
    assert!(info.contains_key("server_retention_days"));
    let _: ChatId = ChatId::new(0);
    Ok(())
}
