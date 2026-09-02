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
