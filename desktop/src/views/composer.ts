/**
 * The composer.
 *
 * One `send_email` call rather than four separate ones, because the order of
 * those four matters: the recipient set has to reach the database before the
 * message is sent, or every Bcc is silently dropped. Putting the sequence in
 * the engine is what stops a UI getting it wrong.
 * See `docs/adr/0014-recipient-sets-on-the-wire.md`.
 */

import { rpc } from "../client";
import { state, changed } from "../state";
import { escapeHtml } from "../html";
import { stageAttachment } from "../shell";
import type { RecipientSet } from "../types";

/** Splits a comma-separated address field, dropping empties. */
function addresses(value: string): string[] {
  return value
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

export function renderComposer(el: HTMLElement): void {
  const draft = state.composerDraft ?? { to: "", cc: "", bcc: "", subject: "", body: "" };

  el.innerHTML = `
    <form class="composer" id="composer">
      <h1>New message</h1>
      <label>To <input name="to" value="${escapeHtml(draft.to)}" autocomplete="off" required /></label>
      <label>Cc <input name="cc" value="${escapeHtml(draft.cc)}" autocomplete="off" /></label>
      <label>Bcc <input name="bcc" value="${escapeHtml(draft.bcc)}" autocomplete="off" /></label>
      <label>Subject <input name="subject" value="${escapeHtml(draft.subject)}" autocomplete="off" /></label>
      <label class="attach">Attachment
        <input name="attachment" type="file" />
        <span class="hint">One file per message: the engine carries one, and
        pretending otherwise here would move the surprise further from where you
        chose it.</span>
      </label>
      <textarea name="body" rows="16" placeholder="Write your message">${escapeHtml(draft.body)}</textarea>
      <div class="notice" id="crypto-note">Checking who has a key…</div>
      <div class="actions">
        <button type="submit">Send</button>
        <button type="button" data-act="cancel">Discard</button>
      </div>
      <div class="error" id="composer-error" hidden></div>
    </form>
  `;

  const form = el.querySelector<HTMLFormElement>("#composer")!;
  const note = el.querySelector<HTMLElement>("#crypto-note")!;
  const error = el.querySelector<HTMLElement>("#composer-error")!;

  const readSet = (): RecipientSet => ({
    to: addresses((form.elements.namedItem("to") as HTMLInputElement).value),
    cc: addresses((form.elements.namedItem("cc") as HTMLInputElement).value),
    bcc: addresses((form.elements.namedItem("bcc") as HTMLInputElement).value),
  });

  // Whether this will go out encrypted is the single most important thing about
  // a message, and the one thing the user cannot see once it is gone. So it is
  // computed while they type rather than reported afterwards.
  const updateCryptoNote = async () => {
    const set = readSet();
    const all = [...set.to, ...set.cc, ...set.bcc];
    if (all.length === 0) {
      note.className = "notice";
      note.textContent = "Add a recipient.";
      return;
    }
    try {
      const contacts = (await rpc.call("get_contacts", [
        state.accountId,
        0,
        null,
      ])) as { address: string; isVerified: boolean }[];
      const known = new Set(contacts.map((c) => c.address.toLowerCase()));
      const missing = all.filter((a) => !known.has(bareAddress(a).toLowerCase()));
      if (missing.length === 0) {
        note.className = "notice good";
        note.textContent = "Everyone here has a key. This will be end-to-end encrypted.";
      } else {
        note.className = "notice warn";
        note.textContent =
          `No key yet for ${missing.join(", ")}. Under the default opportunistic ` +
          `policy this message goes out unencrypted to them, and they are told so.`;
      }
    } catch {
      note.className = "notice";
      note.textContent = "";
    }
  };

  for (const field of ["to", "cc", "bcc"]) {
    (form.elements.namedItem(field) as HTMLInputElement).addEventListener(
      "input",
      () => void updateCryptoNote(),
    );
  }
  void updateCryptoNote();

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const set = readSet();
    if (set.to.length === 0) {
      error.hidden = false;
      // Refused rather than guessed at: a message with only Cc or Bcc has no
      // visible addressee and no conversation to belong to.
      error.textContent = "A message needs at least one To: address.";
      return;
    }

    const file = (form.elements.namedItem("attachment") as HTMLInputElement).files?.[0];
    const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
    submit.disabled = true;
    submit.textContent = "Sending…";

    try {
      await rpc.call("send_email", [
        state.accountId,
        set,
        (form.elements.namedItem("subject") as HTMLInputElement).value,
        (form.elements.namedItem("body") as HTMLTextAreaElement).value,
        // A File in the renderer has no filesystem path, so the shell stages
        // the bytes and hands back one.
        file ? await stageAttachment(file) : null,
      ]);
      state.composerDraft = null;
      state.screen = null;
      state.view = { kind: "tag", tag: "sent" };
      changed();
    } catch (err) {
      error.hidden = false;
      error.textContent = err instanceof Error ? err.message : String(err);
      submit.disabled = false;
      submit.textContent = "Send";
    }
  });

  form.querySelector<HTMLButtonElement>("button[data-act='cancel']")!.addEventListener(
    "click",
    () => {
      state.composerDraft = null;
      state.screen = null;
      changed();
    },
  );
}

function bareAddress(input: string): string {
  const match = /<([^>]+)>/.exec(input);
  return (match?.[1] ?? input).trim();
}
