#!/usr/bin/env python3
"""The six-step end-to-end pass, run against `server/compose`.

Everything eeemail ships is unit-tested; until this script existed, none of it
had ever spoken to a real IMAP or SMTP server. This is the difference between
"tests pass" and "this works".

It drives `deltachat-rpc-server`, which speaks JSON Lines over stdio, because
`cli/` deliberately cannot: it is one-shot with no daemon, so it never starts
core's IO loop and can neither send nor receive.

Stdlib only, like `server/compose/smoke-test.py`. Bring the server up first:

    cd server/compose && docker compose up -d --build && python3 smoke-test.py
    python3 scripts/e2e-pass.py
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile

from dcrpc import (
    ARRIVAL_TIMEOUT,
    DOMAIN,
    Failure,
    Rpc,
    check,
    transport as login_params,
    wait_for,
)

ACCOUNTS = {"alice": "alicepw", "bob": "bobpw"}

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SERVER_BIN = os.path.join(REPO, "core", "target", "debug", "deltachat-rpc-server")


def transport(user: str) -> dict:
    return login_params(user, ACCOUNTS)


# ---------------------------------------------------------------------------
# Step 1 -- account setup
# ---------------------------------------------------------------------------

def step1_setup(rpc: Rpc, user: str) -> int:
    account_id = rpc.call("add_account")

    # Before `configure`, not after. `policy::apply_defaults` early-returns on
    # `is_configured()` and is documented never to touch a configured account,
    # so calling it later is a silent no-op: gating and recoverable expiry
    # would never turn on and step 3 would fail pointing at the wrong module.
    rpc.call("apply_eeemail_defaults", account_id)

    rpc.call("add_or_update_transport", account_id, transport(user))
    check(rpc.call("is_configured", account_id), f"{user}: configured against the test server")

    rpc.call("start_io", account_id)

    # The defaults are eeemail's whole behavioural difference from upstream.
    # Asserting them here is what makes the ordering above load-bearing rather
    # than a comment nobody checks.
    check(rpc.call("get_inbox_gating", account_id) is True,
          f"{user}: inbox gating on by default")
    check(rpc.call("get_trash_purge_days", account_id) == 30,
          f"{user}: recoverable expiry defaults to 30 days")
    check(rpc.call("get_encryption_mode", account_id) == "opportunistic",
          f"{user}: encryption is opportunistic by default")
    return account_id


# ---------------------------------------------------------------------------
# Step 2 -- a Cc'd message over the wire
# ---------------------------------------------------------------------------

SUBJECT = "Thursday's numbers"
BODY = "The totals look like they include the reversed entries from March."


def step2_send_cc(rpc: Rpc, alice: int, bob: int, workdir: str) -> int:
    """alice mails bob with a Cc to carol, and one attachment."""
    attachment = os.path.join(workdir, "numbers.txt")
    with open(attachment, "w") as handle:
        handle.write("march,-120\napril,340\n")

    rpc.call("send_email", alice,
             {"to": [f"bob@{DOMAIN}"], "cc": [f"carol@{DOMAIN}"], "bcc": []},
             SUBJECT, BODY, attachment)

    # Held, not delivered: alice is neither verified nor known to bob, which is
    # step 3's subject. Here it is only how we find the message.
    held = wait_for(lambda: rpc.call("get_tagged_messages", bob, "holding"),
                    "bob to receive alice's message")
    msg_id = held[0]

    row = rpc.call("get_message_rows", bob, [msg_id])[0]
    check(row["subject"] == SUBJECT, "subject survives the round trip",
          f"got {row['subject']!r}")

    recipients = rpc.call("get_message_recipients", bob, msg_id)
    to = sorted(r["addr"] for r in recipients if r["kind"] == "to")
    cc = sorted(r["addr"] for r in recipients if r["kind"] == "cc")
    check(to == [f"bob@{DOMAIN}"], "To: survives the round trip", f"got {to}")
    # The reason this pass needs three mailboxes. Upstream carries no Cc at
    # all; ADR 0008 and Phase 9 are what put it on the wire.
    check(cc == [f"carol@{DOMAIN}"], "Cc: survives the round trip", f"got {cc}")

    check(row["hasAttachment"], "the attachment arrived")

    # Opportunistic means cleartext until a key is known, and eeemail's job is
    # to say so rather than imply protection it did not have.
    crypto = rpc.call("get_message_crypto", bob, msg_id)
    check(crypto["encrypted"] is False,
          "first contact is cleartext, and reported as cleartext")
    return msg_id


def step2b_reply_encrypts(rpc: Rpc, alice: int, bob: int) -> None:
    """bob replies; alice's Autocrypt header means it goes out encrypted."""
    rpc.call("send_email", bob, {"to": [f"alice@{DOMAIN}"], "cc": [], "bcc": []},
             f"Re: {SUBJECT}", "Checked -- you are right.", None)

    def arrived():
        ids = rpc.call("get_tagged_messages", alice, "inbox")
        for candidate in ids:
            row = rpc.call("get_message_rows", alice, [candidate])[0]
            if row["subject"].startswith("Re: "):
                return candidate
        return None

    reply_id = wait_for(arrived, "alice to receive bob's reply")
    crypto = rpc.call("get_message_crypto", alice, reply_id)
    # This is Autocrypt working end to end: bob learned alice's key from the
    # header on the message he was sent, with nobody choosing to encrypt.
    check(crypto["encrypted"] is True, "the reply is encrypted, having learned the key")


# ---------------------------------------------------------------------------
# Step 3 -- held mail reaches the inbox when the sender is accepted
# ---------------------------------------------------------------------------

def step3_release(rpc: Rpc, bob: int, msg_id: int) -> None:
    tags = rpc.call("get_message_tags", bob, msg_id)
    check("holding" in tags["system"], "a stranger's mail is held",
          f"tags were {tags['system']}")
    check("inbox" not in tags["system"], "and is kept out of the inbox")

    message = rpc.call("get_message", bob, msg_id)
    check(bool(message.get("text")), "held mail is readable, not merely quarantined")

    # Accepting the sender, not the message: `contact.rs` scales the origin up
    # and calls `gating::release`, so past and future mail move together.
    rpc.call("accept_chat", bob, message["chatId"])

    def released():
        tags = rpc.call("get_message_tags", bob, msg_id)
        return "holding" not in tags["system"]

    wait_for(released, "held mail to be released", timeout=30)
    tags = rpc.call("get_message_tags", bob, msg_id)
    check("inbox" in tags["system"], "accepting the sender moves it to the inbox",
          f"tags were {tags['system']}")
    check(rpc.call("get_tagged_messages", bob, "holding") == [],
          "and nothing is left holding")


# ---------------------------------------------------------------------------
# Step 3b -- SecureJoin, the one path that does produce encryption
# ---------------------------------------------------------------------------

def step3b_securejoin(rpc: Rpc, alice: int, bob: int) -> None:
    """bob scans alice's setup-contact QR; afterwards their mail encrypts.

    This is also what pins down the finding in step 2b. Encryption in core
    v2.59 follows the *contact type*, not key availability: a `Single` chat is
    encrypted only when its contact row carries a fingerprint (`chat.rs:1690`).
    SecureJoin mints exactly that, so it works -- and nothing else eeemail does
    today ever will.
    """
    qr = rpc.call("get_chat_securejoin_qr_code", alice, None)
    rpc.call("secure_join", bob, qr, timeout=180)

    def verified():
        for contact_id in rpc.call("get_contact_ids", bob, 0, f"alice@{DOMAIN}"):
            contact = rpc.call("get_contact", bob, contact_id)
            if contact["isKeyContact"] and contact["isVerified"]:
                return contact_id
        return None

    contact_id = wait_for(verified, "SecureJoin to complete", timeout=180)
    check(True, "SecureJoin completes and alice becomes a verified key contact")

    rpc.call("send_email", bob, {"to": [f"alice@{DOMAIN}"], "cc": [], "bcc": []},
             "Verified now", "This one should be encrypted.", None)

    def arrived():
        for candidate in rpc.call("get_tagged_messages", alice, "inbox"):
            row = rpc.call("get_message_rows", alice, [candidate])[0]
            if row["subject"] == "Verified now":
                return candidate
        return None

    msg_id = wait_for(arrived, "alice to receive the post-verification message")
    crypto = rpc.call("get_message_crypto", alice, msg_id)
    check(crypto["encrypted"] is True, "mail to a verified contact is encrypted")
    check(crypto["verified"] is True, "and is reported as verified")


# ---------------------------------------------------------------------------
# Step 4 -- a timer fires, and the message survives it
# ---------------------------------------------------------------------------

EXPIRY_SECS = 60


def step4_expiry(rpc: Rpc, bob: int, msg_id: int) -> None:
    rpc.call("set_message_ephemeral_timer", bob, msg_id, EXPIRY_SECS)
    check(rpc.call("get_message_ephemeral_timer", bob, msg_id) is not None,
          "a per-message expiry timer is set")

    def expired():
        tags = rpc.call("get_message_tags", bob, msg_id)
        return "trash" in tags["system"]

    # ADR 0019 exists because upstream destroys the message here. Waiting for
    # core's own `ephemeral_loop` rather than calling anything ourselves is the
    # point: this asserts the real expiry path, not a simulation of it.
    wait_for(expired, "the timer to fire", timeout=EXPIRY_SECS + 120, interval=3)
    check(True, "an expired message moves to Trash")

    trashed = rpc.call("get_trashed_message", bob, msg_id)
    check(trashed is not None and trashed["reason"] == "expired",
          "and is recorded as expired rather than deleted")

    message = rpc.call("get_message", bob, msg_id)
    check(bool(message.get("text")), "and is still readable, not destroyed")

    rpc.call("restore_messages", bob, [msg_id])
    tags = rpc.call("get_message_tags", bob, msg_id)
    check("trash" not in tags["system"], "and can be restored",
          f"tags were {tags['system']}")


# ---------------------------------------------------------------------------
# Step 5 -- encryption at rest
# ---------------------------------------------------------------------------

def step5_at_rest(rpc: Rpc, account_id: int) -> None:
    # Refusing is the feature: a blob key stored beside a cleartext database
    # protects nothing, so `blobcrypt::enable` bails rather than pretending.
    try:
        rpc.call("enable_blob_encryption", account_id)
        raise Failure("blob encryption was enabled without a database passphrase")
    except Failure as err:
        if "was enabled without" in str(err):
            raise
    check(True, "blob encryption refuses to run on a cleartext database")

    rpc.call("set_database_passphrase", account_id, "correct horse battery staple")
    protection = rpc.call("get_at_rest_protection", account_id)
    check(protection["databaseEncrypted"] is True, "the database is encrypted")
    check(protection["blobsEncrypted"] is False,
          "and the blobdir is honestly reported as still cleartext")

    rpc.call("enable_blob_encryption", account_id, timeout=300)
    protection = rpc.call("get_at_rest_protection", account_id)
    check(protection["blobsEncrypted"] is True, "the blobdir is encrypted")
    check(protection["partial"] is False, "and the migration ran to completion")
    check(bool(protection["summary"]), "at-rest protection reports a summary verbatim")


# ---------------------------------------------------------------------------
# Step 6 -- screenshots
# ---------------------------------------------------------------------------

def step6_screenshots() -> None:
    """Regenerates the screenshots twice and checks the bytes match.

    Touches no mail server: they render from `desktop/src/fixtures.ts`, never
    from a mailbox, which is what makes a change in the images a change in the
    UI -- but only if the same UI always produces the same bytes.

    Compared run against run, deliberately not against `git`. Whether the
    images differ from the last commit says whether someone changed the
    interface, which is a different question and a legitimate answer.
    """
    script = os.path.join(REPO, "scripts", "screenshots.sh")

    def render(label: str) -> dict[str, str]:
        result = subprocess.run([script], cwd=REPO, capture_output=True, text=True)
        check(result.returncode == 0, label, result.stderr.strip()[-400:])
        shots = {}
        out = os.path.join(REPO, "screenshots")
        for name in sorted(os.listdir(out)):
            if name.endswith(".png"):
                with open(os.path.join(out, name), "rb") as handle:
                    shots[name] = hashlib.sha256(handle.read()).hexdigest()
        return shots

    first = render("screenshots regenerate")
    second = render("and regenerate a second time")
    moved = sorted(name for name in first if first[name] != second.get(name))
    check(not moved and set(first) == set(second),
          "and are byte-stable across runs",
          f"differed between runs: {moved or 'the set of images changed'}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true",
                        help="leave the accounts directory behind for inspection")
    parser.add_argument("--log", metavar="RUST_LOG",
                        help="pass RUST_LOG to the server and let it write to stderr")
    args = parser.parse_args()

    if not os.path.exists(SERVER_BIN):
        print(f"missing {SERVER_BIN}\n"
              f"build it with: cd core && cargo build -p deltachat-rpc-server",
              file=sys.stderr)
        return 2

    accounts_dir = tempfile.mkdtemp(prefix="eeemail-e2e-")
    rpc = Rpc(SERVER_BIN, accounts_dir, log=args.log)
    failures: list[str] = []

    def run(label: str, fn, *fn_args):
        """Runs one step, and keeps going if it fails.

        A pass that stops at the first failure only ever tells you about one
        thing. The steps are independent enough that the rest are still worth
        knowing about, and a step that genuinely cannot proceed says so.
        """
        print(label)
        try:
            return fn(*fn_args)
        except Failure as err:
            print(f"  FAIL  {err}", file=sys.stderr)
            failures.append(f"{label}: {err}")
            return None

    try:
        alice = run("step 1: account setup", step1_setup, rpc, "alice")
        bob = run("step 1: account setup (bob)", step1_setup, rpc, "bob")
        if alice is None or bob is None:
            raise Failure("no usable accounts; the remaining steps cannot run")

        msg_id = run("step 2: a Cc'd message over the wire",
                     step2_send_cc, rpc, alice, bob, accounts_dir)

        # Before the reply, and the ordering is the assertion. Writing to
        # someone makes them known (`gating.rs`: "writing to someone is enough
        # to want their reply in your inbox"), which releases their held mail --
        # so replying first would quietly dismantle what step 3 checks.
        if msg_id is not None:
            run("step 3: held mail reaches the inbox on acceptance",
                step3_release, rpc, bob, msg_id)

        run("step 2b: the reply encrypts, having learned the key",
            step2b_reply_encrypts, rpc, alice, bob)
        run("step 3b: SecureJoin", step3b_securejoin, rpc, alice, bob)

        if msg_id is not None:
            run("step 4: a timer fires and the message survives it",
                step4_expiry, rpc, bob, msg_id)

        run("step 5: encryption at rest", step5_at_rest, rpc, bob)
        run("step 6: screenshots", step6_screenshots)
    except Failure as err:
        print(f"  FAIL  {err}", file=sys.stderr)
        failures.append(str(err))
    finally:
        rpc.close()
        if args.keep:
            print(f"accounts left in {accounts_dir}", file=sys.stderr)
        else:
            shutil.rmtree(accounts_dir, ignore_errors=True)

    print()
    if failures:
        print(f"{len(failures)} FAILED:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("ALL STEPS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
