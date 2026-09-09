/**
 * The address book: everyone this mailbox knows, and who they are.
 *
 * Contacts are still where the keys live -- a contact's verification state is
 * the only claim this client makes about identity that survives an active
 * attacker, so it stays the first thing every row shows. What changed is that
 * this is now also an address book: a list you can search, and a record you can
 * open and edit, rather than two `filter()`ed dumps under a QR code.
 *
 * Master and detail in one screen. The list is the master; selecting a row
 * opens the detail beside it, and `#/screen/contacts/<id>` selects one directly
 * so a record can be linked to and photographed.
 *
 * The QR block is still here and is now collapsed. It was taking the top of the
 * screen from the list, which is what someone opening "Contacts" came for.
 *
 * See `docs/adr/0028-contacts-are-an-address-book.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import { when } from "./list";
import type {
  BlocklistEntry,
  Contact,
  ContactCategory,
  ContactDetails,
} from "../types";

let contacts: Contact[] = [];
let blocked: BlocklistEntry[] = [];
let categories: ContactCategory[] = [];
let openDetails: ContactDetails | null = null;
let openCategoryIds: number[] = [];
let categoryFilter: number | null = null;
let qrSvg: string | null = null;
let query = "";

/**
 * Drops what belongs to the account that was open.
 *
 * These are module-level and so survive an account switch untouched by anything
 * that only clears `state`. Called by `accounts.ts`; nothing else should need
 * it, and nothing else should hold per-account data out here.
 */
export function resetContactsCache(): void {
  contacts = [];
  blocked = [];
  categories = [];
  openDetails = null;
  openCategoryIds = [];
  categoryFilter = null;
  qrSvg = null;
  query = "";
}

async function load(): Promise<void> {
  // The third parameter is a substring filter the engine applies itself, and
  // it was hardcoded `null` for as long as this screen has existed.
  // `search_contacts`, not `get_contacts`. The upstream call cannot answer
  // "everyone": it hardcodes `blocked=0`, returns key-contacts or
  // address-contacts but never both, and hides anyone below
  // `Origin::IncomingReplyTo`.
  contacts = (await rpc.call("search_contacts", [
    state.accountId,
    query.trim(),
    categoryFilter,
    false,
  ])) as Contact[];
  blocked = (await rpc.call("get_blocklist", [state.accountId])) as BlocklistEntry[];
  categories = (await rpc.call("get_contact_categories", [
    state.accountId,
  ])) as ContactCategory[];

  // Only for the record that is open. Fetching a record per row would be one
  // round trip per contact to render fields the list does not show.
  if (state.selectedContactId !== null) {
    const [details, of] = (await Promise.all([
      rpc.call("get_contact_details", [state.accountId, state.selectedContactId]),
      rpc.call("get_contact_categories_of", [state.accountId, state.selectedContactId]),
    ])) as [ContactDetails, ContactCategory[]];
    openDetails = details;
    openCategoryIds = of.map((c) => c.id);
  } else {
    openDetails = null;
    openCategoryIds = [];
  }
  if (qrSvg === null) {
    try {
      const code = (await rpc.call("get_chat_securejoin_qr_code", [
        state.accountId,
        null,
      ])) as string;
      qrSvg = (await rpc.call("create_qr_svg", [code])) as string;
    } catch {
      // An account that is not configured yet has no invite code. The rest of
      // the screen still works, so this is a missing panel, not an error.
      qrSvg = null;
    }
  }
}

function selected(): Contact | null {
  if (state.selectedContactId === null) return null;
  return contacts.find((c) => c.id === state.selectedContactId) ?? null;
}

function phoneRows(d: ContactDetails): string {
  // One spare blank row, so adding a number needs no click. The engine drops
  // an empty number rather than storing it.
  const rows = [...d.phones, { label: "", number: "" }];
  return rows.map(phoneRow).join("");
}

function phoneRow(p: { label: string; number: string }): string {
  return `
    <div class="phone-row">
      <input name="phoneLabel" placeholder="work" value="${escapeHtml(p.label)}" autocomplete="off" />
      <input name="phoneNumber" placeholder="Number" value="${escapeHtml(p.number)}" autocomplete="off" />
    </div>`;
}

function detail(c: Contact, d: ContactDetails): string {
  const seen = c.lastSeen > 0 ? when(c.lastSeen) : "never";
  return `
    <div class="contact-detail" data-contact-id="${c.id}">
      <h2 class="who">${escapeHtml(c.displayName || c.address)}</h2>
      <div class="badge-row">
        ${
          c.isVerified
            ? `<span class="badge verified">verified</span>`
            : `<span class="badge plain">unverified</span>`
        }
        ${c.e2eeAvail ? `<span class="badge enc">key held</span>` : ""}
        ${c.isBlocked ? `<span class="badge">blocked</span>` : ""}
      </div>

      <dl class="facts">
        <dt>Address</dt><dd>${escapeHtml(c.address)}</dd>
        <dt>Last seen</dt><dd>${escapeHtml(seen)}</dd>
      </dl>

      <form id="contact-record">
        <label>Name <input name="name" value="${escapeHtml(c.name ?? c.displayName)}" autocomplete="off" /></label>
        <div class="grid">
          <label>Organisation
            <input name="organisation" value="${escapeHtml(d.organisation)}" autocomplete="off" />
          </label>
          <label>Job title
            <input name="jobTitle" value="${escapeHtml(d.jobTitle)}" autocomplete="off" />
          </label>
        </div>
        <label>Website
          <input name="website" value="${escapeHtml(d.website)}" autocomplete="off" />
        </label>
        <label>Postal address
          <textarea name="postal" rows="2">${escapeHtml(d.postal)}</textarea>
        </label>
        <label>Notes
          <textarea name="notes" rows="2">${escapeHtml(d.notes)}</textarea>
        </label>

        <h3 class="section">Phone numbers</h3>
        <div id="phones">
          ${phoneRows(d)}
        </div>
        <button type="button" id="add-phone" class="quiet-add">Add a number</button>

        <h3 class="section">Categories</h3>
        ${
          categories.length
            ? `<div class="category-picker">
                ${categories
                  .map(
                    (cat) => `
                  <label class="check">
                    <input type="checkbox" data-category-id="${cat.id}"${
                      openCategoryIds.includes(cat.id) ? " checked" : ""
                    } />
                    <span class="dot" style="background:${escapeHtml(cat.color ?? "transparent")}"></span>
                    ${escapeHtml(cat.name)}
                  </label>`,
                  )
                  .join("")}
               </div>`
            : `<p class="hint">No categories yet. Make one below.</p>`
        }
        <div class="actions"><button type="submit">Save record</button></div>
      </form>

      <div class="contact-actions">
        <button data-act="${c.isBlocked ? "unblock" : "block"}">
          ${c.isBlocked ? "Unblock" : "Block"}
        </button>
        <button data-act="release">Release held mail</button>
        <button data-act="encryption-info">Encryption details</button>
        <button data-act="delete" class="danger">Delete contact</button>
      </div>
      <pre class="source" id="encryption-info" hidden></pre>
    </div>`;
}

export async function renderContacts(el: HTMLElement): Promise<void> {
  await load();

  const row = (c: Contact) => `
    <button class="contact" data-contact-id="${c.id}"
            aria-current="${c.id === state.selectedContactId ? "true" : "false"}">
      <span class="contact-who">
        <span class="name">${escapeHtml(c.displayName || c.address)}</span>
        <span class="addr">${escapeHtml(c.address)}</span>
      </span>
      <span class="contact-marks">
        ${c.isVerified ? `<span class="badge verified">verified</span>` : ""}
        ${c.isBlocked ? `<span class="badge">blocked</span>` : ""}
      </span>
    </button>`;

  const current = selected();

  el.innerHTML = `
    <div class="contacts">
      <h1>Contacts</h1>
      <p class="lede">
        A contact is where a correspondent's key lives. <strong>Verified</strong>
        means you completed a QR exchange with them, which is the only claim here
        that survives someone actively interfering. Anything else means the key
        was learned from mail they sent, which is worth having and is not proof.
      </p>

      <div class="contacts-body">
        <div class="contacts-list">
          ${
            categories.length
              ? `<div class="category-chips">
                  <button data-filter-category="all"
                          aria-current="${categoryFilter === null ? "true" : "false"}">All</button>
                  ${categories
                    .map(
                      (cat) => `
                    <button data-filter-category="${cat.id}"
                            aria-current="${categoryFilter === cat.id ? "true" : "false"}">
                      <span class="dot" style="background:${escapeHtml(cat.color ?? "transparent")}"></span>
                      ${escapeHtml(cat.name)}
                    </button>`,
                    )
                    .join("")}
                 </div>`
              : ""
          }
          <input id="contact-search" type="search" placeholder="Search contacts"
                 value="${escapeHtml(query)}" autocomplete="off" aria-label="Search contacts" />
          <div class="contact-rows">
            ${
              contacts.length
                ? contacts.map(row).join("")
                : `<div class="empty small">${
                    query.trim() ? "Nobody matches that." : "Nobody yet"
                  }</div>`
            }
          </div>
          <form id="add-contact" class="add-contact">
            <h2 class="section">Add a contact</h2>
            <label>Name <input name="name" autocomplete="off" /></label>
            <label>Address <input name="addr" type="email" required autocomplete="off" /></label>
            <button type="submit">Add</button>
          </form>
        </div>

        <div class="contacts-detail">
          ${
            current && openDetails
              ? detail(current, openDetails)
              : `<div class="empty small">Pick somebody to see their details.</div>`
          }
        </div>
      </div>

      <h2 class="section">Categories (${categories.length})</h2>
      <p class="hint">
        A way to group people. Separate from the tags you put on mail, so a
        category never turns up in the sidebar as a mailbox.
      </p>
      <div class="category-list">
        ${
          categories.length
            ? categories
                .map(
                  (cat) => `
          <div class="category-row">
            <span class="dot" style="background:${escapeHtml(cat.color ?? "transparent")}"></span>
            <span class="category-name">${escapeHtml(cat.name)}</span>
            <button data-act="delete-category" data-category-id="${cat.id}" class="danger">
              Delete
            </button>
          </div>`,
                )
                .join("")
            : `<div class="empty small">None yet</div>`
        }
      </div>
      <form id="new-category" class="inline-form">
        <label>New category <input name="name" autocomplete="off" required /></label>
        <input name="color" type="color" value="#2563eb" aria-label="Colour" />
        <button type="submit">Create</button>
      </form>

      <h2 class="section">Blocked (${blocked.length})</h2>
      <p class="hint">
        Mail from these senders is moved straight to the trash on arrival, where
        it waits out the usual window before being destroyed. Blocking somebody
        does not touch mail they already sent you.
      </p>
      <div class="blocklist">
        ${
          blocked.length
            ? blocked
                .map(
                  (b) => `
          <div class="block-row">
            <code>${escapeHtml(b.pattern)}</code>
            <button data-act="unblock-pattern" data-pattern="${escapeHtml(b.pattern)}">
              Remove
            </button>
          </div>`,
                )
                .join("")
            : `<div class="empty small">Nobody</div>`
        }
      </div>
      <form id="block-form" class="inline-form">
        <label>Block an address or a domain
          <input name="pattern" placeholder="spam@example.com or @example.com"
                 autocomplete="off" required />
        </label>
        <button type="submit">Block</button>
      </form>

      <details class="qr-block">
        <summary>Your invite code</summary>
        <p class="hint">
          Have them scan this, or scan theirs. Either direction verifies both of
          you.
        </p>
        <div class="qr" id="qr">${
          qrSvg ?? `<span class="hint">No invite code yet &mdash; finish setting up your account.</span>`
        }</div>
        <form id="scan-form" class="scan">
          <input name="qr" placeholder="Paste a scanned code" autocomplete="off" />
          <button type="submit">Verify from code</button>
        </form>
        <p class="hint">
          Camera scanning is not wired up: the Linux webview does not reliably
          give a page a camera, and a button that works on one platform and
          silently does nothing on another is worse than no button. Paste or use
          another device meanwhile.
        </p>
      </details>

      <div class="error" id="contacts-error" hidden></div>
    </div>
  `;

  const error = el.querySelector<HTMLElement>("#contacts-error")!;
  const fail = (err: unknown) => {
    error.hidden = false;
    error.className = "error";
    error.textContent = err instanceof Error ? err.message : String(err);
  };
  const note = (text: string) => {
    error.hidden = false;
    error.className = "notice";
    error.textContent = text;
  };

  // Debounced for the same reason the message search is: the query is a `LIKE`
  // over the contact table, and a keystroke is not a question.
  const search = el.querySelector<HTMLInputElement>("#contact-search")!;
  let timer: number | undefined;
  search.addEventListener("input", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      query = search.value;
      void renderContacts(el).then(() => {
        // Re-rendering replaces the input, so focus and caret have to be put
        // back or typing the second character lands nowhere.
        const next = el.querySelector<HTMLInputElement>("#contact-search");
        next?.focus();
        next?.setSelectionRange(next.value.length, next.value.length);
      });
    }, 200);
  });

  for (const button of el.querySelectorAll<HTMLButtonElement>("button.contact")) {
    button.addEventListener("click", () => {
      state.selectedContactId = Number(button.dataset["contactId"]);
      void renderContacts(el);
    });
  }

  el.querySelector<HTMLFormElement>("#scan-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const form = event.target as HTMLFormElement;
    const code = (form.elements.namedItem("qr") as HTMLInputElement).value.trim();
    if (!code) return;
    try {
      // Checked before joining: `check_qr` says what the code *is*, and a code
      // that turns out to be an account setup or a group invite must not be run
      // through the contact-verification path by accident.
      const parsed = (await rpc.call("check_qr", [state.accountId, code])) as {
        type: string;
      };
      if (!parsed.type.toLowerCase().includes("verify")) {
        throw new Error(`That code is a ${parsed.type}, not a contact invite.`);
      }
      await rpc.call("secure_join", [state.accountId, code]);
      await renderContacts(el);
      changed();
    } catch (err) {
      fail(err);
    }
  });

  // One spare row is rendered already; this adds another when it is used up.
  el.querySelector<HTMLButtonElement>("#add-phone")?.addEventListener("click", () => {
    const phones = el.querySelector<HTMLElement>("#phones");
    if (!phones) return;
    const row = document.createElement("div");
    row.className = "phone-row";
    row.innerHTML = `
      <input name="phoneLabel" placeholder="work" autocomplete="off" />
      <input name="phoneNumber" placeholder="Number" autocomplete="off" />`;
    phones.append(row);
  });

  el.querySelector<HTMLFormElement>("#contact-record")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (!current) return;
    const form = event.target as HTMLFormElement;
    const field = (name: string) =>
      (form.elements.namedItem(name) as HTMLInputElement | HTMLTextAreaElement).value.trim();

    // Read positionally: the two inputs of a row are siblings, and pairing them
    // by index across two `getElementsByName` lists would silently mismatch the
    // moment a row is missing one.
    const phones = [...form.querySelectorAll<HTMLElement>(".phone-row")]
      .map((row) => ({
        label: row.querySelector<HTMLInputElement>("[name='phoneLabel']")?.value.trim() ?? "",
        number: row.querySelector<HTMLInputElement>("[name='phoneNumber']")?.value.trim() ?? "",
      }))
      .filter((p) => p.number !== "");

    const wanted = [...form.querySelectorAll<HTMLInputElement>("input[data-category-id]")]
      .filter((box) => box.checked)
      .map((box) => Number(box.dataset["categoryId"]));

    try {
      await rpc.call("change_contact_name", [state.accountId, current.id, field("name")]);
      await rpc.call("set_contact_details", [
        state.accountId,
        current.id,
        {
          organisation: field("organisation"),
          jobTitle: field("jobTitle"),
          postal: field("postal"),
          website: field("website"),
          notes: field("notes"),
          phones,
        },
      ]);
      // Only the differences, so saving an unchanged record writes nothing.
      for (const id of wanted.filter((id) => !openCategoryIds.includes(id))) {
        await rpc.call("assign_contact_category", [state.accountId, current.id, id]);
      }
      for (const id of openCategoryIds.filter((id) => !wanted.includes(id))) {
        await rpc.call("unassign_contact_category", [state.accountId, current.id, id]);
      }
      await renderContacts(el);
      note("Record saved.");
    } catch (err) {
      fail(err);
    }
  });

  for (const chip of el.querySelectorAll<HTMLButtonElement>("button[data-filter-category]")) {
    chip.addEventListener("click", () => {
      const raw = chip.dataset["filterCategory"];
      const id = raw === "all" ? null : Number(raw);
      categoryFilter = categoryFilter === id ? null : id;
      void renderContacts(el);
    });
  }

  el.querySelector<HTMLFormElement>("#new-category")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const name = (form.elements.namedItem("name") as HTMLInputElement).value.trim();
    if (!name) return;
    const color = (form.elements.namedItem("color") as HTMLInputElement).value;
    try {
      await rpc.call("create_contact_category", [
        state.accountId,
        name,
        parseInt(color.replace(/^#/, ""), 16),
      ]);
      await renderContacts(el);
    } catch (err) {
      fail(err);
    }
  });

  for (const button of el.querySelectorAll<HTMLButtonElement>("[data-act='delete-category']")) {
    button.addEventListener("click", async () => {
      const id = Number(button.dataset["categoryId"]);
      const name = categories.find((c) => c.id === id)?.name ?? "";
      if (!window.confirm(`Delete the category "${name}"? The people in it stay.`)) return;
      try {
        await rpc.call("delete_contact_category", [state.accountId, id]);
        if (categoryFilter === id) categoryFilter = null;
        await renderContacts(el);
      } catch (err) {
        fail(err);
      }
    });
  }

  const detailEl = el.querySelector<HTMLElement>(".contact-detail");
  detailEl?.querySelector<HTMLButtonElement>("[data-act='release']")?.addEventListener(
    "click",
    async () => {
      if (!current) return;
      try {
        const released = (await rpc.call("release_held_contact", [
          state.accountId,
          current.id,
        ])) as number;
        note(
          released > 0
            ? `Released ${released} message(s) into the inbox.`
            : `Nothing released: this contact is still neither verified nor in your address book. Accept a message from them, or write to them.`,
        );
      } catch (err) {
        fail(err);
      }
    },
  );

  detailEl
    ?.querySelector<HTMLButtonElement>("[data-act='encryption-info']")
    ?.addEventListener("click", async () => {
      if (!current) return;
      const target = detailEl.querySelector<HTMLElement>("#encryption-info")!;
      try {
        target.textContent = (await rpc.call("get_contact_encryption_info", [
          state.accountId,
          current.id,
        ])) as string;
        target.hidden = false;
      } catch (err) {
        fail(err);
      }
    });

  detailEl?.querySelector<HTMLButtonElement>("[data-act='delete']")?.addEventListener(
    "click",
    async () => {
      if (!current) return;
      if (
        !window.confirm(
          `Delete ${current.displayName || current.address}?\n\n` +
            "Their mail stays. What is removed is the record -- including any " +
            "key held for them, so the next message to them may go out in " +
            "cleartext.",
        )
      ) {
        return;
      }
      try {
        await rpc.call("delete_contact", [state.accountId, current.id]);
        state.selectedContactId = null;
        await renderContacts(el);
      } catch (err) {
        fail(err);
      }
    },
  );

  for (const which of ["block", "unblock"] as const) {
    detailEl?.querySelector<HTMLButtonElement>(`[data-act='${which}']`)?.addEventListener(
      "click",
      async () => {
        if (!current) return;
        try {
          // `block_sender`, not upstream's `block_contact`. The latter marks
          // the contact row and leaves the mail arriving, which is the
          // behaviour this replaces; the eeemail call does both halves so no
          // caller can do one of them.
          await rpc.call(which === "block" ? "block_sender" : "unblock_sender", [
            state.accountId,
            current.id,
          ]);
          // A blocked contact drops out of `get_contacts`, which filters them,
          // so the selection has nothing left to point at.
          if (which === "block") state.selectedContactId = null;
          await renderContacts(el);
        } catch (err) {
          fail(err);
        }
      },
    );
  }

  el.querySelector<HTMLFormElement>("#block-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const form = event.target as HTMLFormElement;
    try {
      await rpc.call("add_to_blocklist", [
        state.accountId,
        (form.elements.namedItem("pattern") as HTMLInputElement).value.trim(),
        null,
      ]);
      await renderContacts(el);
    } catch (err) {
      // The engine refuses a pattern that could never match an address, and
      // saying so is the point -- an entry that silently never fires is one
      // the user believes is protecting them.
      fail(err);
    }
  });

  for (const button of el.querySelectorAll<HTMLButtonElement>("[data-act='unblock-pattern']")) {
    button.addEventListener("click", async () => {
      try {
        await rpc.call("remove_from_blocklist", [state.accountId, button.dataset["pattern"]]);
        await renderContacts(el);
      } catch (err) {
        fail(err);
      }
    });
  }

  el.querySelector<HTMLFormElement>("#add-contact")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const form = event.target as HTMLFormElement;
    try {
      const id = (await rpc.call("create_contact", [
        state.accountId,
        (form.elements.namedItem("addr") as HTMLInputElement).value.trim(),
        (form.elements.namedItem("name") as HTMLInputElement).value.trim() || null,
      ])) as number;
      // Creating a contact makes the sender trusted without releasing the mail
      // already held from them: `Contact::create` writes the new origin with a
      // direct UPDATE rather than through `scaleup_origin`, which is the only
      // place carrying the release hook. So the pair of calls is the feature,
      // and anyone adding a third "add this person" path owes both.
      await rpc.call("release_held_contact", [state.accountId, id]);
      state.selectedContactId = id;
      await renderContacts(el);
    } catch (err) {
      fail(err);
    }
  });
}
