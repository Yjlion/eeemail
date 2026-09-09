/**
 * The slice of the RPC surface this UI uses.
 *
 * Hand-written rather than imported from `deltachat-jsonrpc`'s generated
 * bindings: that file describes ~200 methods, and pinning only what we consume
 * makes it obvious when the UI starts depending on something new. The
 * generated bindings remain the source of truth for the shapes.
 */

export type RecipientKind = "to" | "cc" | "bcc";

export type Recipient = {
  kind: RecipientKind;
  addr: string;
  name: string;
};

export type Label = {
  id: number;
  name: string;
  color: string | null;
  isSystem: boolean;
};

/**
 * A tag every account has without the user creating anything.
 *
 * Three of these are stored as reserved labels and three are derived from
 * message state. Which is which is deliberately not visible here: the engine
 * returns them through one type so a client cannot get the rule wrong.
 * See `docs/adr/0017-system-tags.md`.
 */
export type SystemTag =
  | "inbox"
  | "unverified"
  | "sent"
  | "drafts"
  | "archive"
  | "trash";

export const SYSTEM_TAGS: SystemTag[] = [
  "inbox",
  "unverified",
  "sent",
  "drafts",
  "archive",
  "trash",
];

export const TAG_LABELS: Record<SystemTag, string> = {
  inbox: "Inbox",
  unverified: "Unverified",
  sent: "Sent",
  drafts: "Drafts",
  archive: "Archive",
  trash: "Trash",
};

export type MessageTags = {
  system: SystemTag[];
  user: Label[];
};

export type TrashReason = "deleted" | "expired" | "unaccepted" | "blocked";

export type TrashedMessage = {
  trashedAt: number;
  purgeAt: number;
  reason: TrashReason;
};

export type ThreadItem = {
  msgId: number;
  parentMsgId: number | null;
  depth: number;
};

export type MessageCrypto = {
  encrypted: boolean;
  signed: boolean;
  verified: boolean;
};

/** Everything a list row needs, from one RPC rather than two per row. */
export type MessageRow = {
  msgId: number;
  subject: string;
  preview: string;
  from: string;
  /** Sent by this account rather than received. */
  outgoing: boolean;
  /** Who it went to. Empty on incoming mail, where `from` is the useful name. */
  to: string;
  timestamp: number;
  unread: boolean;
  encrypted: boolean;
  verified: boolean;
  hasAttachment: boolean;
  /** As the sender marked it. Absent from the wire entirely when normal. */
  importance: Importance;
  tags: SystemTag[];
};

/** How important a message claims to be. */
export type Importance = "high" | "normal" | "low";

export type EncryptionMode = "strict" | "opportunistic" | "lenient";
export type MdnPolicy = "never" | "verifiedOnly" | "always";

export type Message = {
  id: number;
  chatId: number;
  subject: string;
  text: string;
  hasHtml?: boolean;
  fromId: number;
  timestamp: number;
  state: string;
};

export type Contact = {
  id: number;
  address: string;
  displayName: string;
  /** What the user called them, empty if only the sender's own name is known. */
  name: string;
  isVerified: boolean;
  isBlocked: boolean;
  /** `Name (addr@example.com)`, as the engine formats it. */
  nameAndAddr: string;
  /**
   * Whether this row is keyed on a key rather than only on an address.
   *
   * The same correspondent is routinely two rows: an address-contact from mail
   * sent to them, and a key-contact from the encrypted reply. The address book
   * shows both, because hiding one is how a user ends up wondering why the
   * person they verified still gets cleartext.
   */
  isKeyContact: boolean;
  /** Whether a key is actually held, which a key-contact does not guarantee. */
  e2eeAvail: boolean;
  lastSeen: number;
  color: string;
};

export type RecipientSet = {
  to: string[];
  cc: string[];
  bcc: string[];
};

/** What at-rest protection is actually in force. Rendered verbatim. */
export type AtRestProtection = {
  databaseEncrypted: boolean;
  blobsEncrypted: boolean;
  cleartextBytes: number;
  partial: boolean;
  summary: string;
};

/** Where a structured object came from, and so what it claims to represent. */
export type StructuredSource = "alternative" | "related" | "mixed" | "htmlScript";

/**
 * Machine-readable data a message carried about itself.
 *
 * `trusted` is computed by the engine at receive and is the only thing that
 * may change how this renders. Untrusted objects are shown inert: labelled
 * fields, no links, no buttons, nothing that initiates a request.
 * See `docs/adr/0016-structured-email.md`.
 */
export type StructuredObject = {
  seq: number;
  json: string;
  trusted: boolean;
  source: StructuredSource;
};

/**
 * One profile in the account list.
 *
 * Mirrors the engine's `Account`, which is a `kind`-tagged union: an account
 * exists from the moment it is created and is `Unconfigured` until a transport
 * is added. Both kinds come back from `get_all_accounts`, so the switcher has
 * to expect a profile with no address at all -- that is a setup someone
 * abandoned half-way, and hiding it would leave them no way back to it.
 */
export type Account =
  | {
      kind: "Configured";
      id: number;
      addr: string | null;
      displayName: string | null;
      profileImage: string | null;
      color: string;
      privateTag: string | null;
    }
  | { kind: "Unconfigured"; id: number };

/** What to call an account in a list, when it may not have an address yet. */
export function accountLabel(account: Account): string {
  if (account.kind === "Unconfigured") return "Unfinished setup";
  return account.displayName || account.addr || "Unfinished setup";
}

/** One entry in the blocklist: an address, or `@example.com` for a domain. */
export type BlocklistEntry = {
  id: number;
  pattern: string;
  added: number;
  reason: string;
};

/** One phone number on a contact record. */
export type ContactPhone = {
  label: string;
  number: string;
};

/**
 * What the address book knows about somebody, beyond their contact row.
 *
 * Written back whole rather than a field at a time: a partial update cannot
 * distinguish "clear this field" from "leave it alone".
 */
export type ContactDetails = {
  organisation: string;
  jobTitle: string;
  postal: string;
  website: string;
  notes: string;
  phones: ContactPhone[];
};

/** A user-defined grouping of contacts, with its own colour. */
export type ContactCategory = {
  id: number;
  name: string;
  color: string | null;
};
