/**
 * What can be done to a message, in one place.
 *
 * The reading pane's button row and the context menu offer overlapping sets of
 * the same actions, and having each implement its own was how "Accept sender"
 * ended up being the only thing an unverified message could do. So the item
 * list and the behaviour both live here, and each surface decides only which
 * subset to show.
 */

import { rpc } from "./client";
import { state, changed } from "./state";
import { reload } from "./nav";
import { exportFile } from "./shell";
import type { MenuItem } from "./views/menu";
import type { Contact, Message, Recipient, SystemTag } from "./types";

/** What the caller knows about the message the menu was opened on. */
export type Target = {
  msgId: number;
  tags: SystemTag[];
  /** Whether the original bytes are still retained, where the caller knows. */
  retained?: boolean;
};

/**
 * The menu for one message.
 *
 * Archive and Trash appear as their inverse where the message is already there,
 * rather than twice or greyed out: "Archive" on something already archived is a
 * question the user should not have to answer.
 */
export function itemsFor(target: Target): MenuItem[] {
  const has = (tag: SystemTag) => target.tags.includes(tag);
  const items: MenuItem[] = [
    { label: "Reply", act: "reply" },
    { label: "Reply all", act: "reply-all" },
    null,
    has("archive")
      ? { label: "Move to inbox", act: "unarchive" }
      : { label: "Archive", act: "archive" },
  ];

  if (has("trash")) {
    items.push({ label: "Restore", act: "restore" });
    items.push({ label: "Delete permanently…", act: "delete-forever", danger: true });
  } else {
    items.push({ label: "Trash", act: "trash" });
  }

  if (has("unverified")) {
    items.push(null);
    items.push({ label: "Add sender to contacts", act: "add-contact" });
    items.push({ label: "Verify by code…", act: "verify" });
  }

  items.push(null);
  items.push({ label: "Tags…", act: "tags" });
  items.push({ label: "Block sender…", act: "block-sender", danger: true });
  items.push(null);
  items.push({
    label: "View source",
    act: "view-source",
    disabled: target.retained === false,
  });
  items.push({
    label: "Export…",
    act: "export",
    disabled: target.retained === false,
  });
  items.push({ label: "Copy sender address", act: "copy-address" });
  return items;
}

/**
 * Runs one action. Returns `true` when the message list has to be re-read.
 *
 * Loads what it needs rather than taking it as an argument: the list pane has a
 * row and the reading pane has the whole message, and making them agree on a
 * payload would put the union of both in every call site. These run because
 * somebody clicked, so an extra round trip costs nothing that matters.
 */
export async function run(act: string, msgId: number): Promise<boolean> {
  const account = state.accountId;

  switch (act) {
    case "reply":
    case "reply-all":
      await openComposer(msgId, act === "reply-all");
      return false;

    case "archive":
      await rpc.call("archive_messages", [account, [msgId]]);
      return true;
    case "unarchive":
      await rpc.call("unarchive_messages", [account, [msgId]]);
      return true;
    case "trash":
      await rpc.call("trash_messages", [account, [msgId]]);
      return true;
    case "restore":
      await rpc.call("restore_messages", [account, [msgId]]);
      return true;

    case "delete-forever": {
      // The trash is the only place in eeemail that destroys mail, and this is
      // the one control that skips its recoverable window. So it is confirmed,
      // and the confirmation says what is lost rather than asking "are you
      // sure" about a word the user cannot check.
      const ok = window.confirm(
        "Delete this message permanently?\n\n" +
          "It is removed from this device for good. eeemail keeps mail nowhere " +
          "else, so there is no copy on the server to fetch again.",
      );
      if (!ok) return false;
      await rpc.call("delete_trashed_messages", [account, [msgId]]);
      return true;
    }

    case "tags": {
      const { showTagPicker } = await import("./views/tagpicker");
      // A tag change can move the message out of the view listing it, so the
      // list is re-read only when something was actually applied or removed.
      return await showTagPicker(msgId);
    }

    case "block-sender": {
      const msg = (await rpc.call("get_message", [account, msgId])) as Message;
      const contact = (await rpc.call("get_contact", [account, msg.fromId])) as Contact;
      if (
        !window.confirm(
          `Block ${contact.displayName || contact.address}?\n\n` +
            "Mail from them is moved straight to the trash from now on. This " +
            "message and anything else they have already sent stays where it is.",
        )
      ) {
        return false;
      }
      await rpc.call("block_sender", [account, msg.fromId]);
      return true;
    }

    case "add-contact":
      return await addSenderToContacts(msgId);

    case "verify":
      // Into the flow that already exists, rather than a second one here.
      // Verification is SecureJoin and nothing else, so it needs the other
      // side's code, and asking for that is what the Contacts screen does.
      state.screen = "contacts";
      changed();
      return false;

    case "view-source": {
      const { showSource } = await import("./views/source");
      await showSource(msgId);
      return false;
    }

    case "export":
      await exportMessage(msgId);
      return false;

    case "copy-address": {
      const msg = (await rpc.call("get_message", [account, msgId])) as Message;
      const contact = (await rpc.call("get_contact", [account, msg.fromId])) as Contact;
      await navigator.clipboard.writeText(contact.address);
      return false;
    }
  }
  return false;
}

/**
 * Makes the sender a contact, and releases their held mail.
 *
 * **Two calls, and the second is not optional.** `create_contact` reaches
 * core's `add_or_lookup`, which writes the new origin directly rather than
 * through `scaleup_origin` -- and `scaleup_origin` is the only place carrying
 * the hook that releases held mail. So creating the contact makes the sender
 * *trusted* and leaves their mail *held and invisible*, until the sweep moves
 * it to the trash weeks later. `release_held_contact` re-checks trust itself,
 * so calling it here is correct whatever the contact turned out to be.
 *
 * Note what this claims and what it does not. It makes the sender **known**,
 * which is one of the two ways mail leaves the unverified view. It does not
 * make them *verified*: that means SecureJoin, it is earned rather than set,
 * and the "verified contact" badge means the stronger thing.
 */
async function addSenderToContacts(msgId: number): Promise<boolean> {
  const account = state.accountId;
  const msg = (await rpc.call("get_message", [account, msgId])) as Message;
  const contact = (await rpc.call("get_contact", [account, msg.fromId])) as Contact;

  await rpc.call("create_contact", [account, contact.address, contact.displayName || null]);
  await rpc.call("release_held_contact", [account, msg.fromId]);
  return true;
}

/** Writes the original bytes of a message out as a `.eml`. */
async function exportMessage(msgId: number): Promise<void> {
  const account = state.accountId;
  // The byte-exact variant, not `get_message_raw_mime`: that one replaces
  // invalid sequences so it can be a JSON string, which is right for showing
  // source on screen and wrong for a file meant to be what arrived.
  const bytes = (await rpc.call("get_message_raw_mime_bytes", [account, msgId])) as
    | number[]
    | null;
  if (bytes === null) {
    window.alert(
      "The original source of this message has expired and is no longer available, " +
        "so there is nothing to export.",
    );
    return;
  }
  const msg = (await rpc.call("get_message", [account, msgId])) as Message;
  const path = await exportFile(fileNameFor(msg.subject), new Uint8Array(bytes));
  // `null` is the user cancelling the save dialog, which is not an outcome to
  // report back at them.
  if (path !== null) window.alert(`Saved to ${path}`);
}

/**
 * A `.eml` name from a subject, or a neutral one when there is no subject.
 *
 * Only makes the name readable. Making it *safe* is the shell's job, which
 * reduces whatever arrives to a last component -- a subject is
 * attacker-controlled on every received message, so the rule belongs where it
 * cannot be skipped by a second caller.
 */
function fileNameFor(subject: string | undefined): string {
  const base = (subject ?? "")
    .replace(/[^\p{L}\p{N} ._-]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!base) return "message.eml";
  return `${[...base].slice(0, 60).join("").trim()}.eml`;
}

/**
 * Core's contact id for the account itself. Fixed, and the same in every
 * profile.
 */
const SELF_CONTACT_ID = 1;

/**
 * Case-insensitive dedupe that keeps the first spelling and drops `exclude`.
 *
 * Case-insensitive because a domain is, and because the same person spelled two
 * ways in `To:` and `Cc:` is a duplicate the user has to delete by hand.
 */
function addresses(addrs: string[], exclude: string[]): string[] {
  const seen = new Set(exclude.map((a) => a.toLowerCase()));
  const out: string[] = [];
  for (const addr of addrs) {
    const key = addr.trim().toLowerCase();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(addr.trim());
  }
  return out;
}

/**
 * Fills the composer as a reply to `msgId`.
 *
 * **A reply goes to the sender, which is not the message's `To`.** On receive,
 * `msg_recipients` stores the incoming `To:` and `Cc:` headers verbatim -- so
 * on anything the user received, `To` is *the user's own address*. Reading the
 * reply's addressee out of it addresses the reply to yourself and leaves the
 * sender off it entirely. The recipient set is the right thing to keep and the
 * wrong thing to reply to; the sender comes from `fromId`.
 *
 * Replying to your own sent mail is the exception: there the sender is you, and
 * what the user means is another message to the same people, so the stored `To`
 * is exactly right.
 *
 * Reply-all adds everyone else the message was addressed to, minus the user --
 * a reply that Ccs the sender back to themselves, or copies the user on their
 * own reply, is how a thread turns into duplicates.
 */
async function openComposer(msgId: number, all: boolean): Promise<void> {
  const account = state.accountId;
  const [msg, recipients] = (await Promise.all([
    rpc.call("get_message", [account, msgId]),
    rpc.call("get_message_recipients", [account, msgId]),
  ])) as [Message, Recipient[]];

  const outgoing = msg.fromId === SELF_CONTACT_ID;
  const [sender, self] = (await Promise.all([
    rpc.call("get_contact", [account, msg.fromId]),
    rpc.call("get_contact", [account, SELF_CONTACT_ID]),
  ])) as [Contact, Contact];

  const headerTo = recipients.filter((r) => r.kind === "to").map((r) => r.addr);
  const headerCc = recipients.filter((r) => r.kind === "cc").map((r) => r.addr);

  const to = outgoing
    ? addresses(headerTo, [self.address])
    : addresses(all ? [sender.address, ...headerTo] : [sender.address], [self.address]);
  // Excluding `to` as well, so someone named in both headers is addressed once.
  const cc = all ? addresses(headerCc, [self.address, ...to]) : [];

  const subject = msg.subject?.trim() ?? "";
  state.composerDraft = {
    to: to.join(", "),
    cc: cc.join(", "),
    bcc: "",
    subject: subject.toLowerCase().startsWith("re:") ? subject : `Re: ${subject}`,
    body: `\n\n> ${(msg.text ?? "").split("\n").join("\n> ")}`,
    html: null,
  };
  state.screen = "composer";
  changed();
}

/** Opens the menu for a message and runs whatever was picked. */
export async function showMenuFor(event: MouseEvent, target: Target): Promise<void> {
  event.preventDefault();
  const { openMenu } = await import("./views/menu");
  const act = await openMenu(event.clientX, event.clientY, itemsFor(target));
  if (act === null) return;
  if (await run(act, target.msgId)) await reload();
}
