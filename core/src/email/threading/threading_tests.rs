//! Tests for conversation threading.

use anyhow::Result;

use super::*;
use crate::message::{Message, Viewtype};
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;

/// Builds a classic mail with explicit threading headers.
fn mail(mid: &str, in_reply_to: &str, references: &str, subject: &str, day: u32) -> Vec<u8> {
    let mut raw = format!(
        "From: alice@example.org\r\n\
         To: bob@example.net\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{mid}>\r\n\
         Date: Mon, {day:02} Aug 2026 12:00:00 +0000\r\n"
    );
    if !in_reply_to.is_empty() {
        raw.push_str(&format!("In-Reply-To: <{in_reply_to}>\r\n"));
    }
    if !references.is_empty() {
        let refs: Vec<String> = references
            .split_whitespace()
            .map(|r| format!("<{r}>"))
            .collect();
        raw.push_str(&format!("References: {}\r\n", refs.join(" ")));
    }
    raw.push_str("\r\nbody\r\n");
    raw.into_bytes()
}

async fn recv(t: &TestContext, raw: &[u8]) -> Result<MsgId> {
    let received = receive_imf(t, raw, false).await?.unwrap();
    Ok(*received.msg_ids.last().unwrap())
}

#[test]
fn test_linked_ids_collects_own_id_and_every_reference() {
    let headers = ThreadHeaders {
        rfc724_mid: "c@x",
        in_reply_to: "<b@x>",
        references: "<a@x> <b@x>",
        subject: "Re: hello",
        timestamp: 0,
    };
    let ids = headers.linked_ids();
    assert_eq!(ids, vec!["c@x", "a@x", "b@x"]);
}

#[test]
fn test_linked_ids_deduplicates_and_skips_empties() {
    let headers = ThreadHeaders {
        rfc724_mid: "a@x",
        in_reply_to: "<a@x>",
        references: "<a@x> <a@x>",
        ..Default::default()
    };
    assert_eq!(headers.linked_ids(), vec!["a@x"]);

    let headers = ThreadHeaders {
        rfc724_mid: "",
        ..Default::default()
    };
    assert!(headers.linked_ids().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reply_joins_the_thread_of_its_parent() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let root = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let reply = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;

    let root_thread = thread_of(&t, root).await?.unwrap();
    assert_eq!(thread_of(&t, reply).await?, Some(root_thread));
    assert_eq!(messages(&t, root_thread).await?, vec![root, reply]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_unrelated_messages_stay_apart() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let one = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let two = recv(&t, &mail("b@x", "", "", "hello", 2)).await?;

    assert_ne!(
        thread_of(&t, one).await?,
        thread_of(&t, two).await?,
        "identical subjects must not merge threads: JWZ step 5 is deliberately not implemented"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reply_arriving_before_its_parent() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // The reply arrives first and references a message we do not have.
    let reply = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;
    let thread = thread_of(&t, reply).await?.unwrap();

    // The parent then arrives and must land in the thread its child made.
    let root = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    assert_eq!(thread_of(&t, root).await?, Some(thread));
    assert_eq!(messages(&t, thread).await?, vec![root, reply]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_missing_link_merges_two_thread_fragments() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // Two fragments of one conversation, neither referencing the other.
    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let d = recv(&t, &mail("d@x", "c@x", "c@x", "Re: hello", 4)).await?;
    assert_ne!(thread_of(&t, a).await?, thread_of(&t, d).await?);

    // The message that references both proves they were always one thread.
    let c = recv(&t, &mail("c@x", "a@x", "a@x b@x", "Re: hello", 3)).await?;

    let thread = thread_of(&t, c).await?.unwrap();
    assert_eq!(thread_of(&t, a).await?, Some(thread));
    assert_eq!(thread_of(&t, d).await?, Some(thread));
    assert_eq!(messages(&t, thread).await?, vec![a, c, d]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_merge_keeps_the_older_thread() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let older = thread_of(&t, a).await?.unwrap();
    let d = recv(&t, &mail("d@x", "c@x", "c@x", "Re: hello", 4)).await?;
    let newer = thread_of(&t, d).await?.unwrap();
    assert!(older < newer);

    recv(&t, &mail("c@x", "a@x", "a@x b@x", "Re: hello", 3)).await?;

    assert_eq!(
        thread_of(&t, d).await?,
        Some(older),
        "the long-running conversation must keep its identity"
    );
    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM threads WHERE id=?", (newer,))
            .await?,
        "the merged-away thread must be removed"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_assign_is_idempotent() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let thread = thread_of(&t, a).await?.unwrap();

    assert_eq!(assign_stored(&t, a).await?, Some(thread));
    assert_eq!(assign_stored(&t, a).await?, Some(thread));
    assert_eq!(messages(&t, thread).await?, vec![a], "no duplicate rows");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_assign_stored_on_a_missing_message() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert_eq!(assign_stored(&t, MsgId::new(987_654)).await?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_null_threading_columns() -> Result<()> {
    // Not every path that writes a `msgs` row fills these in -- messages
    // created by SecureJoin leave them NULL -- and neither column is declared
    // NOT NULL. Reading one as a `String` fails the whole assignment, which is
    // how this was found.
    //
    // Only these two are exercised. `subject` and `rfc724_mid` are guarded in
    // the query as well, but a NULL there is not a reachable state: core's own
    // `Message::load_from_db` reads them as `String` too and would fail first.
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    t.sql
        .execute(
            "UPDATE msgs SET mime_in_reply_to=NULL, mime_references=NULL WHERE id=?",
            (a,),
        )
        .await?;

    let thread = assign_stored(&t, a).await?.expect("must not fail on NULLs");
    assert_eq!(messages(&t, thread).await?, vec![a]);
    assert_eq!(tree(&t, thread).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_thread_is_labelled_with_the_normalized_subject() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // The reply arrives first, so the thread is first labelled "Re: hello".
    let reply = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;
    let thread = thread_of(&t, reply).await?.unwrap();
    let subject: String = t
        .sql
        .query_get_value("SELECT subject_norm FROM threads WHERE id=?", (thread,))
        .await?
        .unwrap();
    assert_eq!(subject, "hello", "Re: must be stripped from the label");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tree_shape() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    //   a
    //   |- b
    //   |  `- d
    //   `- c
    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let b = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;
    let c = recv(&t, &mail("c@x", "a@x", "a@x", "Re: hello", 3)).await?;
    let d = recv(&t, &mail("d@x", "b@x", "a@x b@x", "Re: hello", 4)).await?;

    let roots = tree(&t, thread_of(&t, a).await?.unwrap()).await?;
    assert_eq!(roots.len(), 1);
    let root = &roots[0];
    assert_eq!(root.msg_id, a);
    assert_eq!(
        root.children.iter().map(|n| n.msg_id).collect::<Vec<_>>(),
        vec![b, c]
    );
    assert_eq!(
        root.children[0]
            .children
            .iter()
            .map(|n| n.msg_id)
            .collect::<Vec<_>>(),
        vec![d]
    );
    assert!(root.children[1].children.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tree_skips_messages_we_never_received() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // b@x is referenced but never arrives, so d attaches to a instead of
    // vanishing under a placeholder.
    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let d = recv(&t, &mail("d@x", "b@x", "a@x b@x", "Re: hello", 4)).await?;

    let roots = tree(&t, thread_of(&t, a).await?.unwrap()).await?;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].msg_id, a);
    assert_eq!(
        roots[0]
            .children
            .iter()
            .map(|n| n.msg_id)
            .collect::<Vec<_>>(),
        vec![d],
        "the nearest ancestor we actually hold becomes the parent"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tree_has_several_roots_when_the_first_message_is_missing() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let b = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;
    let c = recv(&t, &mail("c@x", "a@x", "a@x", "Re: hello", 3)).await?;

    let roots = tree(&t, thread_of(&t, b).await?.unwrap()).await?;
    assert_eq!(
        roots.iter().map(|n| n.msg_id).collect::<Vec<_>>(),
        vec![b, c],
        "replies to a message we never got are roots, not orphans"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tree_survives_a_reference_cycle() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    // Two messages each claiming to reply to the other. A naive parent map
    // would loop forever; only the older-than rule prevents it.
    let a = recv(&t, &mail("a@x", "b@x", "b@x", "hello", 1)).await?;
    let b = recv(&t, &mail("b@x", "a@x", "a@x", "hello", 2)).await?;

    let roots = tree(&t, thread_of(&t, a).await?.unwrap()).await?;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].msg_id, a);
    assert_eq!(
        roots[0]
            .children
            .iter()
            .map(|n| n.msg_id)
            .collect::<Vec<_>>(),
        vec![b]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_outgoing_reply_threads_with_the_incoming_message() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;

    let incoming = recv(&alice, &mail("a@x", "", "", "hello", 1)).await?;
    let incoming_msg = Message::load_from_db(&alice, incoming).await?;

    let mut reply = Message::new(Viewtype::Text);
    reply.set_text("my reply".to_string());
    reply.set_quote(&alice, Some(&incoming_msg)).await?;
    let sent = alice.send_msg(incoming_msg.chat_id, &mut reply).await;

    assert_eq!(
        thread_of(&alice, sent.sender_msg_id).await?,
        thread_of(&alice, incoming).await?,
        "a reply we send must join the conversation we are replying to"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prune_drops_threads_of_deleted_messages() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let thread = thread_of(&t, a).await?.unwrap();

    crate::message::delete_msgs(&t, &[a]).await?;
    prune(&t).await?;

    assert_eq!(thread_of(&t, a).await?, None);
    assert!(
        !t.sql
            .exists("SELECT COUNT(*) FROM threads WHERE id=?", (thread,))
            .await?,
        "an empty thread must not survive"
    );
    assert!(
        !t.sql
            .exists(
                "SELECT COUNT(*) FROM thread_refs WHERE thread_id=?",
                (thread,)
            )
            .await?,
        "references to a dropped thread must go too, or a later reply would \
         resurrect an empty conversation"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prune_keeps_live_threads() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;

    let a = recv(&t, &mail("a@x", "", "", "hello", 1)).await?;
    let b = recv(&t, &mail("b@x", "a@x", "a@x", "Re: hello", 2)).await?;
    let thread = thread_of(&t, a).await?.unwrap();

    crate::message::delete_msgs(&t, &[b]).await?;
    prune(&t).await?;

    assert_eq!(
        thread_of(&t, a).await?,
        Some(thread),
        "deleting one message must not dissolve the thread"
    );
    assert_eq!(messages(&t, thread).await?, vec![a]);
    Ok(())
}

#[test]
fn test_deep_tree_does_not_overflow_the_stack() {
    // Reply depth is bounded only by how many messages a sender can get us to
    // store, so tree building must not recurse. Exercised directly on
    // `build_node`: routing 50k messages through `receive_imf` would take a
    // minute and prove nothing about this.
    let depth = 50_000u32;
    let mut children: HashMap<MsgId, Vec<MsgId>> = HashMap::new();
    for i in 1..depth {
        children.insert(MsgId::new(i), vec![MsgId::new(i + 1)]);
    }

    let root = build_node(MsgId::new(1), &children);

    let mut node = &root;
    let mut counted = 1;
    while let Some(child) = node.children.first() {
        node = child;
        counted += 1;
    }
    assert_eq!(counted, depth);
    // Dropping the result must survive the same depth: `ThreadNode`'s drop glue
    // recurses even though the builder does not.
    drop(root);
}
