//! Tests for the composer's per-message choices.

use anyhow::Result;

use super::*;
use crate::config::Config;
use crate::email::compose::send;
use crate::email::importance::Importance;
use crate::test_utils::{TestContext, TestContextManager};

/// Sends `text` from `alice` to `to` with `options`, and returns what went on
/// the wire with its quoted-printable escapes undone.
async fn send_with(
    alice: &TestContext,
    set: &RecipientSet,
    text: &str,
    html: Option<&str>,
    options: SendOptions,
) -> Result<String> {
    send(
        alice,
        set,
        "Subject",
        text,
        None,
        html,
        Importance::Normal,
        &options,
    )
    .await?;
    Ok(alice
        .pop_sent_msg()
        .await
        .payload()
        .replace("=\r\n", "")
        .replace("=20", " ")
        .replace("=3D", "="))
}

fn to(addr: &str) -> RecipientSet {
    RecipientSet {
        to: vec![addr.to_string()],
        ..Default::default()
    }
}

const PLAINTEXT: SendOptions = SendOptions {
    encryption: EncryptionChoice::Plaintext,
    signature_in_body: false,
};

const REQUIRED: SendOptions = SendOptions {
    encryption: EncryptionChoice::Required,
    signature_in_body: false,
};

async fn smtp_rows(t: &TestContext) -> Result<usize> {
    t.sql.count("SELECT COUNT(*) FROM smtp", ()).await
}

#[test]
fn test_split_signature() {
    assert_eq!(
        split_signature("hello\n\n-- \nAda\nLondon"),
        ("hello".to_string(), Some("Ada\nLondon".to_string()))
    );
    // The last separator: an earlier `-- ` line is the user's text.
    assert_eq!(
        split_signature("a\n-- \nb\n-- \nAda"),
        ("a\n-- \nb".to_string(), Some("Ada".to_string()))
    );
    // A reply's quote under the signature goes with it, in the order written.
    assert_eq!(
        split_signature("hi\n\n-- \nAda\n\n> quoted"),
        ("hi".to_string(), Some("Ada\n\n> quoted".to_string()))
    );
    assert_eq!(
        split_signature("no signature"),
        ("no signature".to_string(), None)
    );
    assert_eq!(split_signature("hi\n-- \n  "), ("hi".to_string(), None));
    // A quoted separator is not ours.
    assert_eq!(
        split_signature("hi\n> -- \n> Bob"),
        ("hi\n> -- \n> Bob".to_string(), None)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_default_stores_nothing() -> Result<()> {
    let t = TestContext::new_alice().await;
    let msg_id = MsgId::new(4242);
    set(&t, msg_id, &PLAINTEXT, None).await?;
    assert_eq!(load(&t, msg_id).await?, PLAINTEXT);

    // Back to the default removes the row, so an ordinary message costs
    // nothing and reads the same as one sent before this table existed.
    set(&t, msg_id, &SendOptions::default(), None).await?;
    assert_eq!(load(&t, msg_id).await?, SendOptions::default());
    assert_eq!(
        t.sql
            .count("SELECT COUNT(*) FROM msg_send_options", ())
            .await?,
        0
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_plaintext_goes_out_unencrypted_to_someone_with_a_key() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    // Gives alice bob's key, so without the padlock this would be encrypted.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    let auto = send_with(
        &alice,
        &to(&bob_addr),
        "sealed",
        None,
        SendOptions::default(),
    )
    .await?;
    assert!(auto.contains("BEGIN PGP MESSAGE"), "baseline not encrypted");

    let open = send_with(&alice, &to(&bob_addr), "in the open", None, PLAINTEXT).await?;
    assert!(
        !open.contains("BEGIN PGP MESSAGE"),
        "the padlock said cleartext and the message was encrypted:\n{open}"
    );
    assert!(open.contains("in the open"), "body missing:\n{open}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_plaintext_is_refused_where_the_account_is_strict() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Strict).await?;
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    let err = send(
        &alice,
        &to(&bob_addr),
        "Subject",
        "must not leave",
        None,
        None,
        Importance::Normal,
        &PLAINTEXT,
    )
    .await
    .expect_err("cleartext was accepted under end-to-end only");
    assert!(err.to_string().contains("end-to-end only"), "{err:#}");
    // Refused before anything was stored, so nothing is queued either.
    assert_eq!(smtp_rows(&alice).await?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_plaintext_is_refused_where_a_contact_is_strict() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();
    let bob_key_contact = crate::email::compose::key_contact_for(&alice, &bob_addr)
        .await?
        .expect("no key-contact for bob");
    EncryptionMode::set_for_contact(&alice, bob_key_contact, Some(EncryptionMode::Strict)).await?;

    // In Cc rather than To, which is where a check on chat members alone would
    // miss it.
    let set = RecipientSet {
        to: vec!["fiona@example.net".to_string()],
        cc: vec![bob_addr],
        ..Default::default()
    };
    assert!(readiness(&alice, &set).await?.locked);
    let err = send(
        &alice,
        &set,
        "Subject",
        "must not leave",
        None,
        None,
        Importance::Normal,
        &PLAINTEXT,
    )
    .await
    .expect_err("cleartext was accepted to a contact set to end-to-end only");
    assert!(err.to_string().contains("end-to-end only"), "{err:#}");
    assert_eq!(smtp_rows(&alice).await?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_required_names_whoever_has_no_key() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    // Bcc: the recipient nobody else sees, and the one an opportunistic send
    // would drop without a word.
    let set = RecipientSet {
        to: vec![bob_addr],
        bcc: vec!["fiona@example.net".to_string()],
        ..Default::default()
    };
    let err = send(
        &alice,
        &set,
        "Subject",
        "for keyholders only",
        None,
        None,
        Importance::Normal,
        &REQUIRED,
    )
    .await
    .expect_err("an encrypted-only message was accepted for someone with no key");
    assert!(err.to_string().contains("fiona@example.net"), "{err:#}");
    assert_eq!(smtp_rows(&alice).await?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_required_encrypts_when_everyone_has_a_key() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    EncryptionMode::set(&alice, EncryptionMode::Lenient).await?;
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    let payload = send_with(&alice, &to(&bob_addr), "sealed", None, REQUIRED).await?;
    assert!(payload.contains("BEGIN PGP MESSAGE"), "{payload}");
    assert!(!payload.contains("sealed"), "body on the wire:\n{payload}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_readiness_creates_no_contacts() -> Result<()> {
    let t = TestContext::new_alice().await;
    EncryptionMode::set(&t, EncryptionMode::Opportunistic).await?;
    let before = t.sql.count("SELECT COUNT(*) FROM contacts", ()).await?;

    // What a composer sends while someone is still typing.
    let set = RecipientSet {
        to: vec!["Fiona <fiona@example.net>".to_string(), "fio".to_string()],
        cc: vec!["  ".to_string()],
        ..Default::default()
    };
    let ready = readiness(&t, &set).await?;

    assert_eq!(
        t.sql.count("SELECT COUNT(*) FROM contacts", ()).await?,
        before,
        "asking the padlock's question added contacts"
    );
    assert_eq!(ready.missing, vec!["Fiona <fiona@example.net>", "fio"]);
    assert_eq!(ready.mode, EncryptionMode::Opportunistic);
    assert!(!ready.locked);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_readiness_is_locked_under_strict() -> Result<()> {
    let t = TestContext::new_alice().await;
    EncryptionMode::set(&t, EncryptionMode::Strict).await?;
    let ready = readiness(&t, &to("fiona@example.net")).await?;
    assert!(ready.locked);
    assert_eq!(ready.mode, EncryptionMode::Strict);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_signature_in_the_body_is_not_appended_again() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    EncryptionMode::set(&alice, EncryptionMode::Opportunistic).await?;
    alice
        .set_config(Config::EmailSignature, Some("Ada Lovelace"))
        .await?;
    let recipient = to("fiona@example.net");
    let text = "hello\n\n-- \nAda Lovelace";
    let html = "<p>hello</p><p>-- </p><p><b>Ada Lovelace</b></p>";

    // The default still signs: the composer is not the only sender, and a
    // client that does not say it placed the signature gets it appended.
    let appended = send_with(&alice, &recipient, "hello", None, SendOptions::default()).await?;
    assert_eq!(appended.matches("Ada Lovelace").count(), 1, "{appended}");

    let in_body = SendOptions {
        signature_in_body: true,
        ..Default::default()
    };
    let plain = send_with(&alice, &recipient, text, None, in_body).await?;
    assert_eq!(
        plain.matches("Ada Lovelace").count(),
        1,
        "signed twice:\n{plain}"
    );
    // A real separator, not the escaped one upstream writes into text: a
    // `-\u{200B}- ` is a signature no recipient's client recognises.
    assert!(
        plain.contains("hello\r\n\r\n-- \r\nAda Lovelace"),
        "the separator did not survive:\n{plain}"
    );

    // Removed in the composer stays removed: nothing is appended behind the
    // user's back.
    let unsigned = send_with(&alice, &recipient, "hello", None, in_body).await?;
    assert!(
        !unsigned.contains("Ada Lovelace") && !unsigned.contains("-- "),
        "a signature the user removed was put back:\n{unsigned}"
    );

    // Both parts: once in the plain text, once in the HTML, never a third
    // time from the renderer.
    let formatted = send_with(&alice, &recipient, text, Some(html), in_body).await?;
    assert_eq!(
        formatted.matches("Ada Lovelace").count(),
        2,
        "signed twice:\n{formatted}"
    );
    assert!(
        !formatted.contains("signature-sep"),
        "the renderer appended its own signature block:\n{formatted}"
    );
    Ok(())
}
