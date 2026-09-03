/**
 * The few things that need the Tauri shell rather than the RPC pipe.
 *
 * Kept apart from `client.ts` so a demo build never reaches for `invoke`, and
 * so it is obvious how small this surface is: everything else the UI does goes
 * through JSON-RPC. See `docs/adr/0013-desktop-ui.md`.
 */

import { isDemo } from "./client";

/**
 * Turns a picked file into a path the engine can read.
 *
 * A `File` in the renderer has no filesystem path -- the browser security model
 * does not give it one -- and core carries attachments by path. So the bytes
 * cross the IPC boundary once and the shell stages them beside the accounts.
 */
export async function stageAttachment(file: File): Promise<string> {
  if (isDemo) return `/demo/${file.name}`;
  const { invoke } = await import("@tauri-apps/api/core");
  const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
  return (await invoke("stage_attachment", { name: file.name, bytes })) as string;
}

/**
 * Whether the first-launch disclosure still has to be shown.
 *
 * A file beside the accounts, not a config value: the disclosure comes *before*
 * an account exists, and `Config` is per-account. A demo build never shows it,
 * because a demo build is a screenshot of a mailbox nobody has -- except on the
 * `#/first-run` route, which exists so the dialog is reviewable like every other
 * screen.
 */
export async function firstRunPending(): Promise<boolean> {
  if (isDemo) return window.location.hash === "#/first-run";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return (await invoke("first_run_pending")) as boolean;
  } catch {
    // Showing it twice is a papercut; never showing it is the thing this exists
    // to prevent.
    return true;
  }
}

/** Records that the user acknowledged the disclosure. */
export async function acknowledgeFirstRun(): Promise<void> {
  if (isDemo) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("acknowledge_first_run");
}
