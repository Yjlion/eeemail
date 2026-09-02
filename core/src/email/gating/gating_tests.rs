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
        vec![SystemTag::Holding]
    );
    // Holding is not the inbox, and that is the entire point.
    assert!(tags::messages(&t, SystemTag::Inbox).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_held_mail_is_still_downloaded_and_readable() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s2@example.net").await?;

    // Holding is a view, not a refusal to fetch: the user must be able to look
    // at what arrived before deciding about the sender.
    let msg = crate::message::Message::load_from_db(&t, msg_id).await?;
    // Contains rather than equals: core prepends the subject to the body of
    // classic mail, which is its own known problem and not this one's.
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
    // and a release that trusted its call site would empty the holding view.
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
    // until it purged.
    set_enabled(&t, false).await?;
    assert!(held(&t).await?.is_empty());

    recv(&t, "three@example.net", "g3@example.net").await?;
    assert!(held(&t).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_held_mail_is_purged_once_the_hold_elapses() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s5@example.net").await?;
    assert_eq!(held(&t).await?, vec![msg_id]);

    // A day before the deadline, nothing happens.
    SystemTime::shift(std::time::Duration::from_secs(
        (HOLD_DAYS as u64 - 1) * 86_400,
    ));
    assert_eq!(purge(&t).await?, 0);
    assert_eq!(held(&t).await?, vec![msg_id]);

    SystemTime::shift(std::time::Duration::from_secs(2 * 86_400));
    assert_eq!(purge(&t).await?, 1);
    assert!(held(&t).await?.is_empty());
    // Discarded, not archived: a tombstone remains to suppress re-download, and
    // the content is gone.
    let msg = crate::message::Message::load_from_db(&t, msg_id).await;
    assert!(msg.is_err() || msg?.get_text().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_purge_drops_rows_whose_message_is_already_gone() -> Result<()> {
    let t = TestContext::new_alice().await;
    set_enabled(&t, true).await?;
    let msg_id = recv(&t, "stranger@example.net", "s6@example.net").await?;
    crate::message::delete_msgs(&t, &[msg_id]).await?;

    purge(&t).await?;
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
