/**
 * eeemail's desktop UI.
 *
 * Plain DOM, no framework. The app is a sidebar, a message list, a reading pane
 * and four full-width screens; a framework here would be a dependency surface
 * on the process that renders untrusted mail, in exchange for machinery this
 * does not need. See `docs/adr/0013-desktop-ui.md`.
 *
 * It deliberately does not paper over the engine. Encryption state, holding,
 * expiry, undelivered recipients and the at-rest gap are shown as they are,
 * because they are exactly what an encrypted mail client has to be honest
 * about.
 */

import { rpc, isDemo } from "./client";
import { state, changed, onChange, applyHash } from "./state";
import { renderSidebar, refreshHeldCount } from "./views/sidebar";
import { refreshList, renderList } from "./views/list";
import { renderReading } from "./views/reading";
import { renderComposer } from "./views/composer";
import { renderContacts } from "./views/contacts";
import { renderSettings } from "./views/settings";
import { renderSetup } from "./views/setup";
import type { Label } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("missing #app");

async function boot(): Promise<void> {
  await rpc.ready();

  const ids = (await rpc.call("get_all_account_ids")) as number[];
  if (ids.length === 0) {
    state.screen = "setup";
    render();
    return;
  }
  state.accountId = ids[0]!;

  // eeemail's defaults are applied at setup, not as compile-time defaults, so
  // every entry point has to ask for them. See ADR 0012.
  await rpc.call("apply_eeemail_defaults", [state.accountId]);
  state.labels = (await rpc.call("get_labels", [state.accountId])) as Label[];

  // After the labels load, so a `#/label/10` link can resolve its name.
  applyHash();
  window.addEventListener("hashchange", () => {
    if (applyHash()) void reload();
  });

  // New mail arrives pushed, not polled.
  rpc.onEvent(({ event }) => {
    const kind = (event as { kind?: string } | undefined)?.kind;
    if (kind === "IncomingMsg" || kind === "MsgsChanged") {
      void reload();
    }
  });

  await reload();
}

/** Re-reads the current list from the engine, then repaints. */
async function reload(): Promise<void> {
  await Promise.all([refreshList(), refreshHeldCount()]);
  changed();
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
    } catch (err) {
      showError(body, err);
    }
    return;
  }

  app!.className = "three-pane";
  app!.innerHTML = `
    <nav class="sidebar" id="sidebar"></nav>
    <section class="list-pane">
      <div class="search"><input id="search" type="search" placeholder="Search mail" /></div>
      <div class="list" id="list"></div>
    </section>
    <main class="reading" id="reading"></main>
  `;

  renderSidebar(app!.querySelector<HTMLElement>("#sidebar")!);
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
if (isDemo) document.body.classList.add("demo");

boot().catch((err) => {
  app!.className = "single";
  app!.innerHTML = `<main class="screen" id="screen"></main>`;
  showError(app!.querySelector<HTMLElement>("#screen")!, err);
});
