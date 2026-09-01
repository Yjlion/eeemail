import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * A JSON-RPC client over the Tauri IPC pipe.
 *
 * The shell exposes one `rpc_send` command and emits everything coming back --
 * responses and engine events alike -- as `rpc-message`. That is the same shape
 * `deltachat-rpc-server` has over stdio, so the generated TypeScript client
 * from `deltachat-jsonrpc` works here unchanged.
 */

type Pending = {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
};

export type EventHandler = (event: { contextId: number; event: unknown }) => void;

export class Rpc {
  #nextId = 1;
  #pending = new Map<number, Pending>();
  #eventHandlers = new Set<EventHandler>();
  #ready: Promise<void>;

  constructor() {
    this.#ready = listen<string>("rpc-message", (message) => {
      this.#onMessage(message.payload);
    }).then(() => undefined);
  }

  /** Resolves once the event stream is attached. Call before the first request. */
  async ready(): Promise<void> {
    await this.#ready;
  }

  /** Subscribes to engine events. Returns an unsubscribe function. */
  onEvent(handler: EventHandler): () => void {
    this.#eventHandlers.add(handler);
    return () => this.#eventHandlers.delete(handler);
  }

  /** Calls one RPC method. */
  async call(method: string, params: unknown[] = []): Promise<unknown> {
    await this.ready();
    const id = this.#nextId++;
    const request = JSON.stringify({ jsonrpc: "2.0", id, method, params });
    const result = new Promise<unknown>((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
    });
    await invoke("rpc_send", { request });
    return result;
  }

  #onMessage(raw: string): void {
    let message: Record<string, unknown>;
    try {
      message = JSON.parse(raw) as Record<string, unknown>;
    } catch {
      // A malformed frame must not take down the pipe every later message
      // depends on.
      console.error("unparseable RPC message", raw);
      return;
    }

    // A notification has no id: that is how engine events arrive.
    if (message["id"] === undefined || message["id"] === null) {
      if (message["method"] === "event") {
        const params = message["params"] as { contextId: number; event: unknown } | undefined;
        if (params) for (const handler of this.#eventHandlers) handler(params);
      }
      return;
    }

    const pending = this.#pending.get(message["id"] as number);
    if (!pending) return;
    this.#pending.delete(message["id"] as number);

    if (message["error"]) {
      const error = message["error"] as { message?: string };
      pending.reject(new Error(error.message ?? "RPC error"));
    } else {
      pending.resolve(message["result"]);
    }
  }
}

export const rpc = new Rpc();
