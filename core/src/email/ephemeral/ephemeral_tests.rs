//! Tests for recoverable ephemeral expiry.

use anyhow::Result;

use super::*;
use crate::chat::send_text_msg;
use crate::message::Message;
use crate::test_utils::TestContextManager;
use crate::tools::SystemTime;

use super::super::tags::{self, SystemTag};

async fn set_purge_days(context: &Context, days: i64) -> Result<()> {
    context
        .set_config(Config::EphemeralTrashDays, Some(&days.to_string()))
        .await
}

/// Sends a message with a short timer and lets it fire.
async fn expire_one(
    alice: &crate::test_utils::TestContext,
    chat_id: crate::chat::ChatId,
) -> Result<MsgId> {
    // Set directly rather than through `apply_defaults`, which never touches an
    // already-configured account. The compile-time default is 0 -- destroy
    // immediately -- so that upstream's ephemeral tests keep passing unpatched.
    set_purge_days(alice, DEFAULT_PURGE_DAYS).await?;
    chat_id
        .set_ephemeral_timer(alice, Timer::from_u32(60))
        .await?;
    let msg_id = send_text_msg(alice, chat_id, "vanishing".to_string()).await?;
    set_message_timer(alice, msg_id, Timer::from_u32(60)).await?;
    // Asserted rather than assumed: if the window is zero, `divert` correctly
    // does nothing and core destroys the message, and every assertion after
    // this would fail somewhere far less informative.
    assert_eq!(purge_days(alice).await?, DEFAULT_PURGE_DAYS);
    assert!(message_expires_at(alice, msg_id).await?.is_some());

    SystemTime::shift(std::time::Duration::from_secs(120));
    crate::ephemeral::delete_expired_messages(alice, time()).await?;
    Ok(msg_id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_expiry_lands_in_trash_and_the_message_is_still_readable() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;

    let msg_id = expire_one(&alice, chat.id).await?;

    // The whole point of ADR 0019: expiry removes the message from view without
    // destroying it, so the user can change their mind.
    assert!(
        tags::of_msg(&alice, msg_id)
            .await?
            .system
            .contains(&SystemTag::Trash)
    );
    assert_eq!(
        Message::load_from_db(&alice, msg_id).await?.get_text(),
        "vanishing"
    );

    let trashed = trashed(&alice, msg_id).await?.unwrap();
    assert_eq!(trashed.reason, Reason::Expired);
    assert_eq!(
        trashed.purge_at - trashed.trashed_at,
        DEFAULT_PURGE_DAYS * 86_400
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_expiry_clears_the_timer_so_core_does_not_destroy_it() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;

    let msg_id = expire_one(&alice, chat.id).await?;
    // Clearing `ephemeral_timestamp` is what makes this a diversion rather than
    // a duplicate: core's own sweep runs immediately afterwards.
    assert_eq!(message_expires_at(&alice, msg_id).await?, None);

    // Running the sweep again must not find it, however many times it runs.
    crate::ephemeral::delete_expired_messages(&alice, time()).await?;
    assert_eq!(
        Message::load_from_db(&alice, msg_id).await?.get_text(),
        "vanishing"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_restored_message_does_not_expire_again() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;

    let msg_id = expire_one(&alice, chat.id).await?;
    restore(&alice, &[msg_id]).await?;

    assert!(trashed(&alice, msg_id).await?.is_none());
    assert!(
        !tags::of_msg(&alice, msg_id)
            .await?
            .system
            .contains(&SystemTag::Trash)
    );

    // Restoring a message the user asked to keep and then expiring it an hour
    // later would be a bug wearing a feature's clothes.
    SystemTime::shift(std::time::Duration::from_secs(86_400));
    crate::ephemeral::delete_expired_messages(&alice, time()).await?;
    assert!(trashed(&alice, msg_id).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_purge_destroys_the_message_once_the_window_elapses() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;

    let msg_id = expire_one(&alice, chat.id).await?;

    SystemTime::shift(std::time::Duration::from_secs(
        (DEFAULT_PURGE_DAYS as u64 - 1) * 86_400,
    ));
    assert_eq!(purge(&alice).await?, 0);
    assert_eq!(
        Message::load_from_db(&alice, msg_id).await?.get_text(),
        "vanishing"
    );

    SystemTime::shift(std::time::Duration::from_secs(2 * 86_400));
    assert_eq!(purge(&alice).await?, 1);
    let msg = Message::load_from_db(&alice, msg_id).await;
    assert!(msg.is_err() || msg?.get_text().is_empty());
    assert!(trashed(&alice, msg_id).await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_user_can_take_the_timer_off_a_message() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    chat.id
        .set_ephemeral_timer(&alice, Timer::from_u32(60))
        .await?;
    let msg_id = send_text_msg(&alice, chat.id, "keep me".to_string()).await?;
    set_message_timer(&alice, msg_id, Timer::from_u32(60)).await?;
    assert!(message_expires_at(&alice, msg_id).await?.is_some());

    // Issue #3: the timer is the user's, per message.
    set_message_timer(&alice, msg_id, Timer::Disabled).await?;
    assert_eq!(message_expires_at(&alice, msg_id).await?, None);

    SystemTime::shift(std::time::Duration::from_secs(3600));
    crate::ephemeral::delete_expired_messages(&alice, time()).await?;
    assert_eq!(
        Message::load_from_db(&alice, msg_id).await?.get_text(),
        "keep me"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_user_can_shorten_a_timer() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    let msg_id = send_text_msg(&alice, chat.id, "soon".to_string()).await?;

    set_message_timer(&alice, msg_id, Timer::from_u32(86_400)).await?;
    let far = message_expires_at(&alice, msg_id).await?.unwrap();
    set_message_timer(&alice, msg_id, Timer::from_u32(60)).await?;
    let near = message_expires_at(&alice, msg_id).await?.unwrap();
    assert!(near < far, "shortening a timer must move the deadline in");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_manual_trash_and_restore() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    set_purge_days(&alice, DEFAULT_PURGE_DAYS).await?;
    let msg_id = send_text_msg(&alice, chat.id, "junk".to_string()).await?;

    trash(&alice, &[msg_id]).await?;
    assert_eq!(
        trashed(&alice, msg_id).await?.unwrap().reason,
        Reason::Deleted
    );
    assert_eq!(in_trash(&alice).await?, vec![msg_id]);

    restore(&alice, &[msg_id]).await?;
    assert!(in_trash(&alice).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_re_trashing_does_not_extend_the_deadline() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    set_purge_days(&alice, DEFAULT_PURGE_DAYS).await?;
    let msg_id = send_text_msg(&alice, chat.id, "junk".to_string()).await?;

    trash(&alice, &[msg_id]).await?;
    let first = trashed(&alice, msg_id).await?.unwrap();

    SystemTime::shift(std::time::Duration::from_secs(86_400));
    trash(&alice, &[msg_id]).await?;
    let second = trashed(&alice, msg_id).await?.unwrap();

    // Otherwise a countdown the user is watching could be reset out from under
    // them, or extended indefinitely by re-trashing.
    assert_eq!(first.purge_at, second.purge_at);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_delete_device_after_is_still_destroyed_by_core() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    let msg_id = send_text_msg(&alice, chat.id, "old".to_string()).await?;

    // A user who asks for the disk back and gets a trash folder full of the
    // same bytes has been ignored.
    alice
        .set_config(crate::config::Config::DeleteDeviceAfter, Some("60"))
        .await?;
    SystemTime::shift(std::time::Duration::from_secs(120));
    crate::ephemeral::delete_expired_messages(&alice, time()).await?;

    assert!(trashed(&alice, msg_id).await?.is_none());
    let msg = Message::load_from_db(&alice, msg_id).await;
    assert!(msg.is_err() || msg?.get_text().is_empty());
    Ok(())
}
