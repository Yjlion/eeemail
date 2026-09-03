//! Tests for search across body, subject, recipients and labels.

use anyhow::Result;

use super::*;
use crate::config::Config;
use crate::email::labels;
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;

fn mail(mid: &str, subject: &str, to: &str, body: &str) -> Vec<u8> {
    format!(
        "From: alice@example.org\r\n\
         To: {to}\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
         \r\n\
         {body}\r\n"
    )
    .into_bytes()
}

/// Like [`mail`], but from somebody else.
///
/// [`mail`] hardcodes `From: alice@example.org`, which is the test context
/// itself -- so those messages are self-sent and outgoing. Anything about the
/// inbox or about who sent a message needs a real correspondent.
fn mail_from(from: &str, mid: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\n\
         To: alice@example.org\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, 31 Aug 2026 12:00:00 +0000\r\n\
         \r\n\
         {body}\r\n"
    )
    .into_bytes()
}

async fn recv_from(t: &TestContext, from: &str, mid: &str, subject: &str) -> Result<MsgId> {
    let received = receive_imf(t, &mail_from(from, mid, subject, "body"), false)
        .await?
        .unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

async fn recv(t: &TestContext, mid: &str, subject: &str, to: &str, body: &str) -> Result<MsgId> {
    let received = receive_imf(t, &mail(mid, subject, to, body), false)
        .await?
        .unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_matches_the_body_like_upstream() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "unrelated", "bob@example.net", "the pangolin").await?;
    recv(&t, "b@x", "unrelated", "bob@example.net", "something else").await?;

    assert_eq!(search(&t, &SearchQuery::text("pangolin")).await?, vec![a]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_matches_the_subject() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "quarterly numbers", "bob@example.net", "body").await?;
    recv(&t, "b@x", "lunch", "bob@example.net", "body").await?;

    assert_eq!(search(&t, &SearchQuery::text("quarterly")).await?, vec![a]);

    // Upstream's search_msgs finds this too, but not because it searches
    // subjects: with `SubjectInBody` on -- upstream's default, still in force
    // here -- `mimeparser` prepends the subject into the body text of classic
    // mail, so `msgs.txt` literally reads "quarterly numbers - body".
    assert_eq!(t.search_msgs(None, "quarterly").await?, vec![a]);

    // Turn that off, as `email::policy::apply_defaults` does for every eeemail
    // account, and the difference becomes visible: ours still finds the message
    // because it searches the subject column, upstream's no longer finds it at
    // all. This is the test that would have caught the body corruption in the
    // first place -- it was written asserting the opposite. See issue #8.
    t.set_config_bool(Config::SubjectInBody, false).await?;
    let b = recv(&t, "c@x", "annual figures", "bob@example.net", "body").await?;
    assert_eq!(search(&t, &SearchQuery::text("annual")).await?, vec![b]);
    assert!(
        t.search_msgs(None, "annual").await?.is_empty(),
        "the subject leaked into the body"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_matches_a_recipient_address_or_name() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(
        &t,
        "a@x",
        "subject",
        "Carol Danvers <carol@example.com>",
        "body",
    )
    .await?;
    recv(&t, "b@x", "subject", "bob@example.net", "body").await?;

    assert_eq!(
        search(&t, &SearchQuery::text("carol@example")).await?,
        vec![a]
    );
    assert_eq!(search(&t, &SearchQuery::text("danvers")).await?, vec![a]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_message_with_several_matching_recipients_is_returned_once() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(
        &t,
        "a@x",
        "subject",
        "one@example.com, two@example.com, three@example.com",
        "body",
    )
    .await?;

    assert_eq!(
        search(&t, &SearchQuery::text("example.com")).await?,
        vec![a],
        "the recipient match must not multiply the row"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_search_is_case_insensitive() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(
        &t,
        "a@x",
        "Quarterly Numbers",
        "Bob <BOB@example.net>",
        "body",
    )
    .await?;

    assert_eq!(search(&t, &SearchQuery::text("QUARTERLY")).await?, vec![a]);
    assert_eq!(search(&t, &SearchQuery::text("bob")).await?, vec![a]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_empty_query_returns_nothing() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    recv(&t, "a@x", "subject", "bob@example.net", "body").await?;

    // An empty search box must not dump the mailbox.
    assert!(search(&t, &SearchQuery::text("")).await?.is_empty());
    assert!(search(&t, &SearchQuery::text("   ")).await?.is_empty());
    assert!(search(&t, &SearchQuery::default()).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_filter_by_label() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "report", "bob@example.net", "body").await?;
    let b = recv(&t, "b@x", "report", "bob@example.net", "body").await?;
    let work = labels::create(&t, "Work", None).await?;
    labels::apply(&t, &[a], work.id).await?;

    assert_eq!(
        search(&t, &SearchQuery::text("report").with_label(work.id)).await?,
        vec![a]
    );
    // A label on its own is a valid query: this is how a label view is built.
    assert_eq!(
        search(
            &t,
            &SearchQuery {
                label: Some(work.id),
                ..Default::default()
            }
        )
        .await?,
        vec![a]
    );
    let mut both = search(&t, &SearchQuery::text("report")).await?;
    both.sort();
    assert_eq!(both, {
        let mut v = vec![a, b];
        v.sort();
        v
    });
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_filter_by_system_tag() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    // Gating off: this test is about archive and the inbox, not about who sent.
    crate::email::gating::set_enabled(&t, false).await?;
    let a = recv_from(&t, "bob@example.net", "a@x", "report").await?;
    let b = recv_from(&t, "bob@example.net", "b@x", "report").await?;
    labels::archive(&t, &[a]).await?;

    assert_eq!(
        search(
            &t,
            &SearchQuery::text("report").with_tag(SystemTag::Archive)
        )
        .await?,
        vec![a]
    );
    assert_eq!(
        search(&t, &SearchQuery::text("report").with_tag(SystemTag::Inbox)).await?,
        vec![b],
        "the inbox is what carries no system tag, with no row of its own"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_held_mail_is_not_in_the_inbox() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    // Gating on: a stranger's mail is searchable but is not in the inbox. This
    // is the case the old `archived: Option<bool>` flag could not express.
    crate::email::gating::set_enabled(&t, true).await?;
    let held = recv_from(&t, "stranger@example.net", "h@x", "report").await?;

    assert_eq!(
        search(
            &t,
            &SearchQuery::text("report").with_tag(SystemTag::Unverified)
        )
        .await?,
        vec![held]
    );
    assert!(
        search(&t, &SearchQuery::text("report").with_tag(SystemTag::Inbox))
            .await?
            .is_empty()
    );
    assert_eq!(search(&t, &SearchQuery::text("report")).await?, vec![held]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_results_are_newest_first() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "report", "bob@example.net", "body").await?;
    let b = recv(&t, "b@x", "report", "bob@example.net", "body").await?;
    let c = recv(&t, "c@x", "report", "bob@example.net", "body").await?;

    assert_eq!(
        search(&t, &SearchQuery::text("report")).await?,
        vec![c, b, a]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_hidden_and_trashed_messages_are_excluded() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "report", "bob@example.net", "body").await?;
    let b = recv(&t, "b@x", "report", "bob@example.net", "body").await?;
    crate::message::delete_msgs(&t, &[b]).await?;

    assert_eq!(search(&t, &SearchQuery::text("report")).await?, vec![a]);
    Ok(())
}
