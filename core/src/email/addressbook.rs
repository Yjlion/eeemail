//! The address book: what eeemail knows about a person beyond their key.
//!
//! Core's [`Contact`] is a key holder. It has a name, an address, an origin and
//! a fingerprint, because those are what deciding whether to encrypt to
//! somebody requires. An address book needs more — where they work, what to
//! call them, a phone number, which of your circles they belong to — and none
//! of it has anything to do with encryption.
//!
//! So it lives in side tables keyed by `contacts.id`, the same arrangement
//! `contact_policy` already uses for per-contact encryption and receipt
//! overrides, rather than as columns bolted onto an upstream table. Merging is
//! easier and the separation is honest: nothing here is ever consulted when
//! deciding how to send a message.
//!
//! # Why searching does not go through `Contact::get_all`
//!
//! [`search`] runs its own query. Upstream's `Contact::get_all` cannot answer
//! "show me everyone", for three separate reasons:
//!
//! * it hardcodes `AND c.blocked=0`, so a blocked contact cannot be found even
//!   to be unblocked;
//! * it splits on `AND (fingerprint='')=?`, so **one call returns key-contacts
//!   or address-contacts and never both** — and the same correspondent is
//!   routinely one of each ([ADR 0021]);
//! * it hides anyone below `Origin::IncomingReplyTo`, so somebody you have only
//!   ever been Cc'd alongside is invisible.
//!
//! Each of those is right for the picker it was written for and wrong for an
//! address book, which is a list of everyone the mailbox has ever seen.
//!
//! # Categories are not labels
//!
//! They are a separate vocabulary with their own table, even though
//! [`super::labels`] is nearly the same shape. A label answers "where is this
//! message"; a category answers "who is this person". Sharing one table would
//! put every category in the sidebar as a mail view and every tag in the
//! contact picker, and the two lists would grow into each other's way.
//!
//! Categories are **local and not synced**. Labels are synced because they ride
//! an existing per-message wire format; there is no such format for a contact
//! grouping, and inventing one is a larger decision than this module.
//!
//! See [ADR 0028](../../../docs/adr/0028-contacts-are-an-address-book.md).
//!
//! [ADR 0021]: ../../../docs/adr/0021-autocrypt-key-contacts.md

use anyhow::{Context as _, Result, ensure};
use rusqlite::types::Value;

use crate::contact::ContactId;
use crate::context::Context;

/// Contact ids at or below this are core's own reserved rows.
///
/// `ContactId::SELF` is 1 and the range to 9 is reserved; upstream's own
/// queries use `>9` for the same purpose. An address book that listed "you"
/// and seven placeholders would be wrong in a way the user cannot fix.
const FIRST_REAL_CONTACT: u32 = 9;

/// What the address book knows about somebody, beyond their contact row.
///
/// Every field is optional in practice and stored as the empty string when
/// unset, so a contact with no details and a contact with a row of empty
/// details are the same thing to a reader.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Details {
    /// Employer, club, or whatever they answer "who are you with" with.
    pub organisation: String,
    /// Their role, which is only meaningful beside an organisation.
    pub job_title: String,
    /// A postal address, as one free-text block. Never parsed.
    pub postal: String,
    /// A URL, stored verbatim and never fetched by anything here.
    pub website: String,
    /// Free text. Searched, never interpreted.
    pub notes: String,
    /// Numbers, in the order the user put them.
    pub phones: Vec<Phone>,
}

/// One phone number and what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phone {
    /// "work", "mobile" — free text, because the useful set is per person.
    pub label: String,
    /// Stored exactly as typed. Formatting a number is how you break it.
    pub number: String,
}

/// A user-defined grouping of contacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    /// Local row id. Categories are not synced, so this is the only name one has.
    pub id: i64,
    /// As the user typed it.
    pub name: String,
    /// `0xRRGGBB`, or `None` if the user picked no colour.
    pub color: Option<u32>,
}

/// The normalised form a category name is deduplicated by.
fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Reads a contact's details. A contact with no row has empty details.
pub async fn details(context: &Context, contact_id: ContactId) -> Result<Details> {
    let row: Option<(String, String, String, String, String)> = context
        .sql
        .query_row_optional(
            "SELECT organisation, job_title, postal, website, notes
             FROM contact_details WHERE contact_id=?",
            (contact_id,),
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .await?;

    let phones = context
        .sql
        .query_map_vec(
            "SELECT label, number FROM contact_phones WHERE contact_id=? ORDER BY seq",
            (contact_id,),
            |row| {
                Ok(Phone {
                    label: row.get(0)?,
                    number: row.get(1)?,
                })
            },
        )
        .await?;

    let Some((organisation, job_title, postal, website, notes)) = row else {
        return Ok(Details {
            phones,
            ..Default::default()
        });
    };
    Ok(Details {
        organisation,
        job_title,
        postal,
        website,
        notes,
        phones,
    })
}

/// Writes a contact's details, replacing whatever was there.
///
/// Whole-record rather than field-at-a-time: the caller is an edit form that
/// holds every field, and a partial update API would make "clear this field"
/// indistinguishable from "leave it alone".
pub async fn set_details(
    context: &Context,
    contact_id: ContactId,
    details: &Details,
) -> Result<()> {
    let d = details.clone();
    context
        .sql
        .transaction(move |transaction| {
            transaction.execute(
                "INSERT INTO contact_details
                     (contact_id, organisation, job_title, postal, website, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(contact_id) DO UPDATE SET
                     organisation=excluded.organisation,
                     job_title=excluded.job_title,
                     postal=excluded.postal,
                     website=excluded.website,
                     notes=excluded.notes",
                (
                    contact_id,
                    d.organisation.trim(),
                    d.job_title.trim(),
                    d.postal.trim(),
                    d.website.trim(),
                    d.notes.trim(),
                ),
            )?;
            // Replaced wholesale: reconciling a list by position would make
            // deleting the first number look like renaming all of them.
            transaction.execute(
                "DELETE FROM contact_phones WHERE contact_id=?",
                (contact_id,),
            )?;
            for (seq, phone) in d
                .phones
                .iter()
                .filter(|p| !p.number.trim().is_empty())
                .enumerate()
            {
                transaction.execute(
                    "INSERT INTO contact_phones (contact_id, seq, label, number)
                     VALUES (?1, ?2, ?3, ?4)",
                    (
                        contact_id,
                        i64::try_from(seq).unwrap_or(i64::MAX),
                        phone.label.trim(),
                        phone.number.trim(),
                    ),
                )?;
            }
            Ok(())
        })
        .await?;
    Ok(())
}

/// Every category, by name.
pub async fn categories(context: &Context) -> Result<Vec<Category>> {
    context
        .sql
        .query_map_vec(
            "SELECT id, name, color FROM contact_categories ORDER BY name_norm",
            (),
            |row| {
                let color: Option<i64> = row.get(2)?;
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: color.and_then(|c| u32::try_from(c).ok()),
                })
            },
        )
        .await
}

/// Creates a category, or returns the existing one with that name.
pub async fn create_category(
    context: &Context,
    name: &str,
    color: Option<u32>,
) -> Result<Category> {
    let trimmed = name.trim().to_string();
    ensure!(!trimmed.is_empty(), "a category needs a name");
    let name_norm = normalize(&trimmed);
    context
        .sql
        .execute(
            "INSERT INTO contact_categories (name, name_norm, color) VALUES (?1, ?2, ?3)
             ON CONFLICT(name_norm) DO NOTHING",
            (&trimmed, &name_norm, color.map(i64::from)),
        )
        .await?;
    category_by_norm(context, &name_norm)
        .await?
        .context("category vanished immediately after being created")
}

async fn category_by_norm(context: &Context, name_norm: &str) -> Result<Option<Category>> {
    context
        .sql
        .query_row_optional(
            "SELECT id, name, color FROM contact_categories WHERE name_norm=?",
            (name_norm,),
            |row| {
                let color: Option<i64> = row.get(2)?;
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: color.and_then(|c| u32::try_from(c).ok()),
                })
            },
        )
        .await
}

/// Renames a category.
pub async fn rename_category(context: &Context, id: i64, new_name: &str) -> Result<()> {
    let trimmed = new_name.trim().to_string();
    ensure!(!trimmed.is_empty(), "a category needs a name");
    let name_norm = normalize(&trimmed);
    if let Some(existing) = category_by_norm(context, &name_norm).await?
        && existing.id != id
    {
        anyhow::bail!("a category called {trimmed:?} already exists");
    }
    context
        .sql
        .execute(
            "UPDATE contact_categories SET name=?1, name_norm=?2 WHERE id=?3",
            (&trimmed, &name_norm, id),
        )
        .await?;
    Ok(())
}

/// Sets or clears a category's colour.
pub async fn set_category_color(context: &Context, id: i64, color: Option<u32>) -> Result<()> {
    context
        .sql
        .execute(
            "UPDATE contact_categories SET color=?1 WHERE id=?2",
            (color.map(i64::from), id),
        )
        .await?;
    Ok(())
}

/// Deletes a category. The contacts in it are untouched.
pub async fn delete_category(context: &Context, id: i64) -> Result<()> {
    context
        .sql
        .transaction(move |transaction| {
            transaction.execute(
                "DELETE FROM contact_category_members WHERE category_id=?",
                (id,),
            )?;
            transaction.execute("DELETE FROM contact_categories WHERE id=?", (id,))?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// Puts a contact in a category. Already being in it is not an error.
pub async fn assign(context: &Context, contact_id: ContactId, category_id: i64) -> Result<()> {
    context
        .sql
        .execute(
            "INSERT INTO contact_category_members (contact_id, category_id) VALUES (?1, ?2)
             ON CONFLICT(contact_id, category_id) DO NOTHING",
            (contact_id, category_id),
        )
        .await?;
    Ok(())
}

/// Takes a contact out of a category. Not being in it is not an error.
pub async fn unassign(context: &Context, contact_id: ContactId, category_id: i64) -> Result<()> {
    context
        .sql
        .execute(
            "DELETE FROM contact_category_members WHERE contact_id=?1 AND category_id=?2",
            (contact_id, category_id),
        )
        .await?;
    Ok(())
}

/// The categories a contact is in.
pub async fn categories_of(context: &Context, contact_id: ContactId) -> Result<Vec<Category>> {
    context
        .sql
        .query_map_vec(
            "SELECT c.id, c.name, c.color
             FROM contact_categories c
             JOIN contact_category_members m ON m.category_id=c.id
             WHERE m.contact_id=?
             ORDER BY c.name_norm",
            (contact_id,),
            |row| {
                let color: Option<i64> = row.get(2)?;
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: color.and_then(|c| u32::try_from(c).ok()),
                })
            },
        )
        .await
}

/// Everyone the mailbox knows, filtered by a substring and by category.
///
/// An empty `query` returns everybody. See the module docs for why this does
/// not go through `Contact::get_all`.
///
/// `include_blocked` exists because a blocked contact is invisible to
/// `get_contacts` — so without it there would be no way to look one up, which
/// is a problem the moment somebody wants to undo a block.
pub async fn search(
    context: &Context,
    query: &str,
    category: Option<i64>,
    include_blocked: bool,
) -> Result<Vec<ContactId>> {
    let mut sql = format!(
        "SELECT DISTINCT c.id FROM contacts c
         LEFT JOIN contact_details d ON d.contact_id=c.id
         WHERE c.id>{FIRST_REAL_CONTACT}"
    );
    let mut params: Vec<Value> = Vec::new();

    if !include_blocked {
        sql.push_str(" AND IFNULL(c.blocked, 0)=0");
    }

    let needle = query.trim().to_lowercase();
    if !needle.is_empty() {
        let like = format!("%{needle}%");
        // A phone is matched with EXISTS rather than a join so a contact with
        // three matching numbers comes back once. Same reason `email::search`
        // does it for recipients.
        sql.push_str(
            " AND (LOWER(c.addr) LIKE ?
                   OR LOWER(c.name) LIKE ?
                   OR LOWER(c.authname) LIKE ?
                   OR LOWER(IFNULL(d.organisation, '')) LIKE ?
                   OR LOWER(IFNULL(d.job_title, '')) LIKE ?
                   OR LOWER(IFNULL(d.notes, '')) LIKE ?
                   OR EXISTS (SELECT 1 FROM contact_phones p
                              WHERE p.contact_id=c.id AND LOWER(p.number) LIKE ?))",
        );
        for _ in 0..7 {
            params.push(Value::Text(like.clone()));
        }
    }

    if let Some(category_id) = category {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM contact_category_members m
                          WHERE m.contact_id=c.id AND m.category_id=?)",
        );
        params.push(Value::Integer(category_id));
    }

    // Name first so the list reads alphabetically, address as the tiebreak for
    // the many contacts that have no name at all.
    sql.push_str(" ORDER BY LOWER(IFNULL(NULLIF(c.name, ''), c.authname)), LOWER(c.addr)");

    context
        .sql
        .query_map_vec(&sql, rusqlite::params_from_iter(params), |row| {
            Ok(row.get::<_, ContactId>(0)?)
        })
        .await
}

#[cfg(test)]
mod addressbook_tests;
