//! Tests for the address book.

use anyhow::Result;

use super::*;
use crate::contact::{Contact, Origin};
use crate::receive_imf::receive_imf;
use crate::test_utils::TestContext;
use deltachat_contact_tools::ContactAddress;

/// Creates an address-contact, which is what writing to somebody produces.
async fn address_contact(t: &TestContext, name: &str, addr: &str) -> Result<ContactId> {
    let addr = ContactAddress::new(addr)?;
    Ok(Contact::add_or_lookup(t, name, &addr, Origin::OutgoingTo)
        .await?
        .0)
}

/// A minimal message from `addr`, so reception makes a contact the natural way.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_details_round_trip() -> Result<()> {
    let t = TestContext::new_alice().await;
    let id = address_contact(&t, "Ada", "ada@example.com").await?;

    // A contact nobody has filled in has empty details rather than no details,
    // so a reader never has to distinguish the two.
    assert_eq!(details(&t, id).await?, Details::default());

    let written = Details {
        organisation: "Analytical Engines".to_string(),
        job_title: "Programmer".to_string(),
        postal: "12 Marylebone\nLondon".to_string(),
        website: "https://example.com".to_string(),
        notes: "met at the exhibition".to_string(),
        phones: vec![
            Phone {
                label: "work".to_string(),
                number: "+44 20 7946 0000".to_string(),
            },
            Phone {
                label: "mobile".to_string(),
                number: "+44 7700 900000".to_string(),
            },
        ],
    };
    set_details(&t, id, &written).await?;
    assert_eq!(details(&t, id).await?, written);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rewriting_details_replaces_the_phone_list() -> Result<()> {
    let t = TestContext::new_alice().await;
    let id = address_contact(&t, "Ada", "ada@example.com").await?;
    set_details(
        &t,
        id,
        &Details {
            phones: vec![
                Phone {
                    label: "work".to_string(),
                    number: "111".to_string(),
                },
                Phone {
                    label: "home".to_string(),
                    number: "222".to_string(),
                },
            ],
            ..Default::default()
        },
    )
    .await?;

    // Deleting the first number must not leave it behind or renumber the rest
    // into each other's places, which a positional reconcile would do.
    set_details(
        &t,
        id,
        &Details {
            phones: vec![Phone {
                label: "home".to_string(),
                number: "222".to_string(),
            }],
            ..Default::default()
        },
    )
    .await?;
    let after = details(&t, id).await?;
    assert_eq!(after.phones.len(), 1);
    assert_eq!(after.phones[0].number, "222");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_empty_number_is_not_stored() -> Result<()> {
    let t = TestContext::new_alice().await;
    let id = address_contact(&t, "Ada", "ada@example.com").await?;
    set_details(
        &t,
        id,
        &Details {
            phones: vec![
                Phone {
                    label: "work".to_string(),
                    number: "   ".to_string(),
                },
                Phone {
                    label: "mobile".to_string(),
                    number: "222".to_string(),
                },
            ],
            ..Default::default()
        },
    )
    .await?;
    // An edit form with a spare blank row is the normal case, not an error.
    let after = details(&t, id).await?;
    assert_eq!(after.phones.len(), 1);
    assert_eq!(after.phones[0].number, "222");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_search_finds_a_contact_by_organisation_phone_and_note() -> Result<()> {
    let t = TestContext::new_alice().await;
    let ada = address_contact(&t, "Ada", "ada@example.com").await?;
    let other = address_contact(&t, "Someone", "someone@example.com").await?;
    set_details(
        &t,
        ada,
        &Details {
            organisation: "Analytical Engines".to_string(),
            notes: "met at the exhibition".to_string(),
            phones: vec![Phone {
                label: "work".to_string(),
                number: "+44 7700 900123".to_string(),
            }],
            ..Default::default()
        },
    )
    .await?;

    for needle in ["analytical", "900123", "exhibition"] {
        let found = search(&t, needle, None, false).await?;
        assert!(found.contains(&ada), "{needle:?} did not find the contact");
        assert!(
            !found.contains(&other),
            "{needle:?} matched a contact it should not have"
        );
    }

    // Case-insensitively, because nobody types their own notes back exactly.
    assert!(search(&t, "ANALYTICAL", None, false).await?.contains(&ada));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_search_returns_both_kinds_of_contact_row() -> Result<()> {
    let mut tcm = crate::test_utils::TestContextManager::new();
    let alice = tcm.alice().await;
    let bob = tcm.bob().await;
    // An encrypted message from bob mints a *key*-contact for him...
    tcm.send_recv_accept(&bob, &alice, "hi").await;
    let bob_addr = bob.get_config(crate::config::Config::Addr).await?.unwrap();
    // ...and writing to the same address gives an *address*-contact beside it.
    let _ = address_contact(&alice, "Bob", &bob_addr).await?;

    let found = search(&alice, &bob_addr, None, false).await?;
    // This is the thing `Contact::get_all` cannot do: its `(fingerprint='')=?`
    // split returns one kind or the other and never both, so an address book
    // built on it shows half the rows for the same person.
    assert!(
        found.len() >= 2,
        "expected both the key-contact and the address-contact, got {found:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_an_empty_query_returns_everybody_but_the_reserved_rows() -> Result<()> {
    let t = TestContext::new_alice().await;
    let ada = address_contact(&t, "Ada", "ada@example.com").await?;
    let found = search(&t, "", None, false).await?;
    assert!(found.contains(&ada));
    // `SELF` and the reserved range are not people, and an address book that
    // listed them would be wrong in a way the user cannot fix.
    assert!(!found.contains(&ContactId::SELF));
    assert!(found.iter().all(|id| id.to_u32() > FIRST_REAL_CONTACT));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_blocked_contact_is_findable_only_when_asked_for() -> Result<()> {
    let t = TestContext::new_alice().await;
    t.allow_unencrypted().await?;
    receive_imf(&t, &mail_from("spam@example.com", "one@example.com"), false).await?;
    let id = Contact::lookup_id_by_addr(&t, "spam@example.com", Origin::Unknown)
        .await?
        .expect("no contact for a sender we received mail from");
    Contact::block(&t, id).await?;

    assert!(!search(&t, "spam", None, false).await?.contains(&id));
    // Without this there would be no way to look a blocked contact up, which is
    // a problem the moment somebody wants to undo the block.
    assert!(search(&t, "spam", None, true).await?.contains(&id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_categories_round_trip_and_filter() -> Result<()> {
    let t = TestContext::new_alice().await;
    let ada = address_contact(&t, "Ada", "ada@example.com").await?;
    let other = address_contact(&t, "Someone", "someone@example.com").await?;

    let work = create_category(&t, "Work", Some(0x2563eb)).await?;
    assert_eq!(work.color, Some(0x2563eb));

    // Creating the same name twice returns the same category rather than a
    // duplicate the user then has to tell apart.
    let again = create_category(&t, "  work  ", None).await?;
    assert_eq!(again.id, work.id);
    assert_eq!(categories(&t).await?.len(), 1);

    assign(&t, ada, work.id).await?;
    assign(&t, ada, work.id).await?;
    assert_eq!(categories_of(&t, ada).await?, vec![work.clone()]);

    let found = search(&t, "", Some(work.id), false).await?;
    assert!(found.contains(&ada));
    assert!(!found.contains(&other));

    rename_category(&t, work.id, "Colleagues").await?;
    set_category_color(&t, work.id, None).await?;
    let renamed = &categories(&t).await?[0];
    assert_eq!(renamed.name, "Colleagues");
    assert_eq!(renamed.color, None);

    unassign(&t, ada, work.id).await?;
    assert!(categories_of(&t, ada).await?.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_deleting_a_category_keeps_the_contacts() -> Result<()> {
    let t = TestContext::new_alice().await;
    let ada = address_contact(&t, "Ada", "ada@example.com").await?;
    let work = create_category(&t, "Work", None).await?;
    assign(&t, ada, work.id).await?;

    delete_category(&t, work.id).await?;

    assert!(categories(&t).await?.is_empty());
    assert!(categories_of(&t, ada).await?.is_empty());
    // The category was a grouping, not a container. Deleting it must not take
    // the people in it with it.
    assert!(search(&t, "ada", None, false).await?.contains(&ada));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_a_category_cannot_be_renamed_onto_another() -> Result<()> {
    let t = TestContext::new_alice().await;
    let work = create_category(&t, "Work", None).await?;
    create_category(&t, "Family", None).await?;
    // Otherwise the unique index rejects it with a message about a constraint,
    // which is not something to show a person who typed a name.
    assert!(rename_category(&t, work.id, "family").await.is_err());
    // Renaming a category to its own name is not a collision with itself.
    assert!(rename_category(&t, work.id, "Work").await.is_ok());
    Ok(())
}
