//! Tests for the blocklist.

use anyhow::Result;

use super::*;
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;

use super::super::tags::{self, SystemTag};

/// A minimal message from `addr`, as bytes, for `receive_imf`.
fn mail_from(addr: &str, mid: &str) -> Vec<u8> {
    format!(
        "From: <{addr}>\r\n\
         To: <alice@example.org>\r\n\
         Subject: hello\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 8 Sep 2026 10:00:00 +0000\r\n\
         \r\n\
         body\r\n"
    )
    .into_bytes()
}

/// Receives one cleartext message and returns its id.
///
/// `allow_unencrypted` because `TestContext` carries upstream's strict default,
/// which drops a cleartext message at `receive_imf` before anything in this
/// module could see it. Blocking is about who sent a message, not how.
async fn recv(t: &TestContext, addr: &str, mid: &str) -> Result<MsgId> {
    t.allow_unencrypted().await?;
    let received = receive_imf(t, &mail_from(addr, mid), false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

/// Whether a message ended up in the trash.
async fn in_trash(t: &TestContext, msg_id: MsgId) -> Result<bool> {
    Ok(tags::messages(t, SystemTag::Trash).await?.contains(&msg_id))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_pattern_that_can_never_match_is_refused() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Stored and never fired is the worst outcome: the user believes the
    // blocklist is protecting them from something it cannot see.
    assert!(add(&t, "notanaddress", "").await.is_err());
    assert!(add(&t, "@", "").await.is_err());
    assert!(add(&t, "@nodot", "").await.is_err());
    assert!(add(&t, "a@b@c.com", "").await.is_err());
    assert!(add(&t, "   ", "").await.is_err());
    assert!(add(&t, "spam@example.com", "").await.is_ok());
    assert!(add(&t, "@example.com", "").await.is_ok());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_matching_ignores_case_on_both_sides() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "Spam@Example.COM", "").await?;
    assert!(matches(&t, "spam@example.com").await?);
    assert!(matches(&t, "SPAM@EXAMPLE.COM").await?);
    // And the same entry cannot be added twice in a different case, which
    // would otherwise leave a blocklist the user cannot fully remove from.
    add(&t, "spam@example.com", "").await?;
    assert_eq!(list(&t).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_domain_pattern_does_not_reach_a_subdomain() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "@example.com", "").await?;
    assert!(matches(&t, "anyone@example.com").await?);
    // Deliberate, and the module docs say why: a rule whose reach the user
    // cannot predict is one they cannot undo selectively.
    assert!(!matches(&t, "anyone@mail.example.com").await?);
    // And it must not match a domain that merely ends the same way.
    assert!(!matches(&t, "anyone@notexample.com").await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mail_from_a_blocked_address_lands_in_trash() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "spam@example.com", "").await?;
    let msg_id = recv(&t, "spam@example.com", "blocked@example.com").await?;
    assert!(
        in_trash(&t, msg_id).await?,
        "a blocked sender's mail stayed in the mailbox"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_domain_pattern_blocks_the_whole_domain() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "@spam.example", "").await?;
    let one = recv(&t, "anyone@spam.example", "one@spam.example").await?;
    let two = recv(&t, "someone.else@spam.example", "two@spam.example").await?;
    assert!(in_trash(&t, one).await?);
    assert!(in_trash(&t, two).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_unblocked_sender_reaches_the_mailbox() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "spam@example.com", "").await?;
    let msg_id = recv(&t, "friend@example.org", "kept@example.org").await?;
    assert!(
        !in_trash(&t, msg_id).await?,
        "mail from someone who is not on the list was trashed"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unblocking_lets_the_next_message_through() -> Result<()> {
    let t = TestContext::new_alice().await;
    add(&t, "spam@example.com", "").await?;
    let first = recv(&t, "spam@example.com", "first@example.com").await?;
    assert!(in_trash(&t, first).await?);

    remove(&t, "spam@example.com").await?;
    let second = recv(&t, "spam@example.com", "second@example.com").await?;
    assert!(
        !in_trash(&t, second).await?,
        "mail kept being trashed after the pattern was removed"
    );
    // And the message already trashed stays trashed: unblocking is about what
    // arrives next, not a request to undo a decision already taken.
    assert!(in_trash(&t, first).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blocking_never_touches_mail_already_received() -> Result<()> {
    let t = TestContext::new_alice().await;
    let early = recv(&t, "spam@example.com", "early@example.com").await?;
    add(&t, "spam@example.com", "").await?;
    assert!(
        !in_trash(&t, early).await?,
        "blocking retroactively binned mail the user had already been shown"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_blocking_a_contact_does_both_halves() -> Result<()> {
    let t = TestContext::new_alice().await;
    recv(&t, "spam@example.com", "early@example.com").await?;
    let contact_id =
        Contact::lookup_id_by_addr(&t, "spam@example.com", crate::contact::Origin::Unknown)
            .await?
            .expect("no contact for a sender we received mail from");

    block_contact(&t, contact_id).await?;

    // The contact row, which is what core's own filtering keys off...
    assert!(Contact::is_blocked_load(&t, contact_id).await?);
    // ...and the blocklist, which is the only half that stops mail. A caller
    // that did one of these would have a feature that looks like it works.
    assert!(matches(&t, "spam@example.com").await?);
    let after = recv(&t, "spam@example.com", "after@example.com").await?;
    assert!(in_trash(&t, after).await?);

    unblock_contact(&t, contact_id).await?;
    assert!(!Contact::is_blocked_load(&t, contact_id).await?);
    assert!(!matches(&t, "spam@example.com").await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_the_query_and_the_predicate_agree() -> Result<()> {
    // `matches` is SQL and `pattern_covers` is Rust, and they express one rule.
    // Two implementations of one rule is how two answers happen, so this pins
    // them together over the cases that distinguish them.
    let t = TestContext::new_alice().await;
    let patterns = ["spam@example.com", "@blocked.example"];
    for pattern in patterns {
        add(&t, pattern, "").await?;
    }
    let addrs = [
        "spam@example.com",
        "SPAM@EXAMPLE.COM",
        "other@example.com",
        "anyone@blocked.example",
        "anyone@mail.blocked.example",
        "anyone@notblocked.example",
        "notanaddress",
    ];
    for addr in addrs {
        let by_query = matches(&t, addr).await?;
        let by_predicate = patterns
            .iter()
            .any(|p| pattern_covers(&normalize(p), &normalize(addr)));
        assert_eq!(by_query, by_predicate, "the two disagree about {addr:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_removing_a_pattern_unblocks_the_contact_it_blocked() -> Result<()> {
    let t = TestContext::new_alice().await;
    recv(&t, "spam@example.com", "early@example.com").await?;
    let contact_id =
        Contact::lookup_id_by_addr(&t, "spam@example.com", crate::contact::Origin::Unknown)
            .await?
            .expect("no contact for a sender we received mail from");
    block_contact(&t, contact_id).await?;
    assert!(Contact::is_blocked_load(&t, contact_id).await?);

    // Removing the pattern from the blocklist has to undo the contact half as
    // well. Otherwise the contact stays `blocked=1` -- hidden from
    // `get_contacts`, filtered out of search -- while their mail arrives again.
    remove(&t, "spam@example.com").await?;
    assert!(
        !Contact::is_blocked_load(&t, contact_id).await?,
        "the contact stayed blocked after its pattern was removed"
    );
    let after = recv(&t, "spam@example.com", "after@example.com").await?;
    assert!(!in_trash(&t, after).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_removing_a_domain_pattern_unblocks_contacts_in_that_domain() -> Result<()> {
    let t = TestContext::new_alice().await;
    recv(&t, "someone@spam.example", "early@spam.example").await?;
    let contact_id =
        Contact::lookup_id_by_addr(&t, "someone@spam.example", crate::contact::Origin::Unknown)
            .await?
            .expect("no contact for a sender we received mail from");
    block_contact(&t, contact_id).await?;
    // Blocked by their own address; now blanket-block the domain and drop it.
    add(&t, "@spam.example", "").await?;
    remove(&t, "@spam.example").await?;
    assert!(
        !Contact::is_blocked_load(&t, contact_id).await?,
        "a contact in the removed domain stayed blocked"
    );
    Ok(())
}
