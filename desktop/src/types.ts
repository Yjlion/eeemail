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

export type TrashReason = "deleted" | "expired" | "unaccepted";

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
  timestamp: number;
  unread: boolean;
  encrypted: boolean;
  verified: boolean;
  hasAttachment: boolean;
  tags: SystemTag[];
};

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
  isVerified: boolean;
  isBlocked?: boolean;
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
