/**
 * The one RPC handle the UI uses.
 *
 * In a normal build this is the real JSON-RPC pipe over Tauri IPC. In a demo
 * build (`VITE_EEEMAIL_DEMO=1`) it is `fixtures.ts`, which answers the same
 * methods from canned data so the UI can be developed and screenshotted
 * without a mailbox.
 *
 * The choice is made here, once, so no view has to know which one it is
 * talking to -- and so a demo build cannot accidentally reach a real account.
 */

import { Rpc, type EventHandler } from "./rpc";

export interface RpcLike {
  ready(): Promise<void>;
  onEvent(handler: EventHandler): () => void;
  call(method: string, params?: unknown[]): Promise<unknown>;
}

export const isDemo = import.meta.env["VITE_EEEMAIL_DEMO"] === "1";

async function makeClient(): Promise<RpcLike> {
  if (isDemo) {
    const { DemoRpc } = await import("./fixtures");
    return new DemoRpc();
  }
  return new Rpc();
}

export const rpc: RpcLike = await makeClient();
