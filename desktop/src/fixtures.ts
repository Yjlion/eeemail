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
    tags: ["holding"],
    body: "Hi, I saw your post and wanted to ask whether it is still available.",
    to: ["you@example.org"],
    cc: [],
    parent: null,
    // Deliberately on the held message: a stranger's structured data is
    // exactly the case that must render inert, and putting it here means the
    // existing `holding` screenshot shows it.
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
    tags: ["holding"],
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
  { id: 2, name: "Holding", color: null, isSystem: true },
  { id: 3, name: "Trash", color: null, isSystem: true },
  { id: 10, name: "Accounts", color: "#2563eb", isSystem: false },
  { id: 11, name: "Reading list", color: "#15803d", isSystem: false },
];

const CONTACTS = [
  { id: 20, address: "ada@okonjo.example", displayName: "Ada Okonjo", isVerified: true },
  { id: 21, address: "mira@dorn.example", displayName: "Mira Dorn", isVerified: true },
  { id: 22, address: "tomas@reyes.example", displayName: "Tomas Reyes", isVerified: true },
  { id: 23, address: "billing@vendor.example", displayName: "Vendor Billing", isVerified: false },
  {
    id: 24,
    address: "unknown@elsewhere.example",
    displayName: "unknown@elsewhere.example",
    isVerified: false,
  },
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
        return [1];
      case "apply_eeemail_defaults":
        return null;
      case "get_labels":
        return LABELS;
      case "get_inbox_gating":
        return true;
      case "get_hold_days":
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
          .map(({ body, to, cc, parent, fromAddr, structured, ...row }) => row);
      }
      case "get_message": {
        const r = rowOf(arg<number>(1));
        return r
          ? { id: r.msgId, subject: r.subject, text: r.body, hasHtml: false }
          : null;
      }
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
        return r?.tags.includes("trash")
          ? { trashedAt: NOW - 6 * DAY, purgeAt: NOW + 24 * DAY, reason: "expired" }
          : null;
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
      case "get_contacts_by_ids":
        return CONTACTS;
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
