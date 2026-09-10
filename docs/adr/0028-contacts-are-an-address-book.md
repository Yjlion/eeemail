# 0028 — Contacts are an address book, not only a key store

**Status:** Accepted — 2026-09-09

## Context

Core's `Contact` is a key holder. It carries a name, an address, an origin and
a fingerprint, because those are what deciding whether to encrypt to somebody
requires. That is the right shape for the thing it is, and it is not an address
book: there is nowhere to record where somebody works, what to call them, a
phone number, or which of your circles they belong to.

eeemail's contacts screen inherited that shape and then made it worse. It
showed two `filter()`ed lists — verified and everyone else — under a QR code
that took the top of the screen, with no search, no detail, and no way to group
anybody. For a messenger with a dozen correspondents that is adequate. For a
mail client, whose contact table accumulates a row for every person who has
ever been in a `To:` or `Cc:` header, it is a list you cannot navigate.

The listing call is the sharper half of the problem. `Contact::get_all` cannot
answer "show me everyone", for three independent reasons:

- it hardcodes `AND c.blocked=0`, so a blocked contact cannot be found even in
  order to unblock them;
- it splits on `AND (fingerprint='')=?`, so **one call returns key-contacts or
  address-contacts and never both** — and since
  [ADR 0021](0021-autocrypt-key-contacts.md) the same correspondent is
  routinely one of each;
- it hides anyone below `Origin::IncomingReplyTo`, so somebody you have only
  ever been Cc'd alongside is invisible.

Each of those is correct for the member picker it was written for. All three
are wrong for an address book.

## Decision

**Extra fields live in side tables keyed by `contacts.id`**, not as columns on
upstream's `contacts`. `contact_details` holds organisation, job title, postal
address, website and notes; `contact_phones` holds numbers in the order the
user put them. This is the arrangement `contact_policy` already uses for
per-contact encryption and receipt overrides, and it keeps the merge surface at
zero: nothing here is ever consulted when deciding how to send a message, so it
has no business in the row that decides it.

**A phone number gets a table row, not a JSON column**, so it is searchable by
the same `LIKE` as the rest of the record. Numbers are stored exactly as typed —
formatting a phone number is how you break it.

**`email::addressbook::search` is our own query.** It joins the detail tables,
matches name, authname, address, organisation, job title, notes and phone,
excludes the reserved contact ids, and returns both kinds of contact row. It
takes `include_blocked` explicitly, because otherwise there is no way to look
up a blocked contact — a problem the moment somebody wants to undo a block.

**Categories are a separate vocabulary from message tags.** They have their own
table with their own colours, even though `email::labels` is nearly the same
shape. A label answers "where is this message"; a category answers "who is this
person". One table would put every category in the sidebar as a mail view and
every tag in the contact picker, and the two lists would grow into each other's
way.

**A record is written whole, never a field at a time.** The caller is an edit
form that holds every field, and a partial-update API cannot distinguish "clear
this field" from "leave it alone". The phone list is replaced rather than
reconciled by position, because a positional merge makes deleting the first
number look like renaming all of them.

**The contacts screen becomes master and detail**: a searchable, scrolling list
with category filter chips, and an editable record beside it. The QR block stays
and is collapsed — it is how verification happens and it is not what somebody
opening "Contacts" came for.

## Consequences

**Categories are local and not synced.** Labels sync because they ride an
existing per-message wire format; there is no such format for a contact
grouping, and inventing one is a larger decision than this. Someone with two
devices will categorise twice. That is a gap, recorded rather than hidden.

**The address book shows the same person twice** when they have both a
key-contact and an address-contact row. This is deliberate — it is the thing
`Contact::get_all` cannot express — and it is also confusing, because the two
rows differ in whether mail to them is encrypted. Merging them in the view
would hide exactly the distinction that matters. A future change could group
them under one heading while keeping both visible; collapsing them to one row
would not be that change.

**Nothing here is exported.** vCard import and export exist upstream
(`make_vcard`, `parse_vcard`) and know nothing about these tables, so a contact
exported from eeemail loses its record. Worth wiring up, and it is not wired up.

**`search` is a `LIKE` scan with no index**, like `email::search` beside it. Fine
at the sizes a personal mailbox reaches, and the place to look first if the
contacts screen ever feels slow.

**Deleting a category leaves its members alone.** A category is a grouping, not
a container, and deleting one must not take the people in it.
