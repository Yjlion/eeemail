//! Tests for structured email extraction and its trust verdict.

use anyhow::Result;

use super::*;
use crate::receive_imf::receive_imf;
use crate::test_utils::{TestContext, TestContextManager};

const OFFER: &str =
    r#"{"@context":"https://schema.org","@type":"ParcelDelivery","trackingNumber":"XY1"}"#;

/// A cleartext message whose body is `body`, with the given top-level headers.
fn mail(mid: &str, headers: &str, body: &str) -> Vec<u8> {
    format!(
        "From: <sender@example.net>\n\
         To: <alice@example.org>\n\
         Message-ID: <{mid}>\n\
         Subject: your parcel\n\
         Date: Mon, 1 Sep 2026 12:00:00 +0000\n\
         {headers}\n\
         \n\
         {body}"
    )
    .into_bytes()
}

/// A `multipart/<subtype>` message carrying one machine-readable JSON-LD part.
fn sml(mid: &str, subtype: &str, json: &str) -> Vec<u8> {
    mail(
        mid,
        &format!("MIME-Version: 1.0\nContent-Type: multipart/{subtype}; boundary=\"b\""),
        &format!(
            "--b\n\
             Content-Type: text/plain\n\
             \n\
             Your parcel is on its way.\n\
             --b\n\
             Content-Type: application/ld+json\n\
             Content-Purpose: Machine-readable\n\
             \n\
             {json}\n\
             --b--\n"
        ),
    )
}

async fn recv(t: &TestContext, raw: &[u8]) -> Result<MsgId> {
    let received = receive_imf(t, raw, false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

/// Receives a message that carries an `application/ld+json` part.
///
/// Core logs "Missing attachment" and drops that part, because it has no
/// filename. That is the whole reason this module re-walks the raw MIME
/// instead of reading `MimeMessage::parts`, so the warning is expected and
/// asserted rather than tolerated.
async fn recv_sml(t: &TestContext, raw: &[u8]) -> Result<MsgId> {
    let msg_id = recv(t, raw).await?;
    t.assert_warn("Missing attachment").await;
    Ok(msg_id)
}

async fn objects(t: &TestContext, msg_id: MsgId) -> Result<Vec<StructuredObject>> {
    of_msg(t, msg_id).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_machine_readable_part_is_extracted() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let msg_id = recv_sml(&alice, &sml("s1@example.net", "mixed", OFFER)).await?;
    let found = objects(&alice, msg_id).await?;

    assert_eq!(found.len(), 1);
    assert!(found[0].json.contains("ParcelDelivery"));
    Ok(())
}

#[test]
fn test_all_three_arrangements_are_recognised() {
    // The arrangement is SML's way of saying whether the data is a full,
    // partial or unrelated representation of the body, and it cannot be
    // recovered once the message is stored. Checked directly on the bytes:
    // what core makes of these parts afterwards is a separate question, and
    // one this module exists to route around.
    for (subtype, expected) in [
        ("alternative", Source::Alternative),
        ("related", Source::Related),
        ("mixed", Source::Mixed),
    ] {
        let raw = sml("arr@example.net", subtype, OFFER);
        let found = objects_in(&raw);
        assert_eq!(found.len(), 1, "nothing extracted from multipart/{subtype}");
        assert_eq!(found[0].2, expected, "wrong arrangement for {subtype}");
    }
}

#[test]
fn test_the_html_scan_survives_multibyte_text() {
    // The scan used to index a lowercased copy of the body, which can differ
    // in byte length from the original. Any message with a Turkish dotted
    // capital or similar would then slice mid-character.
    let html = format!(
        "<html><body><p>İstanbul — Ünicode ✉</p>\
         <script type=\"application/ld+json\">{OFFER}</script></body></html>"
    );
    let raw = mail(
        "mb@example.net",
        "MIME-Version: 1.0\nContent-Type: text/html; charset=utf-8",
        &html,
    );
    let found = objects_in(&raw);

    assert_eq!(found.len(), 1, "multi-byte text broke the scan");
    assert_eq!(found[0].2, Source::HtmlScript);
    assert!(found[0].1.contains("ParcelDelivery"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_ld_json_part_without_content_purpose_is_ignored() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let raw = mail(
        "np@example.net",
        "MIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"",
        &format!(
            "--b\n\
             Content-Type: text/plain\n\
             \n\
             hello\n\
             --b\n\
             Content-Type: application/ld+json\n\
             \n\
             {OFFER}\n\
             --b--\n"
        ),
    );
    let msg_id = recv_sml(&alice, &raw).await?;

    // `Content-Purpose` is what distinguishes structured data from a JSON file
    // somebody attached. Without it this is an attachment, not a claim.
    assert!(objects(&alice, msg_id).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_content_purpose_matching_ignores_case_and_parameters() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let raw = mail(
        "cp@example.net",
        "MIME-Version: 1.0\nContent-Type: multipart/mixed; boundary=\"b\"",
        &format!(
            "--b\n\
             Content-Type: text/plain\n\
             \n\
             hello\n\
             --b\n\
             Content-Type: Application/LD+JSON\n\
             Content-Purpose: machine-readable; v=1\n\
             \n\
             {OFFER}\n\
             --b--\n"
        ),
    );
    let msg_id = recv_sml(&alice, &raw).await?;

    assert_eq!(objects(&alice, msg_id).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_html_script_fallback_is_used_only_when_there_is_no_mime_part() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    // What deployed senders actually emit today.
    let html = mail(
        "h1@example.net",
        "MIME-Version: 1.0\nContent-Type: text/html",
        &format!(
            "<html><body><p>hi</p><script type=\"application/ld+json\">{OFFER}</script></body></html>\n"
        ),
    );
    let msg_id = recv(&alice, &html).await?;
    let found = objects(&alice, msg_id).await?;
    assert_eq!(found.len(), 1, "the Schema.org fallback was not read");
    assert_eq!(found[0].source, Source::HtmlScript);

    // A sender who emits both is read through the mechanism they specified.
    let both = mail(
        "h2@example.net",
        "MIME-Version: 1.0\nContent-Type: multipart/alternative; boundary=\"b\"",
        &format!(
            "--b\n\
             Content-Type: text/html\n\
             \n\
             <html><body><script type=\"application/ld+json\">{OFFER}</script></body></html>\n\
             --b\n\
             Content-Type: application/ld+json\n\
             Content-Purpose: Machine-readable\n\
             \n\
             {OFFER}\n\
             --b--\n"
        ),
    );
    let msg_id = recv_sml(&alice, &both).await?;
    let found = objects(&alice, msg_id).await?;
    assert_eq!(found.len(), 1, "the fallback ran alongside the real thing");
    assert_eq!(found[0].source, Source::Alternative);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_graph_becomes_one_row_per_member() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let graph = r#"{"@context":"https://schema.org","@graph":[{"@type":"ParcelDelivery"},{"@type":"Organization"}]}"#;
    let msg_id = recv_sml(&alice, &sml("g1@example.net", "mixed", graph)).await?;
    let found = objects(&alice, msg_id).await?;

    assert_eq!(found.len(), 2, "a @graph is several objects, not one");
    assert!(found[0].json.contains("ParcelDelivery"));
    assert!(found[1].json.contains("Organization"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_unknown_schema_type_is_kept_not_dropped() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    // The vocabulary is large and grows. A client that stores only what it
    // recognises silently discards the rest of the message's meaning.
    let exotic = r#"{"@context":"https://schema.org","@type":"BorrowAction","object":"a ladder"}"#;
    let msg_id = recv_sml(&alice, &sml("u1@example.net", "mixed", exotic)).await?;
    let found = objects(&alice, msg_id).await?;

    assert_eq!(found.len(), 1);
    assert!(found[0].json.contains("BorrowAction"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_malformed_json_does_not_fail_reception() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    // Load-bearing: an enhancement must never be able to lose a message.
    let msg_id = recv_sml(&alice, &sml("m1@example.net", "mixed", "{not json at all")).await?;

    let msg = crate::message::Message::load_from_db(&alice, msg_id).await?;
    assert_eq!(msg.get_subject(), "your parcel", "the message was lost");
    assert!(objects(&alice, msg_id).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_from_a_stranger_is_stored_untrusted() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let msg_id = recv_sml(&alice, &sml("t1@example.net", "mixed", OFFER)).await?;
    let found = objects(&alice, msg_id).await?;

    // Parsed for everyone, acted on for nobody unproven. This is the whole ADR.
    assert_eq!(found.len(), 1);
    assert!(!found[0].trusted);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cleartext_from_a_known_contact_is_not_trusted() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    // Known, by the same rule gating uses.
    let contact = crate::contact::Contact::create(&alice, "Sender", "sender@example.net").await?;
    assert!(super::super::gating::is_trusted(&alice, contact).await?);

    let msg_id = recv_sml(&alice, &sml("t2@example.net", "mixed", OFFER)).await?;

    // Knowing someone is not evidence that a cleartext message came from them:
    // `From` is trivially spoofed and core checks no DKIM. Half the predicate
    // is not the predicate.
    assert!(
        !objects(&alice, msg_id).await?[0].trusted,
        "cleartext was trusted because the address was familiar"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_verdict_is_true_for_an_encrypted_message_from_a_known_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // Encrypted, signed, and accepted -- both halves of the predicate.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let msg_id = alice.get_last_msg().await.get_id();

    let crypto = super::super::policy::message_crypto(&alice, msg_id).await?;
    assert!(
        crypto.encrypted && crypto.signed,
        "test setup did not produce an encrypted, signed message"
    );
    // The positive case. Without it the predicate could be `false` and every
    // other test here would still pass.
    assert!(
        super::is_trusted(&alice, msg_id).await?,
        "an encrypted, signed message from an accepted contact was not trusted"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rows_go_when_the_message_goes() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let msg_id = recv_sml(&alice, &sml("d1@example.net", "mixed", OFFER)).await?;
    assert_eq!(objects(&alice, msg_id).await?.len(), 1);

    alice
        .sql
        .execute("DELETE FROM msgs WHERE id=?", (msg_id,))
        .await?;
    prune(&alice).await?;

    assert!(
        objects(&alice, msg_id).await?.is_empty(),
        "the table outlived what it describes"
    );
    Ok(())
}
