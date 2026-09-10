/**
 * eeemail's desktop UI.
 *
 * Plain DOM, no framework. The app is a sidebar, a message list, a reading pane
 * and four full-width screens; a framework here would be a dependency surface
 * on the process that renders untrusted mail, in exchange for machinery this
 * does not need. See `docs/adr/0013-desktop-ui.md`.
 *
 * It deliberately does not paper over the engine. Encryption state, the
 * unverified view, expiry, undelivered recipients and the at-rest gap are shown
 * as they are, because they are exactly what an encrypted mail client has to be
 * honest about.
 */

import { rpc, isDemo } from "./client";
import { state, changed, onChange, applyHash } from "./state";
import { reload, refreshConnectivity } from "./nav";
import { loadAccounts, prepareAccount } from "./accounts";
import { renderSidebar } from "./views/sidebar";
import { renderList, renderListHeader } from "./views/list";
import { renderReading } from "./views/reading";
import { renderComposer } from "./views/composer";
import { renderContacts } from "./views/contacts";
import { renderSettings } from "./views/settings";
import { renderTags } from "./views/tags";
import { renderSetup } from "./views/setup";
import { showFirstRun } from "./views/firstrun";
import { firstRunPending } from "./shell";
import type { Label } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing #app");

async function boot(): Promise<void> {
  await rpc.ready();

  // Before the account list, and so before the setup form: what this software
  // is has to be said ahead of the screen that asks for a mail password, not
  // after it.
  if (await firstRunPending()) await showFirstRun();

  const ids = (await rpc.call("get_all_account_ids")) as number[];
  if (ids.length === 0) {
    state.setupAccountId = null;
    state.screen = "setup";
    render();
    return;
  }

  // The account the user left open, not whichever one happens to be first.
  // `get_selected_account_id` returns null for a profile that was removed, so
  // the first id stays the fallback rather than the answer.
  const selected = (await rpc.call("get_selected_account_id")) as number | null;
  state.accountId = selected !== null && ids.includes(selected) ? selected : ids[0]!;
  await loadAccounts();

  // eeemail's defaults are applied at setup, not as compile-time defaults, so
  // every entry point has to ask for them (ADR 0012) -- and `start_io` on every
  // boot, because the only other `start_io` in this client is in the setup
  // form, so for eight releases the scheduler ran exactly once, in the session
  // that created the account. Every launch after that had no IMAP loop.
  //
  // For *every* account, not only the visible one: an account nobody is looking
  // at still receives mail. Both calls are no-ops when they have nothing to do.
  // Sequential rather than concurrent -- these open databases, and a failure on
  // one account must not take down the boot of the others, which is what the
  // per-account catch is for.
  for (const id of ids) {
    try {
      await prepareAccount(id);
    } catch (err) {
      console.error(`account ${id} could not be started`, err);
    }
  }
  void refreshConnectivity();

  state.labels = (await rpc.call("get_labels", [state.accountId])) as Label[];

  // After the labels load, so a `#/label/10` link can resolve its name.
  applyHash();
  window.addEventListener("hashchange", () => {
    if (applyHash()) void reload();
  });

  // New mail arrives pushed, not polled. The refresh button is a nudge for when
  // it has not, not the mechanism.
  rpc.onEvent(({ event }) => {
    const kind = (event as { kind?: string } | undefined)?.kind;
    if (kind === "IncomingMsg" || kind === "MsgsChanged") {
      void reload();
    }
    if (kind === "ConnectivityChanged") {
      void refreshConnectivity().then(changed);
    }
    // The account list changes when one is added, removed or reordered, and
    // when a display name or avatar changes on one. Both redraw the switcher.
    if (kind === "AccountsChanged" || kind === "AccountsItemChanged") {
      void loadAccounts().then(changed);
    }
  });

  await reload();
}

let painting = false;
function render(): void {
  // Renders are async and can be triggered by an engine event mid-paint. One at
  // a time, or two passes interleave and the pane ends up showing neither.
  if (painting) return;
  painting = true;
  void paint().finally(() => {
    painting = false;
  });
}

async function paint(): Promise<void> {
  if (state.screen === "setup") {
    app!.className = "single";
    app!.innerHTML = `<main class="screen" id="screen"></main>`;
    renderSetup(app!.querySelector<HTMLElement>("#screen")!);
    return;
  }

  if (state.screen !== null) {
    app!.className = "with-sidebar";
    app!.innerHTML = `
      <nav class="sidebar" id="sidebar"></nav>
      <main class="screen" id="screen"></main>`;
    renderSidebar(app!.querySelector<HTMLElement>("#sidebar")!);
    const screen = app!.querySelector<HTMLElement>("#screen")!;
    // A back affordance on every full screen: the sidebar switches views, but
    // it does not say how to get out of the composer without discarding.
    const close = document.createElement("button");
    close.className = "close";
    close.textContent = "← Back to mail";
    close.addEventListener("click", () => {
      state.screen = null;
      changed();
    });
    screen.append(close);
    const body = document.createElement("div");
    screen.append(body);

    try {
      if (state.screen === "composer") renderComposer(body);
      else if (state.screen === "contacts") await renderContacts(body);
      else if (state.screen === "settings") await renderSettings(body);
      else if (state.screen === "tags") await renderTags(body);
    } catch (err) {
      showError(body, err);
    }
    return;
  }

  app!.className = "three-pane";
  app!.innerHTML = `
    <nav class="sidebar" id="sidebar"></nav>
    <section class="list-pane">
      <div class="pane-head" id="pane-head"></div>
      <div class="list" id="list"></div>
    </section>
    <main class="reading" id="reading"></main>
  `;

  renderSidebar(app!.querySelector<HTMLElement>("#sidebar")!);
  renderListHeader(app!.querySelector<HTMLElement>("#pane-head")!);
  wireSearch(app!.querySelector<HTMLInputElement>("#search")!);

  const list = app!.querySelector<HTMLElement>("#list")!;
  const reading = app!.querySelector<HTMLElement>("#reading")!;
  try {
    await renderList(list);
  } catch (err) {
    showError(list, err);
  }
  try {
    await renderReading(reading);
  } catch (err) {
    showError(reading, err);
  }
}

function wireSearch(input: HTMLInputElement): void {
  if (state.view.kind === "search") input.value = state.view.query;
  let timer: number | undefined;
  input.addEventListener("input", () => {
    window.clearTimeout(timer);
    // Debounced: search runs a LIKE over the mailbox, and a query per keystroke
    // would issue one for every prefix of a word nobody meant to search for.
    timer = window.setTimeout(() => {
      const query = input.value.trim();
      state.view = query ? { kind: "search", query } : { kind: "tag", tag: "inbox" };
      state.selectedMsgId = null;
      void reload();
    }, 200);
  });
}

function showError(target: HTMLElement, err: unknown): void {
  const el = document.createElement("div");
  el.className = "error";
  el.textContent = err instanceof Error ? err.message : String(err);
  target.prepend(el);
}

onChange(render);

// A demo build answers from fixtures and never reaches an account. Saying so on
// screen is cheaper than someone mistaking a screenshot for their mailbox.
//
// Every other build says `PREVIEW`, on the same pattern. The first-launch
// dialog is dismissed once and then gone; the state it describes lasts longer
// than one click, so something has to keep saying it.
document.body.classList.add(isDemo ? "demo" : "preview");

boot().catch((err) => {
  app!.className = "single";
  app!.innerHTML = `<main class="screen" id="screen"></main>`;
  showError(app!.querySelector<HTMLElement>("#screen")!, err);
});
