//! Tests for read receipts and ephemeral defaults.

use anyhow::Result;

use super::*;
use crate::test_utils::{TestContext, TestContextManager};

#[test]
fn test_unknown_policy_falls_back_to_the_default() {
    assert_eq!(MdnPolicy::from_i64(99), MdnPolicy::Always);
    assert_eq!(MdnPolicy::from_i64(-1), MdnPolicy::Always);
}

#[test]
fn test_timer_from_secs_never_invents_a_short_timer() {
    assert_eq!(timer_from_secs(0), Timer::Disabled);
    // A negative or out-of-range value must mean "no timer", never an
    // arbitrarily short one that deletes mail.
    assert_eq!(timer_from_secs(-1), Timer::Disabled);
    assert_eq!(timer_from_secs(i64::MIN), Timer::Disabled);
    assert_eq!(timer_from_secs(i64::MAX), Timer::Disabled);
    assert_eq!(timer_from_secs(60).to_u32(), 60);
}

#[test]
fn test_shorter_treats_disabled_as_longest() {
    let hour = timer_from_secs(3600);
    let day = timer_from_secs(86_400);
    assert_eq!(shorter(Timer::Disabled, hour), hour);
    assert_eq!(shorter(hour, Timer::Disabled), hour);
    assert_eq!(shorter(day, hour), hour);
    assert_eq!(shorter(Timer::Disabled, Timer::Disabled), Timer::Disabled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_read_receipts_are_on_by_default() -> Result<()> {
    let t = TestContext::new_alice().await;
    assert_eq!(MdnPolicy::load(&t).await?, MdnPolicy::Always);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_ephemeral_default_ships_disabled() -> Result<()> {
    let t = TestContext::new_alice().await;
    // Deliberate: ephemeral deletion removes the local copy, which is the only
    // durable one. See ADR 0011.
    assert_eq!(default_timer(&t).await?, Timer::Disabled);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_global_off_is_a_hard_off() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    // Someone turns receipts off but the policy key still says "always", and
    // the contact has an override saying yes. Off must still win: a user who
    // turned read receipts off must not keep sending them to anyone.
    alice.set_config_bool(Config::MdnsEnabled, false).await?;
    alice.set_config(Config::MdnPolicy, Some("2")).await?;
    set_mdn_for_contact(&alice, bob_id, Some(true)).await?;

    assert_eq!(MdnPolicy::load(&alice).await?, MdnPolicy::Never);
    assert!(!should_send_mdn(&alice, bob_id).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_setting_the_policy_keeps_mdns_enabled_in_step() -> Result<()> {
    let t = TestContext::new_alice().await;

    MdnPolicy::set(&t, MdnPolicy::Never).await?;
    assert!(!t.get_config_bool(Config::MdnsEnabled).await?);

    MdnPolicy::set(&t, MdnPolicy::VerifiedOnly).await?;
    assert!(t.get_config_bool(Config::MdnsEnabled).await?);
    assert_eq!(MdnPolicy::load(&t).await?, MdnPolicy::VerifiedOnly);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_per_contact_override_beats_the_policy() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    assert!(should_send_mdn(&alice, bob_id).await?);

    set_mdn_for_contact(&alice, bob_id, Some(false)).await?;
    assert!(!should_send_mdn(&alice, bob_id).await?);

    set_mdn_for_contact(&alice, bob_id, None).await?;
    assert!(
        should_send_mdn(&alice, bob_id).await?,
        "clearing an override must fall back to the policy, not stay off"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verified_only_excludes_an_unverified_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    MdnPolicy::set(&alice, MdnPolicy::VerifiedOnly).await?;

    // Key learned opportunistically is not verification: it does not survive an
    // active attacker, which is the whole point of the distinction.
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    assert!(!should_send_mdn(&alice, bob_id).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verified_only_includes_a_securejoined_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    MdnPolicy::set(&alice, MdnPolicy::VerifiedOnly).await?;

    tcm.execute_securejoin(&alice, &bob).await;

    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    let contact = Contact::get_by_id(&alice, bob_id).await?;
    assert!(contact.is_verified(&alice).await?);
    assert!(contact.origin.is_known());
    assert!(should_send_mdn(&alice, bob_id).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verified_only_excludes_an_unknown_contact() -> Result<()> {
    let t = TestContext::new_alice().await;
    MdnPolicy::set(&t, MdnPolicy::VerifiedOnly).await?;
    // A contact id that does not resolve must not disclose anything.
    assert!(!should_send_mdn(&t, ContactId::new(9999)).await?);
    Ok(())
}

/// MDNs are queued in `smtp_mdns`, not the ordinary `smtp` table, so that is
/// where "a receipt was produced" is observable.
async fn queued_mdns(t: &TestContext, contact_id: ContactId) -> Result<bool> {
    t.sql
        .exists(
            "SELECT COUNT(*) FROM smtp_mdns WHERE from_id=?",
            (contact_id,),
        )
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_read_receipt_is_produced_for_an_ordinary_contact() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;

    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    let msg = alice.get_last_msg().await;
    crate::message::markseen_msgs(&alice, vec![msg.id]).await?;

    assert!(queued_mdns(&alice, bob_id).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_no_read_receipt_when_the_contact_is_opted_out() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;

    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    set_mdn_for_contact(&alice, bob_id, Some(false)).await?;

    let msg = alice.get_last_msg().await;
    crate::message::markseen_msgs(&alice, vec![msg.id]).await?;

    assert!(
        !queued_mdns(&alice, bob_id).await?,
        "an opted-out contact must get no receipt at all"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_verified_only_produces_no_receipt_for_an_unverified_sender() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    MdnPolicy::set(&alice, MdnPolicy::VerifiedOnly).await?;

    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    let msg = alice.get_last_msg().await;
    crate::message::markseen_msgs(&alice, vec![msg.id]).await?;

    assert!(!queued_mdns(&alice, bob_id).await?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Ephemeral
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_per_contact_timer_override() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;

    assert_eq!(timer_for_contact(&alice, bob_id).await?, None);
    set_timer_for_contact(&alice, bob_id, Some(timer_from_secs(3600))).await?;
    assert_eq!(
        timer_for_contact(&alice, bob_id).await?.map(|t| t.to_u32()),
        Some(3600)
    );
    set_timer_for_contact(&alice, bob_id, None).await?;
    assert_eq!(timer_for_contact(&alice, bob_id).await?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_effective_timer_takes_the_shortest() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let fiona = tcm.fiona().await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    let fiona_id = alice.add_or_lookup_contact_id(&fiona).await;

    set_default_timer(&alice, timer_from_secs(86_400)).await?;
    set_timer_for_contact(&alice, bob_id, Some(timer_from_secs(3600))).await?;

    assert_eq!(
        effective_default_timer(&alice, &[bob_id, fiona_id])
            .await?
            .to_u32(),
        3600,
        "an override is a statement about that correspondent, so it tightens the group"
    );

    // And an override longer than the default does not loosen it.
    set_timer_for_contact(&alice, fiona_id, Some(timer_from_secs(604_800))).await?;
    assert_eq!(
        effective_default_timer(&alice, &[bob_id, fiona_id])
            .await?
            .to_u32(),
        3600
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_default_timer_disabled_changes_nothing() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = crate::message::Message::new(crate::message::Viewtype::Text);
    msg.set_text("hello".to_string());
    alice.send_msg(chat.id, &mut msg).await;

    assert_eq!(
        chat.id.get_ephemeral_timer(&alice).await?,
        Timer::Disabled,
        "the shipped default must not silently start deleting mail"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_default_timer_applies_to_a_new_conversation() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    set_default_timer(&alice, timer_from_secs(3600)).await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = crate::message::Message::new(crate::message::Viewtype::Text);
    msg.set_text("hello".to_string());
    let sent = alice.send_msg(chat.id, &mut msg).await;

    assert_eq!(chat.id.get_ephemeral_timer(&alice).await?.to_u32(), 3600);
    assert!(
        sent.payload().contains("Ephemeral-Timer: 3600"),
        "the message itself must carry the timer, which is what tells the other side"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_turning_the_timer_off_sticks() -> Result<()> {
    let alice = TestContext::new_alice().await;
    alice.allow_unencrypted().await?;
    set_default_timer(&alice, timer_from_secs(3600)).await?;
    let chat = alice
        .create_chat_with_contact("Bob", "bob@example.net")
        .await;

    let mut msg = crate::message::Message::new(crate::message::Viewtype::Text);
    msg.set_text("first".to_string());
    alice.send_msg(chat.id, &mut msg).await;
    assert_eq!(chat.id.get_ephemeral_timer(&alice).await?.to_u32(), 3600);

    // The user decides they want this conversation kept.
    chat.id
        .inner_set_ephemeral_timer(&alice, Timer::Disabled)
        .await?;

    let mut msg = crate::message::Message::new(crate::message::Viewtype::Text);
    msg.set_text("second".to_string());
    alice.send_msg(chat.id, &mut msg).await;

    assert_eq!(
        chat.id.get_ephemeral_timer(&alice).await?,
        Timer::Disabled,
        "the default must apply once, not fight the user on every send"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_prune_drops_overrides_for_deleted_contacts() -> Result<()> {
    let mut tcm = TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    let bob_id = alice.add_or_lookup_contact_id(&bob).await;
    set_mdn_for_contact(&alice, bob_id, Some(false)).await?;

    alice
        .sql
        .execute("DELETE FROM contacts WHERE id=?", (bob_id,))
        .await?;
    prune(&alice).await?;

    assert!(
        !alice
            .sql
            .exists("SELECT COUNT(*) FROM contact_policy", ())
            .await?
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_settings_appear_in_get_info() -> Result<()> {
    let t = TestContext::new_alice().await;
    let info = t.get_info().await?;
    assert!(info.contains_key("mdn_policy"));
    assert!(info.contains_key("ephemeral_default_seconds"));
    Ok(())
}
