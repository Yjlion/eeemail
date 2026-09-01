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

export type EncryptionMode = "strict" | "opportunistic" | "lenient";
export type MdnPolicy = "never" | "verifiedOnly" | "always";

export type Message = {
  id: number;
  chatId: number;
  subject: string;
  text: string;
  fromId: number;
  timestamp: number;
  state: string;
};

export type Contact = {
  id: number;
  address: string;
  displayName: string;
  isVerified: boolean;
};
