//! Tests for contact gating.

use anyhow::Result;

use super::*;
use crate::chat::send_text_msg;
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};
use crate::tools::SystemTime;

use super::super::tags::{self, SystemTag};

fn mail_from(addr: &str, mid: &str) -> Vec<u8> {
    format!(
        "From: {addr}\r\n\
         To: alice@example.org\r\n\
         Subject: hello\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
         \r\n\
         body\r\n"
    )
    .into_bytes()
}

/// Sets the sweep window. Tests run on a bare `TestContext`, whose compile-time
/// default is `0` -- never sweep -- so a test about the deadline has to ask for
/// one.
async fn set_window(t: &TestContext, days: i64) -> Result<()> {
    set_hold_days(t, days).await
}

async fn recv(t: &TestContext, addr: &str, mid: &str) -> Result<MsgId> {
    t.allow_unencrypted().await?;
    let received = receive_imf(t, &mail_from(addr, mid), false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_gating_is_on_for_an_eeemail_account() -> Result<()> {
    // Unconfigured on purpose: `apply_defaults` never touches an account that
    // is already set up, so this is the only state in which it does anything.
    let t = TestContext::new().await;
    // Off in upstream's compile-time default, on for us. A gate the user has to
    // find and enable protects only the people who already knew to look for it,
    // so eeemail turns it on at setup. See ADR 0018 and ADR 0012.
    assert!(!is_enabled(&t).await?, "upstream's default is off");
    super::super::policy::apply_defaults(&t).await?;
    assert!(is_enabled(&t).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_applying_defaults_never_overwrites_an_explicit_choice() -> Result<()> {
    let t = TestContext::new().await;
    set_enabled(&t, false).await?;
    super::super::policy::apply_defaults(&t).await?;
    assert!(
        !is_enabled(&t).await?,
        "a user who turned it off must stay off"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mail_from_a_stranger_is_held() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s1@example.net").await?;

    assert_eq!(held(&t).await?, vec![msg_id]);
    assert_eq!(
        tags::of_msg(&t, msg_id).await?.system,
        vec![SystemTag::Unverified]
    );
    // The unverified view is not the inbox, and that is the entire point.
    assert!(tags::messages(&t, SystemTag::Inbox).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_held_mail_is_still_downloaded_and_readable() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s2@example.net").await?;

    // Unverified is a view, not a refusal to fetch: the user must be able to look
    // at what arrived before deciding about the sender.
    let msg = crate::message::Message::load_from_db(&t, msg_id).await?;
    // Contains rather than equals: this account never had
    // `email::policy::apply_defaults` run on it, so `SubjectInBody` is still
    // upstream's default and the subject is prepended to the body.
    assert!(msg.get_text().contains("body"), "got {:?}", msg.get_text());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mail_from_someone_you_wrote_to_is_not_held() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    set_enabled(&alice, true).await?;

    // Writing to someone makes them known, which is enough.
    let chat = alice.create_chat(&bob).await;
    send_text_msg(&alice, chat.id, "hi".to_string()).await?;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    assert!(is_trusted(&alice, bob_id).await?);

    tcm.send_recv(&bob, &alice, "reply").await;
    assert!(held(&alice).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_accepting_a_contact_releases_the_mail_already_held() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s3@example.net").await?;
    assert_eq!(held(&t).await?, vec![msg_id]);

    // Past mail, not just future mail: the user accepted the *sender*.
    let chat_id = crate::message::Message::load_from_db(&t, msg_id)
        .await?
        .get_chat_id();
    chat_id.accept(&t).await?;

    assert!(held(&t).await?.is_empty());
    assert_eq!(
        tags::of_msg(&t, msg_id).await?.system,
        vec![SystemTag::Inbox]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_release_ignores_contacts_that_are_still_strangers() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s4@example.net").await?;
    let from_id = crate::message::Message::load_from_db(&t, msg_id)
        .await?
        .get_from_id();

    // Origin is scaled up constantly; most scale-ups do not cross into trusted,
    // and a release that trusted its call site would empty the unverified view.
    assert_eq!(release(&t, &[from_id]).await?, 0);
    assert_eq!(held(&t).await?, vec![msg_id]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_turning_gating_off_releases_everything() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    recv(&t, "one@example.net", "g1@example.net").await?;
    recv(&t, "two@example.net", "g2@example.net").await?;
    assert_eq!(held(&t).await?.len(), 2);

    // Leaving mail in a view the user just switched off would strand it there
    // until it was swept.
    set_enabled(&t, false).await?;
    assert!(held(&t).await?.is_empty());

    recv(&t, "three@example.net", "g3@example.net").await?;
    assert!(held(&t).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_held_mail_is_swept_into_trash_once_the_hold_elapses() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    set_window(&t, DEFAULT_HOLD_DAYS).await?;
    super::super::ephemeral::set_purge_days(&t, 30).await?;
    let msg_id = recv(&t, "stranger@example.net", "s5@example.net").await?;
    assert_eq!(held(&t).await?, vec![msg_id]);

    // A day before the deadline, nothing happens.
    SystemTime::shift(std::time::Duration::from_secs(
        (DEFAULT_HOLD_DAYS as u64 - 1) * 86_400,
    ));
    assert_eq!(sweep(&t).await?, 0);
    assert_eq!(held(&t).await?, vec![msg_id]);

    SystemTime::shift(std::time::Duration::from_secs(2 * 86_400));
    assert_eq!(sweep(&t).await?, 1);
    assert!(held(&t).await?.is_empty());

    // Moved, not destroyed. This is the whole point of the change: the deadline
    // next door had a recoverable window and this one did not, and two
    // deadlines a few lines apart disagreeing about whether a deadline may
    // destroy the only copy of a mailbox was not defensible.
    assert_eq!(
        tags::of_msg(&t, msg_id).await?.system,
        vec![SystemTag::Trash]
    );
    // `contains` rather than `==`: this context has not had `apply_defaults`
    // run on it, so upstream's `SubjectInBody` is still on and the subject is
    // prepended to the text. What matters here is that the content survived.
    let msg = crate::message::Message::load_from_db(&t, msg_id).await?;
    assert!(
        msg.get_text().contains("body"),
        "the swept message lost its content: {:?}",
        msg.get_text()
    );

    // And it says why it is there. "You deleted this" would be a lie about mail
    // the user never touched.
    let trashed = super::super::ephemeral::trashed(&t, msg_id).await?.unwrap();
    assert_eq!(trashed.reason, super::super::ephemeral::Reason::Unaccepted);

    // And restorable by hand, which is the only way back: `release` reads
    // `held_msgs`, and the sweep dropped that row.
    super::super::ephemeral::restore(&t, &[msg_id]).await?;
    assert_eq!(
        tags::of_msg(&t, msg_id).await?.system,
        vec![SystemTag::Inbox]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_zero_window_never_sweeps() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    // Zero means "wait indefinitely", not "sweep now". Someone who wants
    // unverified mail gone at once turns gating off instead, which releases it
    // to the inbox where they can delete it.
    set_window(&t, 0).await?;
    let msg_id = recv(&t, "stranger@example.net", "s7@example.net").await?;

    SystemTime::shift(std::time::Duration::from_secs(3650 * 86_400));
    assert_eq!(sweep(&t).await?, 0);
    assert_eq!(held(&t).await?, vec![msg_id]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_shortening_the_window_moves_mail_already_waiting() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    set_window(&t, 30).await?;
    super::super::ephemeral::set_purge_days(&t, 30).await?;
    let msg_id = recv(&t, "stranger@example.net", "s8@example.net").await?;

    SystemTime::shift(std::time::Duration::from_secs(10 * 86_400));
    assert_eq!(sweep(&t).await?, 0, "not due under a 30-day window");

    // The deadline is `held_at` plus the *current* window, read afresh on every
    // sweep. Someone shortening this means the mail already waiting, not only
    // whatever arrives next; a deadline stored at hold time would have silently
    // outvoted them.
    set_window(&t, 7).await?;
    assert_eq!(sweep(&t).await?, 1);
    assert!(held(&t).await?.is_empty());
    assert_eq!(
        tags::of_msg(&t, msg_id).await?.system,
        vec![SystemTag::Trash]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sweep_drops_rows_whose_message_is_already_gone() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    set_window(&t, DEFAULT_HOLD_DAYS).await?;
    let msg_id = recv(&t, "stranger@example.net", "s6@example.net").await?;
    crate::message::delete_msgs(&t, &[msg_id]).await?;

    sweep(&t).await?;
    let rows: i64 = t
        .sql
        .query_get_value("SELECT COUNT(*) FROM held_msgs", ())
        .await?
        .unwrap_or_default();
    // Retention must not outlive what it describes.
    assert_eq!(rows, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_mail_is_never_held() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    set_enabled(&alice, true).await?;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    send_text_msg(&alice, chat.id, "hi".to_string()).await?;
    assert!(held(&alice).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_encrypted_reply_from_someone_you_wrote_to_is_not_held() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    crate::email::policy::apply_defaults(&alice).await?;
    alice.set_config_bool(Config::InboxGating, true).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    // Alice writes first, which makes her *address*-contact for Bob known.
    crate::email::compose::send(
        &alice,
        &crate::email::compose::RecipientSet {
            to: vec![bob_addr.clone()],
            ..Default::default()
        },
        "Numbers",
        "hello",
        None,
    )
    .await?;
    alice.pop_sent_msg().await;

    // Bob's encrypted reply arrives attributed to a *key*-contact -- a
    // different row, with no origin of its own. Holding it would mean the
    // inbox silently swallows the replies to the user's own mail.
    tcm.send_recv(&bob, &alice, "encrypted reply").await;

    let received = alice.get_last_msg().await;
    let tags = tags::of_msg(&alice, received.get_id()).await?;
    assert!(
        !tags.system.contains(&SystemTag::Unverified),
        "an encrypted reply from someone the user wrote to was held: {:?}",
        tags.system
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verifying_a_stranger_releases_the_mail_they_sent_cold() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    alice.allow_unencrypted().await?;
    crate::email::policy::apply_defaults(&alice).await?;
    alice.set_config_bool(Config::InboxGating, true).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    // Cold mail, unsigned, from someone Alice has never written to. It is
    // attributed to Bob's *address* row: there is no signature, so
    // `receive_imf` has no fingerprint to attach.
    let msg_id = recv(&alice, &bob_addr, "cold@example.net").await?;
    assert_eq!(
        tags::of_msg(&alice, msg_id).await?.system,
        vec![SystemTag::Unverified]
    );
    let address_row = crate::message::Message::load_from_db(&alice, msg_id)
        .await?
        .get_from_id();

    // Alice scans Bob's code. SecureJoin verifies his *key* row and releases
    // against that -- a different row from the one holding the message. Nothing
    // before this point writes to Bob, which would make him known and release
    // the mail on its own, testing nothing.
    tcm.execute_securejoin(&alice, &bob).await;

    let key_row = crate::contact::Contact::get_by_id(&alice, address_row)
        .await?
        .is_key_contact();
    assert!(!key_row, "the held message should be on the address row");

    assert!(
        held(&alice).await?.is_empty(),
        "cold mail stayed held after its sender was verified"
    );
    assert_eq!(
        tags::of_msg(&alice, msg_id).await?.system,
        vec![SystemTag::Inbox]
    );
    Ok(())
}
