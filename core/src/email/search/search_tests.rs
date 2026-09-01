//! Tests for search across body, subject, recipients and labels.

use anyhow::Result;

use super::*;
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

    // Upstream's search_msgs happens to find this too, but not because it
    // searches subjects: `mimeparser` prepends the subject into the body text
    // of classic mail, so `msgs.txt` literally reads "quarterly numbers - body".
    // That is a chat-app accommodation we should not rely on -- it does not
    // apply to mail from another Delta Chat or eeemail client, where the
    // subject stays a subject.
    assert_eq!(t.search_msgs(None, "quarterly").await?, vec![a]);
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
async fn test_filter_by_archived() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, "a@x", "report", "bob@example.net", "body").await?;
    let b = recv(&t, "b@x", "report", "bob@example.net", "body").await?;
    labels::archive(&t, &[a]).await?;

    assert_eq!(
        search(&t, &SearchQuery::text("report").with_archived(true)).await?,
        vec![a]
    );
    assert_eq!(
        search(&t, &SearchQuery::text("report").with_archived(false)).await?,
        vec![b],
        "the inbox is what has not been archived, with no row of its own"
    );
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
