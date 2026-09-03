//! Tests for system tags.

use anyhow::Result;

use super::*;
use crate::chat::{ChatId, send_text_msg};
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};

/// Mail *to* the test context, from somebody else.
///
/// `From: alice@example.org` would be the test context itself, which makes the
/// message self-sent and outgoing -- and therefore never in the inbox.
fn mail(mid: &str) -> Vec<u8> {
    format!(
        "From: bob@example.net\r\n\
         To: alice@example.org\r\n\
         Subject: hello\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
         \r\n\
         body\r\n"
    )
    .into_bytes()
}

async fn recv(t: &TestContext, mid: &str) -> Result<MsgId> {
    t.allow_unencrypted().await?;
    let received = receive_imf(t, &mail(mid), false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

#[test]
fn test_the_derived_stored_split_is_one_function() {
    // The whole model in one assertion: three tags have a row, three do not.
    let stored: Vec<SystemTag> = SystemTag::ALL
        .into_iter()
        .filter(|t| t.stored_name().is_some())
        .collect();
    assert_eq!(
        stored,
        vec![SystemTag::Unverified, SystemTag::Archive, SystemTag::Trash]
    );
}

#[test]
fn test_names_round_trip() {
    for tag in SystemTag::ALL {
        assert_eq!(SystemTag::parse(tag.as_str()), Some(tag));
    }
    assert_eq!(SystemTag::parse("  INBOX "), Some(SystemTag::Inbox));
    assert_eq!(SystemTag::parse("nonsense"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reserved_labels_exist_after_migration() -> Result<()> {
    let t = TestContext::new_alice().await;
    for name in labels::RESERVED {
        let label = labels::reserved(&t, name).await?;
        assert!(label.is_system, "{name} must be a system label");
        assert_eq!(label.name, name);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_incoming_mail_from_a_known_sender_is_in_the_inbox() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Gating off, so this test is about the inbox and not about the unverified view.
    super::super::gating::set_enabled(&t, false).await?;
    let msg_id = recv(&t, "inbox@example.org").await?;

    let tags = of_msg(&t, msg_id).await?;
    assert_eq!(tags.system, vec![SystemTag::Inbox]);
    assert!(tags.user.is_empty());
    assert_eq!(messages(&t, SystemTag::Inbox).await?, vec![msg_id]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_archiving_takes_a_message_out_of_the_inbox() -> Result<()> {
    let t = TestContext::new_alice().await;
    super::super::gating::set_enabled(&t, false).await?;
    let msg_id = recv(&t, "arch@example.org").await?;

    labels::archive(&t, &[msg_id]).await?;
    let tags = of_msg(&t, msg_id).await?;
    assert_eq!(tags.system, vec![SystemTag::Archive]);
    assert!(messages(&t, SystemTag::Inbox).await?.is_empty());
    assert_eq!(messages(&t, SystemTag::Archive).await?, vec![msg_id]);

    labels::unarchive(&t, &[msg_id]).await?;
    assert_eq!(of_msg(&t, msg_id).await?.system, vec![SystemTag::Inbox]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_user_tag_does_not_take_a_message_out_of_the_inbox() -> Result<()> {
    let t = TestContext::new_alice().await;
    super::super::gating::set_enabled(&t, false).await?;
    let msg_id = recv(&t, "both@example.org").await?;

    let work = labels::create(&t, "Work", None).await?;
    labels::apply(&t, &[msg_id], work.id).await?;

    // The point of tags over folders: a message is in the inbox *and* tagged.
    let tags = of_msg(&t, msg_id).await?;
    assert_eq!(tags.system, vec![SystemTag::Inbox]);
    assert_eq!(tags.user.len(), 1);
    assert_eq!(tags.user[0].name, "Work");
    assert_eq!(messages(&t, SystemTag::Inbox).await?, vec![msg_id]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_drafts_and_sent_are_derived_without_rows() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;

    let mut draft = crate::message::Message::new_text("not sent yet".to_string());
    chat.id.set_draft(&alice, Some(&mut draft)).await?;
    let draft_id = ChatId::get_draft(chat.id, &alice).await?.unwrap().id;
    assert_eq!(
        of_msg(&alice, draft_id).await?.system,
        vec![SystemTag::Drafts]
    );
    assert_eq!(messages(&alice, SystemTag::Drafts).await?, vec![draft_id]);

    let sent = send_text_msg(&alice, chat.id, "sent".to_string()).await?;
    assert_eq!(of_msg(&alice, sent).await?.system, vec![SystemTag::Sent]);
    assert!(messages(&alice, SystemTag::Sent).await?.contains(&sent));

    // Neither has a row: the whole reason they are derived.
    let rows: i64 = alice
        .sql
        .query_get_value("SELECT COUNT(*) FROM msg_labels", ())
        .await?
        .unwrap_or_default();
    assert_eq!(rows, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_mail_is_never_in_the_inbox() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let chat = alice.create_chat(&bob).await;
    let sent = send_text_msg(&alice, chat.id, "hi".to_string()).await?;

    assert!(
        !of_msg(&alice, sent)
            .await?
            .system
            .contains(&SystemTag::Inbox)
    );
    assert!(!messages(&alice, SystemTag::Inbox).await?.contains(&sent));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_that_no_longer_exists_has_no_tags() -> Result<()> {
    let t = TestContext::new_alice().await;
    let tags = of_msg(&t, MsgId::new(9_999)).await?;
    assert!(tags.system.is_empty());
    assert!(tags.user.is_empty());
    Ok(())
}
