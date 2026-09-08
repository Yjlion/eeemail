/**
 * A fake engine, for developing the UI and for screenshots.
 *
 * The reading client could only be looked at by pointing it at a real account,
 * which meant the only pictures of eeemail that could exist were pictures of
 * somebody's mail. This module answers the same JSON-RPC methods from canned
 * data, so the whole UI is developable offline and `screenshots/` regenerates
 * deterministically in CI without a mailbox.
 *
 * It is not a mock in the testing sense and proves nothing about the engine.
 * It exists so that what the UI *looks like* can be inspected without what the
 * UI *talks to* being involved.
 *
 * Enabled by `VITE_EEEMAIL_DEMO=1` at build time. In a normal build the bundler
 * drops this module entirely.
 */

import type { EventHandler } from "./rpc";

const HOUR = 3600;
const DAY = 86_400;
// Fixed, not `Date.now()`: a screenshot that changes every run is a diff nobody
// can read.
const NOW = 1_788_000_000;

type Row = {
  msgId: number;
  subject: string;
  preview: string;
  from: string;
  fromAddr: string;
  timestamp: number;
  unread: boolean;
  encrypted: boolean;
  verified: boolean;
  hasAttachment: boolean;
  tags: string[];
  body: string;
  to: string[];
  cc: string[];
  parent: number | null;
  structured?: { seq: number; json: string; trusted: boolean; source: string }[];
};

const ROWS: Row[] = [
  {
    msgId: 101,
    subject: "Re: Thursday's numbers",
    preview: "That matches what I have. One thing on the second column —",
    from: "Ada Okonjo",
    fromAddr: "ada@okonjo.example",
    timestamp: NOW - 2 * HOUR,
    unread: true,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["inbox"],
    body: "That matches what I have. One thing on the second column — the totals\nlook like they include the reversed entries from March. Worth checking\nbefore this goes out.\n\nAda",
    to: ["you@example.org"],
    cc: ["mira@dorn.example"],
    parent: 102,
  },
  {
    msgId: 102,
    subject: "Thursday's numbers",
    preview: "Attaching the reconciliation. Everything balances except the",
    from: "Mira Dorn",
    fromAddr: "mira@dorn.example",
    timestamp: NOW - 5 * HOUR,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: true,
    tags: ["inbox"],
    body: "Attaching the reconciliation. Everything balances except the two entries\nI flagged in red.\n\nMira",
    to: ["you@example.org", "ada@okonjo.example"],
    cc: [],
    parent: null,
  },
  {
    msgId: 103,
    subject: "Keys are rotated",
    preview: "Done. New fingerprint is in the usual place; scan when you get",
    from: "Tomas Reyes",
    fromAddr: "tomas@reyes.example",
    timestamp: NOW - DAY,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["inbox"],
    body: "Done. New fingerprint is in the usual place; scan when you get a chance.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    msgId: 104,
    subject: "Notes from the standup",
    preview: "Short one today. Three things worth writing down:",
    from: "Mira Dorn",
    fromAddr: "mira@dorn.example",
    timestamp: NOW - 2 * DAY,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["inbox"],
    body: "Short one today. Three things worth writing down:\n\n1. The migration is done.\n2. Nobody has looked at the backup restore path.\n3. We still owe an answer on the retention default.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    msgId: 105,
    subject: "Invoice 2291",
    preview: "Please find attached invoice 2291 for services rendered in",
    from: "billing@vendor.example",
    fromAddr: "billing@vendor.example",
    timestamp: NOW - 3 * DAY,
    unread: false,
    encrypted: false,
    verified: false,
    hasAttachment: true,
    tags: ["archive"],
    body: "Please find attached invoice 2291 for services rendered in August.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    msgId: 106,
    subject: "quick question about your listing",
    preview: "Hi, I saw your post and wanted to ask whether it is still",
    from: "unknown@elsewhere.example",
    fromAddr: "unknown@elsewhere.example",
    timestamp: NOW - 4 * HOUR,
    unread: true,
    encrypted: false,
    verified: false,
    hasAttachment: false,
    tags: ["unverified"],
    body: "Hi, I saw your post and wanted to ask whether it is still available.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
    // Deliberately on the held message: a stranger's structured data is
    // exactly the case that must render inert, and putting it here means the
    // existing `unverified` screenshot shows it.
    structured: [
      {
        seq: 0,
        trusted: false,
        source: "htmlScript",
        json: JSON.stringify({
          "@context": "https://schema.org",
          "@type": "Offer",
          name: "Bicycle, blue",
          price: "180.00",
          priceCurrency: "EUR",
          url: "https://elsewhere.example/listing/4417",
        }),
      },
    ],
  },
  {
    msgId: 107,
    subject: "You have won",
    preview: "CONGRATULATIONS you have been selected as our",
    from: "prizes@nowhere.example",
    fromAddr: "prizes@nowhere.example",
    timestamp: NOW - DAY,
    unread: true,
    encrypted: false,
    verified: false,
    hasAttachment: false,
    tags: ["unverified"],
    body: "CONGRATULATIONS you have been selected as our monthly winner.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    msgId: 108,
    subject: "Re: Thursday's numbers",
    preview: "Checked — you are right, March is double counted. Fixing now.",
    from: "You",
    fromAddr: "you@example.org",
    timestamp: NOW - HOUR,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["sent"],
    body: "Checked — you are right, March is double counted. Fixing now.",
    to: ["ada@okonjo.example"],
    cc: ["mira@dorn.example"],
    parent: 101,
  },
  {
    msgId: 109,
    subject: "Dinner Friday?",
    preview: "This one had a timer on it and has expired into the trash.",
    from: "Ada Okonjo",
    fromAddr: "ada@okonjo.example",
    timestamp: NOW - 6 * DAY,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["trash"],
    body: "This one had a timer on it and has expired into the trash. It is still\nhere, and still readable, until the purge window runs out.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    // Swept out of Unverified rather than expired or thrown away. The third
    // route into the trash needs a fixture of its own, because it is the one
    // the reading pane has to explain -- nobody remembers doing this, since
    // nobody did it.
    msgId: 111,
    subject: "Following up on my last message",
    preview: "Nobody accepted this sender, so it waited and then moved here.",
    from: "unknown@elsewhere.example",
    fromAddr: "unknown@elsewhere.example",
    timestamp: NOW - 34 * DAY,
    unread: false,
    encrypted: false,
    verified: false,
    hasAttachment: false,
    tags: ["trash"],
    body: "Just checking whether you saw my earlier note.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
  },
  {
    msgId: 110,
    subject: "Your parcel is on its way",
    preview: "Dispatched today. Expected between Thursday and Friday.",
    from: "Mira Dorn",
    fromAddr: "mira@dorn.example",
    timestamp: NOW - 3 * HOUR,
    unread: false,
    encrypted: true,
    verified: true,
    hasAttachment: false,
    tags: ["inbox"],
    body: "Dispatched today. Expected between Thursday and Friday.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
    structured: [
      {
        seq: 0,
        trusted: true,
        source: "alternative",
        json: JSON.stringify({
          "@context": "https://schema.org",
          "@type": "ParcelDelivery",
          trackingNumber: "XQ-4417-2290",
          deliveryAddress: { addressLocality: "Leipzig", postalCode: "04109" },
          expectedArrivalFrom: "2026-09-03",
          expectedArrivalUntil: "2026-09-04",
        }),
      },
    ],
  },
];

const LABELS = [
  { id: 1, name: "Archive", color: null, isSystem: true },
  { id: 2, name: "Unverified", color: null, isSystem: true },
  { id: 3, name: "Trash", color: null, isSystem: true },
  { id: 10, name: "Accounts", color: "#2563eb", isSystem: false },
  { id: 11, name: "Reading list", color: "#15803d", isSystem: false },
];

/** Two profiles, so the switcher is drawn -- it is hidden below two. */
const ACCOUNTS = [
  {
    kind: "Configured",
    id: 1,
    addr: "you@example.org",
    displayName: "You",
    profileImage: null,
    color: "#2563eb",
    privateTag: null,
  },
  {
    kind: "Configured",
    id: 2,
    addr: "you@work.example",
    displayName: null,
    profileImage: null,
    color: "#15803d",
    privateTag: "Work",
  },
];

const contact = (
  id: number,
  address: string,
  name: string,
  isVerified: boolean,
  extra: Partial<{ isBlocked: boolean; isKeyContact: boolean; e2eeAvail: boolean; lastSeen: number }> = {},
) => ({
  id,
  address,
  name,
  displayName: name || address,
  nameAndAddr: name ? `${name} (${address})` : address,
  isVerified,
  isBlocked: false,
  isKeyContact: isVerified,
  e2eeAvail: isVerified,
  // A fixed offset from the pinned `NOW`, never the wall clock: a screenshot
  // that renders a real timestamp changes on a run that changed no code.
  lastSeen: NOW - 3 * DAY,
  color: "#2563eb",
  ...extra,
});

const CONTACTS = [
  contact(20, "ada@okonjo.example", "Ada Okonjo", true),
  contact(21, "mira@dorn.example", "Mira Dorn", true),
  contact(22, "tomas@reyes.example", "Tomas Reyes", true),
  contact(23, "billing@vendor.example", "Vendor Billing", false, { isKeyContact: false }),
  contact(24, "unknown@elsewhere.example", "", false, { isKeyContact: false }),
];

/** A recognisable but meaningless QR, so the screenshot shows the real layout. */
function demoQrSvg(): string {
  const cells: string[] = [];
  // Deterministic pseudo-random fill: a screenshot must not change between runs.
  let seed = 7;
  for (let y = 0; y < 25; y++) {
    for (let x = 0; x < 25; x++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      const finder = (x < 7 && y < 7) || (x > 17 && y < 7) || (x < 7 && y > 17);
      if (finder ? (x + y) % 2 === 0 || x % 6 === 0 || y % 6 === 0 : seed % 3 === 0) {
        cells.push(`<rect x="${x}" y="${y}" width="1" height="1"/>`);
      }
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 25 25" width="220" height="220" shape-rendering="crispEdges"><rect width="25" height="25" fill="#fff"/><g fill="#111">${cells.join("")}</g></svg>`;
}

function rowOf(msgId: number): Row | undefined {
  return ROWS.find((r) => r.msgId === msgId);
}

/** Answers the same methods as `Rpc`, from the data above. */
/** Core's contact id for the account itself, and the demo's own address. */
const SELF_CONTACT_ID = 1;
const SELF_ADDRESS = "you@example.org";

export class DemoRpc {
  async ready(): Promise<void> {}

  onEvent(_handler: EventHandler): () => void {
    // Nothing arrives in a demo: there is no engine to push anything.
    return () => {};
  }

  async call(method: string, params: unknown[] = []): Promise<unknown> {
    const arg = <T>(i: number): T => params[i] as T;

    switch (method) {
      case "get_all_account_ids":
        return ACCOUNTS.map((a) => a.id);
      case "get_all_accounts":
        return ACCOUNTS;
      case "get_selected_account_id":
        return 1;
      case "get_connectivity":
        return 4000;
      case "apply_eeemail_defaults":
        return null;
      case "get_labels":
        return LABELS;
      case "get_inbox_gating":
        return true;
      case "get_unverified_trash_days":
        return 30;
      case "get_trash_purge_days":
        return 30;

      case "get_tagged_messages": {
        const tag = arg<string>(1);
        return ROWS.filter((r) => r.tags.includes(tag)).map((r) => r.msgId);
      }
      case "get_label_messages": {
        const id = arg<number>(1);
        // Only the demo user tags carry messages; enough to show a populated view.
        return id === 10 ? [105] : id === 11 ? [104] : [];
      }
      case "search_email": {
        const q = (arg<{ text?: string; tag?: string }>(1) ?? {}) as {
          text?: string;
          tag?: string;
        };
        const needle = (q.text ?? "").toLowerCase();
        return ROWS.filter(
          (r) =>
            (!q.tag || r.tags.includes(q.tag)) &&
            (!needle ||
              r.subject.toLowerCase().includes(needle) ||
              r.body.toLowerCase().includes(needle) ||
              r.from.toLowerCase().includes(needle)),
        ).map((r) => r.msgId);
      }

      case "get_message_rows": {
        const ids = arg<number[]>(1);
        return ids
          .map(rowOf)
          .filter((r): r is Row => r !== undefined)
          .map(({ body, to, cc, parent, fromAddr, structured, ...row }) => ({
            ...row,
            // What the engine computes from message state and the recipient
            // set. Derived here from the same data so a Sent row in the demo
            // reads the way a Sent row reads against a real mailbox.
            outgoing: row.tags.includes("sent"),
            to: row.tags.includes("sent") ? to.join(", ") : "",
          }));
      }
      case "get_message": {
        const r = rowOf(arg<number>(1));
        return r
          ? {
              id: r.msgId,
              chatId: r.msgId,
              subject: r.subject,
              text: r.body,
              hasHtml: false,
              fromId: r.msgId,
            }
          : null;
      }
      case "get_contact": {
        const id = arg<number>(1);
        // Contact 1 is the account itself, in a demo as in a real profile.
        // Answering it with a message row would tell the reply path that the
        // user is whoever sent message 1, and reply-all would then drop that
        // person as though they were self.
        if (id === SELF_CONTACT_ID) {
          return { id, address: SELF_ADDRESS, displayName: "You", isVerified: true };
        }
        const known = CONTACTS.find((c) => c.id === id);
        if (known) return known;
        // Otherwise the id is a `fromId`, which the demo keys to the message
        // row it came from -- see `get_message`.
        const r = rowOf(id);
        return {
          id,
          address: r?.fromAddr ?? "someone@example.org",
          displayName: r?.from ?? "",
          isVerified: r?.verified ?? false,
        };
      }
      case "get_message_raw_mime":
      case "get_message_raw_mime_bytes":
        // A demo has no stored bytes. `null` is what the engine returns once
        // retention has elapsed, and it is the branch worth photographing.
        return null;
      case "get_connectivity":
        // Connected: a demo build has no engine, and a red dot in every
        // screenshot would be a claim about the software rather than the mailbox.
        return 4000;
      case "get_structured_data":
        return rowOf(arg<number>(1))?.structured ?? [];
      case "get_message_html":
        return null;
      case "get_message_recipients": {
        const r = rowOf(arg<number>(1));
        if (!r) return [];
        return [
          ...r.to.map((addr) => ({ kind: "to", addr, name: "" })),
          ...r.cc.map((addr) => ({ kind: "cc", addr, name: "" })),
        ];
      }
      case "get_message_crypto": {
        const r = rowOf(arg<number>(1));
        return {
          encrypted: r?.encrypted ?? false,
          signed: r?.encrypted ?? false,
          verified: r?.verified ?? false,
        };
      }
      case "get_message_tags": {
        const r = rowOf(arg<number>(1));
        return { system: r?.tags ?? [], user: r?.msgId === 105 ? [LABELS[3]] : [] };
      }
      case "get_trashed_message": {
        const r = rowOf(arg<number>(1));
        if (!r?.tags.includes("trash")) return null;
        // 111 is the swept one. The reason is what decides which sentence the
        // reading pane shows, and "you deleted this" would be a lie about mail
        // the user never touched.
        return r.msgId === 111
          ? { trashedAt: NOW - 4 * DAY, purgeAt: NOW + 26 * DAY, reason: "unaccepted" }
          : { trashedAt: NOW - 6 * DAY, purgeAt: NOW + 24 * DAY, reason: "expired" };
      }
      case "get_message_ephemeral_timer":
        return null;
      case "is_message_raw_mime_retained":
        return true;
      case "get_undelivered_recipients":
        return [];
      case "get_message_thread": {
        const r = rowOf(arg<number>(1));
        return r && [101, 102, 108].includes(r.msgId) ? 1 : null;
      }
      case "get_thread_tree":
        return [
          { msgId: 102, parentMsgId: null, depth: 0 },
          { msgId: 101, parentMsgId: 102, depth: 1 },
          { msgId: 108, parentMsgId: 101, depth: 2 },
        ];

      case "get_contacts":
      case "get_contacts_by_ids": {
        // The engine filters server-side on this parameter, so the demo has to
        // as well or the search box appears to do nothing.
        const q = (arg<string | null>(2) ?? "").trim().toLowerCase();
        if (!q) return CONTACTS;
        return CONTACTS.filter(
          (c) =>
            c.address.toLowerCase().includes(q) || c.displayName.toLowerCase().includes(q),
        );
      }
      case "get_contact":
        return CONTACTS.find((c) => c.id === arg<number>(1)) ?? null;
      case "get_contact_encryption_info":
        return "End-to-end encryption available.\nFingerprint: DEMO 0000 1111 2222 3333";
      case "get_chat_securejoin_qr_code":
        return "OPENPGP4FPR:DEMO#a=you%40example.org&n=You&i=demo&s=demo";
      case "create_qr_svg":
        return demoQrSvg();
      case "check_qr":
        return { type: "askVerifyContact", id: 20, text1: "ada@okonjo.example" };

      case "get_at_rest_protection":
        return {
          databaseEncrypted: true,
          blobsEncrypted: false,
          cleartextBytes: 41_943_040,
          partial: true,
          summary:
            "Database encrypted, but 40.0 MB of attachments and original message sources remain in cleartext. Use filesystem or full-disk encryption for complete protection.",
        };
      case "get_blob_encryption":
        return false;
      case "get_encryption_mode":
        return "opportunistic";
      case "get_mdn_policy":
        return "always";
      case "get_server_retention":
        return { mode: "deleteAfterDownload", days: 0 };
      case "get_ephemeral_default":
        return 0;
      case "get_config":
        return null;

      // Everything that writes is accepted and forgotten: a demo that pretended
      // to send mail would be lying about the one thing that matters.
      default:
        return null;
    }
  }
}
