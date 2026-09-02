//! Tests for labels, tags and archive.

use anyhow::Result;

use super::*;
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;

fn mail(mid: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: alice@example.org\r\n\
         To: bob@example.net\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
         \r\n\
         body\r\n"
    )
    .into_bytes()
}

async fn recv(t: &TestContext, mid: &str) -> Result<MsgId> {
    let received = receive_imf(t, &mail(mid, "hello"), false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_archive_label_exists_after_migration() -> Result<()> {
    let t = TestContext::new_alice().await;
    let archive = archive_label(&t).await?;
    assert!(archive.is_system);
    assert_eq!(archive.name, ARCHIVE);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_is_idempotent_and_case_insensitive() -> Result<()> {
    let t = TestContext::new_alice().await;
    let first = create(&t, "Work", None).await?;
    // Two devices creating the same label independently must converge on one,
    // not collide -- this is also how a synced creation arrives.
    let second = create(&t, "work", Some(0x00ff00)).await?;
    assert_eq!(first.id, second.id);
    assert_eq!(second.name, "Work", "the first spelling is kept");

    // System labels first, then user ones alphabetically.
    let names: Vec<String> = list(&t).await?.into_iter().map(|l| l.name).collect();
    let mut expected: Vec<String> = RESERVED.iter().map(|n| n.to_string()).collect();
    expected.sort_by_key(|n| n.to_lowercase());
    expected.push("Work".to_string());
    assert_eq!(names, expected);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_empty_name_is_rejected() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert!(create(&t, "   ", None).await.is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_system_labels_cannot_be_renamed_or_deleted() -> Result<()> {
    let t = TestContext::new_alice().await;
    let archive = archive_label(&t).await?;
    assert!(rename(&t, archive.id, "Attic").await.is_err());
    assert!(delete(&t, archive.id).await.is_err());
    // Still there, still working.
    assert_eq!(archive_label(&t).await?.id, archive.id);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rename_rejects_a_name_already_taken() -> Result<()> {
    let t = TestContext::new_alice().await;
    let work = create(&t, "Work", None).await?;
    create(&t, "Personal", None).await?;
    assert!(rename(&t, work.id, "personal").await.is_err());
    // Renaming to its own name in different case is not a collision.
    rename(&t, work.id, "WORK").await?;
    assert_eq!(by_name(&t, "work").await?.unwrap().name, "WORK");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_apply_and_unapply() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;
    let b = recv(&t, "b@x").await?;
    let work = create(&t, "Work", None).await?;

    apply(&t, &[a, b], work.id).await?;
    assert_eq!(
        of_msg(&t, a)
            .await?
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>(),
        vec![work.id]
    );
    let mut labelled = msgs_with(&t, work.id).await?;
    labelled.sort();
    assert_eq!(labelled, {
        let mut v = vec![a, b];
        v.sort();
        v
    });

    // Applying twice must not duplicate.
    apply(&t, &[a], work.id).await?;
    assert_eq!(of_msg(&t, a).await?.len(), 1);

    unapply(&t, &[a], work.id).await?;
    assert!(of_msg(&t, a).await?.is_empty());
    assert_eq!(msgs_with(&t, work.id).await?, vec![b]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_can_carry_several_labels() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;
    let work = create(&t, "Work", None).await?;
    let urgent = create(&t, "Urgent", None).await?;

    apply(&t, &[a], work.id).await?;
    apply(&t, &[a], urgent.id).await?;
    archive(&t, &[a]).await?;

    let names: Vec<String> = of_msg(&t, a).await?.into_iter().map(|l| l.name).collect();
    // System labels first, then alphabetical.
    assert_eq!(names, vec!["Archive", "Urgent", "Work"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_archive_is_the_presence_of_a_label() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;

    // The default state needs no row: a message nothing has touched is in the
    // inbox. This is what makes a missed hook harmless.
    assert!(!is_archived(&t, a).await?);

    archive(&t, &[a]).await?;
    assert!(is_archived(&t, a).await?);

    unarchive(&t, &[a]).await?;
    assert!(!is_archived(&t, a).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_deleting_a_label_keeps_the_messages() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;
    let work = create(&t, "Work", None).await?;
    apply(&t, &[a], work.id).await?;

    delete(&t, work.id).await?;

    assert!(by_name(&t, "Work").await?.is_none());
    assert!(of_msg(&t, a).await?.is_empty());
    assert!(
        crate::message::Message::load_from_db(&t, a).await.is_ok(),
        "deleting a label is organizational, never destructive"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_colors() -> Result<()> {
    let t = TestContext::new_alice().await;
    let work = create(&t, "Work", Some(0xff8800)).await?;
    assert_eq!(by_name(&t, "Work").await?.unwrap().color, Some(0xff8800));

    set_color(&t, work.id, None).await?;
    assert_eq!(by_name(&t, "Work").await?.unwrap().color, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prune_drops_labels_of_deleted_messages() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;
    let work = create(&t, "Work", None).await?;
    apply(&t, &[a], work.id).await?;

    crate::message::delete_msgs(&t, &[a]).await?;
    prune(&t).await?;

    assert!(msgs_with(&t, work.id).await?.is_empty());
    assert!(
        by_name(&t, "Work").await?.is_some(),
        "the label itself must survive its last message"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Device sync
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sync_create_rename_and_delete() -> Result<()> {
    let t = TestContext::new_alice().await;

    execute_sync_item(
        &t,
        &LabelSyncItem::Create {
            name: "Work".to_string(),
            color: Some(0x112233),
        },
        0,
    )
    .await?;
    assert_eq!(by_name(&t, "work").await?.unwrap().color, Some(0x112233));

    execute_sync_item(
        &t,
        &LabelSyncItem::Rename {
            from: "Work".to_string(),
            to: "Job".to_string(),
        },
        0,
    )
    .await?;
    assert!(by_name(&t, "Work").await?.is_none());
    assert!(by_name(&t, "Job").await?.is_some());

    execute_sync_item(
        &t,
        &LabelSyncItem::Delete {
            name: "Job".to_string(),
        },
        0,
    )
    .await?;
    assert!(by_name(&t, "Job").await?.is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sync_rename_of_an_unknown_label_still_lands() -> Result<()> {
    let t = TestContext::new_alice().await;
    // If the creation was lost, the devices must still converge rather than
    // one of them silently lacking the label.
    execute_sync_item(
        &t,
        &LabelSyncItem::Rename {
            from: "NeverSeen".to_string(),
            to: "Job".to_string(),
        },
        0,
    )
    .await?;
    assert!(by_name(&t, "Job").await?.is_some());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sync_apply_creates_the_label_it_names() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x").await?;

    execute_sync_item(
        &t,
        &LabelSyncItem::Apply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        0,
    )
    .await?;

    let names: Vec<String> = of_msg(&t, a).await?.into_iter().map(|l| l.name).collect();
    assert_eq!(names, vec!["Work"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sync_arriving_before_the_message_is_not_lost() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // The label change arrives first. Core's own sync handlers would warn and
    // drop here; we park it instead.
    execute_sync_item(
        &t,
        &LabelSyncItem::Apply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        1000,
    )
    .await?;

    let a = recv(&t, "a@x").await?;
    let names: Vec<String> = of_msg(&t, a).await?.into_iter().map(|l| l.name).collect();
    assert_eq!(
        names,
        vec!["Work"],
        "a label applied on another device must survive arriving early"
    );

    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM pending_msg_labels", ())
            .await?,
        "the parked change must be consumed, not replayed forever"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_parked_changes_settle_on_the_latest_intent() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    execute_sync_item(
        &t,
        &LabelSyncItem::Apply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        1000,
    )
    .await?;
    // The user changed their mind before the message ever reached this device.
    execute_sync_item(
        &t,
        &LabelSyncItem::Unapply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        2000,
    )
    .await?;

    let a = recv(&t, "a@x").await?;
    assert!(
        of_msg(&t, a).await?.is_empty(),
        "the later change must win, not the one that arrived first"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_out_of_order_older_change_does_not_win() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    execute_sync_item(
        &t,
        &LabelSyncItem::Unapply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        2000,
    )
    .await?;
    // Arrives late but happened earlier; it must not override.
    execute_sync_item(
        &t,
        &LabelSyncItem::Apply {
            msgs: vec!["a@x".to_string()],
            label: "Work".to_string(),
        },
        1000,
    )
    .await?;

    let a = recv(&t, "a@x").await?;
    assert!(of_msg(&t, a).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_parked_changes_expire() -> Result<()> {
    let t = TestContext::new_alice().await;
    execute_sync_item(
        &t,
        &LabelSyncItem::Apply {
            msgs: vec!["never-arrives@x".to_string()],
            label: "Work".to_string(),
        },
        // Older than the TTL: the message is never coming.
        crate::tools::time() - PENDING_TTL - 1,
    )
    .await?;
    assert!(
        t.sql
            .exists("SELECT COUNT(*) FROM pending_msg_labels", ())
            .await?
    );

    prune(&t).await?;
    // Discarding a label the user set is worth saying out loud.
    t.assert_warn("never arrived").await;

    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM pending_msg_labels", ())
            .await?,
        "changes for messages that never arrive must not accumulate forever"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_sync_does_not_echo() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.set_config_bool(crate::config::Config::BccSelf, true)
        .await?;
    t.sql.execute("DELETE FROM multi_device_sync", ()).await?;

    execute_sync_item(
        &t,
        &LabelSyncItem::Create {
            name: "Work".to_string(),
            color: None,
        },
        0,
    )
    .await?;

    // Replaying a change must not queue it straight back out: two devices
    // echoing each other would loop forever.
    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM multi_device_sync", ())
            .await?,
        "executing a sync item must not produce one"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_labels_reach_the_other_device() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice
        .set_config_bool(crate::config::Config::SyncMsgs, true)
        .await?;

    create(&alice, "Work", Some(0x445566)).await?;
    alice.send_sync_msg().await?.unwrap();
    let sent = alice.pop_sent_msg().await;

    let alice2 = TestContext::new_alice().await;
    alice2
        .set_config_bool(crate::config::Config::SyncMsgs, true)
        .await?;
    alice2.recv_msg_trash(&sent).await;

    let label = by_name(&alice2, "Work").await?.expect("label must sync");
    assert_eq!(label.color, Some(0x445566));
    assert!(!label.is_system);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_archiving_reaches_the_other_device() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    alice
        .set_config_bool(crate::config::Config::SyncMsgs, true)
        .await?;
    let a = recv(&alice, "a@x").await?;
    archive(&alice, &[a]).await?;

    alice.send_sync_msg().await?.unwrap();
    let sent = alice.pop_sent_msg().await;

    let alice2 = TestContext::new_alice().await;
    alice2.allow_unencrypted().await?;
    alice2
        .set_config_bool(crate::config::Config::SyncMsgs, true)
        .await?;
    // The other device already has the message.
    let a2 = recv(&alice2, "a@x").await?;
    alice2.recv_msg_trash(&sent).await;

    assert!(
        is_archived(&alice2, a2).await?,
        "archiving is Apply of the reserved label, so it syncs like any other"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_label_names_never_reach_the_server_in_cleartext() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice
        .set_config_bool(crate::config::Config::SyncMsgs, true)
        .await?;

    create(&alice, "Divorce lawyers", None).await?;
    alice.send_sync_msg().await?.unwrap();
    let sent = alice.pop_sent_msg().await;

    assert!(
        !sent.payload().contains("Divorce lawyers"),
        "a label name is as sensitive as a subject and must not appear in cleartext"
    );
    Ok(())
}
