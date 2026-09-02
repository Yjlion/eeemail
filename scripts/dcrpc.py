"""A JSON-RPC client for `deltachat-rpc-server`, shared by the live passes.

Two scripts drive this protocol: `e2e-pass.py`, which runs eeemail against
itself, and `interop-pass.py`, which runs eeemail against upstream's released
binary. What they share is not test logic -- it is a wire client, with a reader
thread, a condition-variable reply table, and a background event drain that
exists so core never blocks on a full channel. A framing bug fixed in one copy
of that and not the other produces a green run that is testing nothing, which
is the one failure mode a pass must not have.

The account tables and the steps stay in the scripts. Only the protocol, and
the one timeout that is a property of the transport rather than of a test,
live here.

Stdlib only, like everything else under `scripts/` and `server/compose/`.
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
import time
from typing import Any

HOST = "127.0.0.1"
IMAP_PORT = 2143
SMTP_PORT = 2587
DOMAIN = "eeemail.test"

# Long enough for a STARTTLS round trip plus Postfix's own queue run on a
# loaded machine; short enough that a genuine hang is still a test failure.
ARRIVAL_TIMEOUT = 90


class Failure(Exception):
    """A step did not do what eeemail claims it does."""


class Rpc:
    """A `deltachat-rpc-server` subprocess, spoken to in JSON-RPC 2.0.

    `binary` is a parameter rather than a module constant because the interop
    pass runs two different builds of this server -- ours and upstream's -- in
    one process, and telling them apart is the whole point of that script.
    """

    def __init__(self, binary: str, accounts_dir: str, log: str | None = None) -> None:
        self.binary = binary
        env = dict(os.environ, DC_ACCOUNTS_PATH=accounts_dir)
        if log:
            env["RUST_LOG"] = log
        self.proc = subprocess.Popen(
            [binary],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None if log else subprocess.DEVNULL,
            env=env,
            text=True,
            bufsize=1,
        )
        self._id = 0
        self._lock = threading.Lock()
        self._replies: dict[int, Any] = {}
        self._ready = threading.Condition(self._lock)
        self.events: list[dict] = []
        self._stop = False
        threading.Thread(target=self._read_loop, daemon=True).start()
        threading.Thread(target=self._event_loop, daemon=True).start()

    def _read_loop(self) -> None:
        assert self.proc.stdout
        for line in self.proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if "id" in msg and ("result" in msg or "error" in msg):
                with self._ready:
                    self._replies[msg["id"]] = msg
                    self._ready.notify_all()

    def _event_loop(self) -> None:
        """Drains the event queue so nothing blocks on a full channel."""
        while not self._stop:
            try:
                event = self.call("get_next_event", timeout=5)
            except Failure:
                continue
            except Exception:
                return
            with self._lock:
                self.events.append(event)

    def _request(self, method: str, params: tuple, timeout: int) -> dict:
        """Sends one request and returns the whole reply, error or result."""
        with self._lock:
            self._id += 1
            req_id = self._id
        payload = {
            "jsonrpc": "2.0",
            "id": req_id,
            "method": method,
            "params": list(params),
        }
        assert self.proc.stdin
        self.proc.stdin.write(json.dumps(payload) + "\n")
        self.proc.stdin.flush()

        deadline = time.time() + timeout
        with self._ready:
            while req_id not in self._replies:
                if not self._ready.wait(timeout=max(0.1, deadline - time.time())):
                    if time.time() > deadline:
                        raise Failure(f"{method} timed out after {timeout}s")
            return self._replies.pop(req_id)

    def call(self, method: str, *params: Any, timeout: int = 180) -> Any:
        msg = self._request(method, params, timeout)
        if "error" in msg:
            raise Failure(f"{method} failed: {json.dumps(msg['error'])}")
        return msg["result"]

    def call_expecting_error(self, method: str, *params: Any, timeout: int = 30) -> dict:
        """Like `call`, but returns the JSON-RPC error object instead of raising.

        The interop pass needs to assert that a method is *absent* from the far
        end, which is a success there and a failure everywhere else.
        """
        msg = self._request(method, params, timeout)
        if "error" not in msg:
            raise Failure(f"{method} unexpectedly succeeded: {json.dumps(msg.get('result'))}")
        return msg["error"]

    def close(self) -> None:
        self._stop = True
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
            self.proc.wait(timeout=10)
        except Exception:
            self.proc.kill()


def transport(user: str, accounts: dict[str, str]) -> dict:
    """Login parameters for one of the compose server's accounts.

    Certificate checks are relaxed because `entrypoint.sh` regenerates a
    self-signed cert on every start (`server/README.md`). Without this,
    configure fails in a way that reads like a network problem.

    The account table is the caller's, because the two passes deliberately do
    not share mailboxes.
    """
    addr = f"{user}@{DOMAIN}"
    return {
        "addr": addr,
        "password": accounts[user],
        "imapServer": HOST,
        "imapPort": IMAP_PORT,
        "imapSecurity": "starttls",
        "imapUser": addr,
        "smtpServer": HOST,
        "smtpPort": SMTP_PORT,
        "smtpSecurity": "starttls",
        "smtpUser": addr,
        "smtpPassword": accounts[user],
        "certificateChecks": "acceptInvalidCertificates",
    }


def check(ok: bool, label: str, detail: str = "") -> None:
    if ok:
        print(f"  PASS  {label}")
    else:
        raise Failure(f"{label}{': ' + detail if detail else ''}")


def wait_for(predicate, what: str, timeout: int = ARRIVAL_TIMEOUT, interval: float = 1.0):
    """Polls until `predicate` returns something truthy, or gives up."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    raise Failure(f"timed out after {timeout}s waiting for {what}")
