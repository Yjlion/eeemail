//! Tests for the outgoing signature.

use anyhow::Result;

use crate::email::importance::Importance;

use super::*;
use crate::email::compose::{RecipientSet, send};
use crate::email::policy::EncryptionMode;
use crate::test_utils::{TestContext, TestContextManager};

/// An account set up the way eeemail sets one up.
///
/// `TestContext` carries upstream's strict default, which refuses to send to
/// anyone whose key we do not hold -- and a signature is a property of a
/// message this account writes, whether or not the recipient has a key. See
/// `email::policy::apply_defaults`.
async fn opportunistic(context: &TestContext) -> Result<()> {
    EncryptionMode::set(context, EncryptionMode::Opportunistic).await?;
    Ok(())
}

/// Sends one message from `alice` to `to` and returns what went on the wire.
async fn sent_payload(alice: &TestContext, to: &str, text: &str, html: Option<&str>) -> String {
    send(
        alice,
        &RecipientSet {
            to: vec![to.to_string()],
            ..Default::default()
        },
        "Subject",
        text,
        None,
        html,
        Importance::Normal,
    )
    .await
    .expect("send failed");
    readable(alice.pop_sent_msg().await.payload())
}

/// The payload with its quoted-printable escapes undone, for substring checks.
///
/// The RFC 3676 separator goes on the wire as `--=20`, because a trailing space
/// is not literal in quoted-printable; `class="signature"` goes as
/// `class=3D"signature"`; and a long line is split by a soft break. Asserting
/// on the raw bytes would be asserting on the transfer encoding, which is not
/// what any of these tests is about.
///
/// Deliberately not a decoder -- it joins headers that were never
/// quoted-printable in the first place. Good enough to look for a substring in,
/// and nothing here should ask it for more than that.
fn readable(payload: &str) -> String {
    payload
        .replace("=\r\n", "")
        .replace("=20", " ")
        .replace("=3D", "=")
}

#[test]
fn test_a_plain_signature_becomes_escaped_html() {
    // `&`, `<` and `>` in a signature are an ampersand and two comparison
    // signs, not markup: a company called "Smith & Sons <Ltd>" must not
    // silently open a tag in every message they send.
    let html = default_html("Ada Lovelace\nSmith & Sons <Ltd>");
    assert_eq!(
        html,
        "<pre class=\"signature\">Ada Lovelace\nSmith &amp; Sons &lt;Ltd&gt;</pre>"
    );
}

#[test]
fn test_a_signature_goes_inside_the_body_of_a_document() {
    // Composed HTML is a fragment and forwarded HTML can be a whole document.
    // Appending after `</body>` puts the signature outside the body, where a
    // strict renderer is entitled to drop it.
    let signature = Signature {
        plain: "Ada".to_string(),
        html: "<p>Ada</p>".to_string(),
    };
    let document = append_to_html("<html><body><p>hi</p></body></html>", &signature);
    assert!(
        document.ends_with("<p>Ada</p></body></html>"),
        "signature landed outside the body: {document}"
    );

    let fragment = append_to_html("<p>hi</p>", &signature);
    assert!(
        fragment.ends_with("<p>Ada</p>"),
        "signature missing from a fragment: {fragment}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_signature_that_is_only_whitespace_is_no_signature() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    // Someone who cleared the box by selecting the text and pressing space has
    // no signature. Treating it as one puts a bare `-- ` separator with nothing
    // after it on every message they send.
    alice
        .set_config(Config::EmailSignature, Some("   \n  "))
        .await?;
    assert_eq!(load(&alice).await?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_signature_leaves_the_message_as_upstream_renders_it() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    opportunistic(&alice).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();

    let payload = sent_payload(&alice, &bob_addr, "the body", None).await;

    // The separator is what upstream emits for a status, and an account with
    // neither must not grow one. This is the property that keeps every
    // unaffected message byte-identical to what upstream produces.
    assert!(
        !payload.contains("-- \r\n"),
        "an account with no signature emitted a footer separator:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_signature_is_appended_once() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    opportunistic(&alice).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();
    alice
        .set_config(
            Config::EmailSignature,
            Some("Ada Lovelace\nAnalytical Engines"),
        )
        .await?;

    let payload = sent_payload(&alice, &bob_addr, "the body", None).await;

    assert_eq!(
        payload.matches("Analytical Engines").count(),
        1,
        "the signature appears more than once:\n{payload}"
    );
    assert!(
        payload.contains("-- \r\nAda Lovelace"),
        "the signature is not behind an RFC 3676 separator:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_signature_displaces_the_status_rather_than_joining_it() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    opportunistic(&alice).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();
    alice
        .set_config(Config::Selfstatus, Some("sent with a messenger"))
        .await?;
    alice
        .set_config(Config::EmailSignature, Some("Ada Lovelace"))
        .await?;

    let payload = sent_payload(&alice, &bob_addr, "the body", None).await;

    // Only one block can follow the separator. Emitting both would append the
    // status to the signature, which reads as part of it.
    assert!(payload.contains("Ada Lovelace"), "no signature:\n{payload}");
    assert!(
        !payload.contains("sent with a messenger"),
        "the status went out beside the signature:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_html_message_carries_the_signature_in_both_parts() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    opportunistic(&alice).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();
    alice
        .set_config(Config::EmailSignature, Some("Ada Lovelace"))
        .await?;

    let payload = sent_payload(&alice, &bob_addr, "the body", Some("<p>the body</p>")).await;

    // Twice: once in `text/plain`, once in `text/html`. The plain part is
    // never optional, and it is also not the part most recipients are shown.
    assert_eq!(
        payload.matches("Ada Lovelace").count(),
        2,
        "the signature is not in both alternatives:\n{payload}"
    );
    assert!(
        payload.contains("<pre class=\"signature\">"),
        "the HTML part did not get the derived markup:\n{payload}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_configured_html_replaces_the_derived_markup() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    opportunistic(&alice).await?;
    let bob_addr = bob.get_config(Config::Addr).await?.unwrap();
    alice
        .set_config(Config::EmailSignature, Some("Ada Lovelace"))
        .await?;
    alice
        .set_config(
            Config::EmailSignatureHtml,
            Some("<p><b>Ada Lovelace</b></p>"),
        )
        .await?;

    let payload = sent_payload(&alice, &bob_addr, "the body", Some("<p>the body</p>")).await;

    assert!(
        payload.contains("<b>Ada Lovelace</b>"),
        "the configured HTML signature was not used:\n{payload}"
    );
    assert!(
        !payload.contains("<pre class=\"signature\">"),
        "the derived markup was emitted alongside the configured one:\n{payload}"
    );
    // The plain part still carries the plain signature: setting markup does not
    // remove the signature from the alternative that is never optional.
    assert!(
        payload.contains("-- \r\nAda Lovelace"),
        "the plain part lost its signature:\n{payload}"
    );
    Ok(())
}
