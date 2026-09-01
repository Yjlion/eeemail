//! Tests for encrypted backup.

use anyhow::Result;

use super::*;
use crate::test_utils::TestContext;
use deltachat_time::SystemTimeTools as SystemTime;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_never_backed_up_is_stale() -> Result<()> {
    let t = TestContext::new_alice().await;
    let status = status(&t).await?;
    assert_eq!(status.last_backup, None);
    assert!(
        status.stale,
        "never having backed up is the staler state, not a neutral one: it is \
         where a lost device costs the whole mailbox"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_empty_passphrase_is_refused() -> Result<()> {
    let t = TestContext::new_alice().await;
    let dir = tempfile::tempdir()?;
    // Core permits an empty passphrase, which writes the whole mailbox to a
    // file in the clear. That must not be reachable by leaving a field blank.
    assert!(export(&t, dir.path(), "").await.is_err());
    assert!(
        std::fs::read_dir(dir.path())?.next().is_none(),
        "nothing should have been written"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_export_records_when_it_happened() -> Result<()> {
    let t = TestContext::new_alice().await;
    let dir = tempfile::tempdir()?;

    export(&t, dir.path(), "correct horse battery staple").await?;

    let status = status(&t).await?;
    assert!(status.last_backup.is_some());
    assert!(!status.stale, "a backup just taken is not stale");

    let files: Vec<_> = std::fs::read_dir(dir.path())?
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1, "one backup file");
    assert!(std::fs::metadata(files[0].path())?.len() > 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_backup_goes_stale_after_a_week() -> Result<()> {
    let t = TestContext::new_alice().await;
    let dir = tempfile::tempdir()?;
    export(&t, dir.path(), "passphrase").await?;
    assert!(!status(&t).await?.stale);

    // Six days on: still current.
    SystemTime::shift(Duration::from_secs(6 * 86_400));
    assert!(!status(&t).await?.stale);

    // Past a week.
    SystemTime::shift(Duration::from_secs(2 * 86_400));
    assert!(status(&t).await?.stale);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_backup_round_trip() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let raw = b"From: bob@example.net\r\n\
To: alice@example.org\r\n\
Subject: survives a restore\r\n\
Message-ID: <backup-1@example.net>\r\n\
Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
\r\n\
body\r\n";
    crate::receive_imf::receive_imf(&alice, raw, false)
        .await?
        .unwrap();

    let dir = tempfile::tempdir()?;
    export(&alice, dir.path(), "passphrase").await?;
    let file = std::fs::read_dir(dir.path())?.next().unwrap()?.path();

    // The local store is the only durable copy of the mailbox, so a restore
    // that loses mail is the failure this whole feature exists to prevent.
    let restored = TestContext::new().await;
    import(&restored, &file, "passphrase").await?;

    let msg_id = crate::message::rfc724_mid_exists(&restored, "backup-1@example.net")
        .await?
        .expect("the message must survive the round trip");
    let msg = crate::message::Message::load_from_db(&restored, msg_id).await?;
    assert_eq!(msg.get_subject(), "survives a restore");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_wrong_passphrase_does_not_restore() -> Result<()> {
    let alice = TestContext::new_alice().await;
    let dir = tempfile::tempdir()?;
    export(&alice, dir.path(), "right passphrase").await?;
    let file = std::fs::read_dir(dir.path())?.next().unwrap()?.path();

    let restored = TestContext::new().await;
    assert!(import(&restored, &file, "wrong passphrase").await.is_err());
    // Refusing to decrypt is the correct outcome, and it is loud on purpose:
    // silently restoring nothing would look like an empty mailbox.
    restored.assert_error("not a database").await;
    restored.assert_warn("IMEX failed to complete").await;
    Ok(())
}
