#!/usr/bin/env python3
"""eeemail against a real Delta Chat engine, over `server/compose`.

`scripts/e2e-pass.py` proves eeemail works. It cannot prove eeemail
*interoperates*, because both of its endpoints are this fork: a wire-format
mistake shared by both ends passes green, and its SecureJoin runs the same core
on both sides of the handshake.

This pass drives two different codebases. One end is our
`core/target/debug/deltachat-rpc-server`. The other is upstream's released
`deltachat-rpc-server-x86_64-linux`, downloaded at a pinned tag and checked
against a recorded hash. That binary is not a stand-in for Delta Chat: the same
GitHub release publishes `deltachat-stdio-rpc-server-linux-x64-<v>.tgz`, which
is the npm package Delta Chat Desktop installs. Driving it is driving Delta
Chat's engine. What stays untested against a Delta Chat client is its UI.

The centrepiece is step 2. [ADR 0021](../docs/adr/0021-autocrypt-key-contacts.md)
has eeemail mint a key-contact from an `Autocrypt:` header, which upstream
v2.59 deliberately removed -- it decides encryption by contact type, and gives a
contact a fingerprint only from an OpenPGP signature (`receive_imf.rs:588`) or
SecureJoin. Nothing until now showed that our divergence produces mail a stock
client can actually read. Step 2 walks that bootstrap hop by hop.

The two ends do not share a vocabulary. Ours has 60 JSON-RPC methods upstream
does not (`apply_eeemail_defaults`, `send_email`, `get_tagged_messages`,
`get_message_crypto`, `get_message_tags`, `get_message_thread`, the label and
at-rest families, ...) and removes none, so the upstream side here is restricted
to stock calls: `create_contact`, `create_chat_by_contact_id`, `accept_chat`,
`misc_send_text_message`, `send_msg`, `get_chatlist_entries`, `get_message_ids`,
`get_message`, `get_contact`, `get_chat_securejoin_qr_code`, `secure_join`.
Encryption on that side is read off `MessageObject.showPadlock`, which is
`Param::GuaranteeE2ee` -- encrypted *and* signed, the same predicate our own
`get_message_crypto` uses, so the two ends' verdicts are comparable.

Stdlib only, like `e2e-pass.py` and `server/compose/smoke-test.py`. The server
must be up **from the compose file**, not the bare `docker run` recipe, which
provisions only alice and bob:

    cd server/compose && docker compose up -d --build && python3 smoke-test.py
    cd core && cargo build -p deltachat-rpc-server && cd ..
    python3 scripts/interop-pass.py

Offline, or when the release binary will not run on this host: the subtree
carries upstream's full history, so a pristine upstream build needs no network.

    git worktree add /tmp/upstream-v2.59.0 $(cat docs/fork-base)
    cd /tmp/upstream-v2.59.0 && cargo build -p deltachat-rpc-server
    python3 scripts/interop-pass.py \\
        --upstream-binary /tmp/upstream-v2.59.0/target/debug/deltachat-rpc-server
"""

from __future__ import annotations

import argparse
import email
import hashlib
import imaplib
import os
import platform
import secrets
import shutil
import ssl
import subprocess
import sys
import tempfile
import urllib.request

from dcrpc import (
    ARRIVAL_TIMEOUT,
    DOMAIN,
    HOST,
    IMAP_PORT,
    Failure,
    Rpc,
    check,
    transport as login_params,
    wait_for,
)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EEEMAIL_BIN = os.path.join(REPO, "core", "target", "debug", "deltachat-rpc-server")
PIN_FILE = os.path.join(REPO, "docs", "interop-upstream")
CACHE_DIR = os.path.join(REPO, "core", "target", "interop")
RELEASE_URL = ("https://github.com/chatmail/core/releases/download"
               "/{tag}/deltachat-rpc-server-x86_64-linux")

# Two independent pairs. Within each pair the earlier name runs eeemail and the
# later runs upstream -- mixing that up costs an hour, so it is a convention
# rather than a lookup. carol is nobody's client; she is read over IMAP, because
# a Cc reaching a third party is only checkable at a mailbox.
ACCOUNTS = {
    "dana": "danapw", "erin": "erinpw",
    "frank": "frankpw", "grace": "gracepw",
    "carol": "carolpw",
}
# These mailboxes are shared with e2e-pass.py and with every previous interop
# run. A fixed subject would happily match a leftover.
TOKEN = secrets.token_hex(4)
SUBJECT = f"[{TOKEN}] Thursday's numbers"
BODY = "The totals look like they include the reversed entries from March."

# Each message the stock side sends carries a phrase we match on. Matching by
# elimination ("any subject that is not ours") silently matches core's own
# encrypted BccSelf sync messages, which do land in the inbox and would make
# step 2b report that a stock client encrypted a reply it had not yet sent.
REPLY_CLEARTEXT = "Checked -- you are right."
REPLY_ENCRYPTED = "Encrypted both ways now."
REPLY_ATTACHMENT = "Ledger attached."

SECUREJOIN_TIMEOUT = 240


# ---------------------------------------------------------------------------
# The upstream binary
# ---------------------------------------------------------------------------

def read_pin() -> tuple[str, str]:
    """The pinned upstream tag and the SHA-256 we expect of its binary.

    Pinned to the tag in `docs/fork-base` rather than to the newest release: a
    failure against the fork point is unambiguously ours, while a failure
    against a newer release could be ours or could be upstream drift we have
    simply not merged yet. That second question belongs to the merge, and
    `docs/fork-patches.md` already commits to re-running this pass there.
    """
    with open(PIN_FILE) as handle:
        for line in handle:
            line = line.split("#", 1)[0].strip()
            if line:
                tag, digest = line.split()
                return tag, digest
    raise Failure(f"{PIN_FILE} has no pin in it")


def fetch_upstream(tag: str, expected: str) -> str:
    """Downloads the pinned release binary, or returns the cached one.

    Upstream publishes no checksum file, so the recorded hash buys
    tamper-detection across runs and machines rather than provenance. Download
    goes to `.part` and is renamed only once it verifies, so an interrupted
    fetch never leaves a half-binary that looks cached.
    """
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise Failure(f"no released binary for {platform.system()}/{platform.machine()}; "
                      f"use --upstream-binary (see the module docstring)")

    os.makedirs(CACHE_DIR, exist_ok=True)
    path = os.path.join(CACHE_DIR, f"deltachat-rpc-server-{tag}")
    if os.path.exists(path):
        with open(path, "rb") as handle:
            if hashlib.sha256(handle.read()).hexdigest() == expected:
                return path
        os.remove(path)

    url = RELEASE_URL.format(tag=tag)
    print(f"  fetching {url}")
    partial = path + ".part"
    with urllib.request.urlopen(url, timeout=300) as response, open(partial, "wb") as handle:
        shutil.copyfileobj(response, handle)
    with open(partial, "rb") as handle:
        digest = hashlib.sha256(handle.read()).hexdigest()
    if digest != expected:
        os.remove(partial)
        raise Failure(f"{tag} binary hashed {digest}, expected {expected}\n"
                      f"    if upstream re-cut the release, update {PIN_FILE}")
    os.chmod(partial, 0o755)
    os.replace(partial, path)
    return path


def version_of(binary: str) -> str:
    """Runs the binary before trusting it.

    A prebuilt binary's one real failure mode on a foreign host is a libc
    mismatch, and this turns that into a legible error rather than a hang. It
    also means the run report says which two engines it actually ran, without
    which the report is not evidence.
    """
    try:
        done = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=30)
    except OSError as err:
        raise Failure(f"{binary} will not execute: {err}") from err
    if done.returncode != 0:
        raise Failure(f"{binary} --version failed: {done.stderr.strip()}")
    # Written to stderr, by both builds.
    return (done.stdout.strip() or done.stderr.strip())


# ---------------------------------------------------------------------------
# Preflight
# ---------------------------------------------------------------------------

def preflight_mailboxes() -> None:
    """Logs into every mailbox this pass needs, before anything else runs.

    The compose file provisions seven accounts; the bare `docker run` recipe in
    server/README.md, and CI's `mail-server` job, pass no ACCOUNTS and so get
    entrypoint.sh's fallback of alice and bob only. Without this check that
    shows up as a configure failure deep in step 1, which reads like a network
    problem -- the same trap `transport()` warns about for certificates.
    """
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    missing = []
    for user, password in sorted(ACCOUNTS.items()):
        try:
            with imaplib.IMAP4(HOST, IMAP_PORT) as imap:
                imap.starttls(context)
                imap.login(f"{user}@{DOMAIN}", password)
        except Exception:
            missing.append(user)
    if missing:
        raise Failure(
            f"not provisioned: {', '.join(m + '@' + DOMAIN for m in missing)}\n"
            f"    bring the server up with server/compose/docker-compose.yml, not the\n"
            f"    bare `docker run` recipe -- see server/README.md")
    check(True, f"all {len(ACCOUNTS)} mailboxes accept a login")


# ---------------------------------------------------------------------------
# Helpers for talking to each side
# ---------------------------------------------------------------------------

def stock_messages(rpc: Rpc, account_id: int) -> list[dict]:
    """Every real message a stock account can see, newest chat first.

    Walks the chatlist rather than `get_next_msgs`, whose cursor is advanced by
    reading and which upstream documents as able to return an id before the
    message has finished downloading.
    """
    out = []
    for chat_id in rpc.call("get_chatlist_entries", account_id, None, None, None):
        for msg_id in rpc.call("get_message_ids", account_id, chat_id, False, False):
            message = rpc.call("get_message", account_id, msg_id)
            # SecureJoin injects several info messages per handshake, and a
            # message still downloading has no body to assert on yet.
            if message.get("isInfo"):
                continue
            if str(message.get("downloadState", "Done")).lower() != "done":
                continue
            out.append(message)
    return out


def wait_stock(rpc: Rpc, account_id: int, predicate, what: str,
               timeout: int = ARRIVAL_TIMEOUT) -> dict:
    return wait_for(
        lambda: next((m for m in stock_messages(rpc, account_id) if predicate(m)), None),
        what, timeout=timeout)


def wait_eeemail(rpc: Rpc, account_id: int, predicate, what: str,
                 timeout: int = ARRIVAL_TIMEOUT) -> int:
    def look():
        for msg_id in rpc.call("get_tagged_messages", account_id, "inbox"):
            row = rpc.call("get_message_rows", account_id, [msg_id])[0]
            if predicate(row):
                return msg_id
        return None
    return wait_for(look, what, timeout=timeout)


def stock_contact(rpc: Rpc, account_id: int, addr: str, want_key: bool) -> dict | None:
    """The stock side's contact row for `addr`, preferring a key-contact.

    The same correspondent is routinely two rows since upstream split address-
    and key-contacts, which is the whole reason ADR 0021 exists.
    """
    best = None
    for contact_id in rpc.call("get_contact_ids", account_id, 0, addr):
        contact = rpc.call("get_contact", account_id, contact_id)
        if contact["address"].lower() != addr.lower():
            continue
        if contact["isKeyContact"] == want_key:
            return contact
        best = best or contact
    return best


def carol_headers(subject: str) -> email.message.Message:
    """Fetches carol's copy of a message and returns its parsed headers.

    Asserted at the mailbox rather than at a client because stock core has no
    per-message recipient set at all -- carrying Cc is ADR 0014, ours. The claim
    "the Cc reached a third party and is legible to an ordinary reader" is only
    checkable where no client of ours owns the data.
    """
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE

    def look():
        with imaplib.IMAP4(HOST, IMAP_PORT) as imap:
            imap.starttls(context)
            imap.login(f"carol@{DOMAIN}", ACCOUNTS["carol"])
            imap.select("INBOX")
            _, data = imap.search(None, "ALL")
            for num in data[0].split():
                _, fetched = imap.fetch(num, "(RFC822)")
                if not fetched or not isinstance(fetched[0], tuple):
                    continue
                message = email.message_from_bytes(fetched[0][1])
                if subject in (message.get("Subject") or ""):
                    return message
        return None

    return wait_for(look, "carol's copy to be delivered")


# ---------------------------------------------------------------------------
# Step 1 -- two engines, four accounts
# ---------------------------------------------------------------------------

def step1_eeemail_account(rpc: Rpc, user: str) -> int:
    """An eeemail account, with the defaults asserted rather than assumed.

    Lifted from `e2e-pass.py` step 1, and worth repeating here: the ordering
    trap is that `policy::apply_defaults` early-returns on a configured account,
    so the call must come *before* the transport. That was wrong in the GUI for
    two phases.
    """
    account_id = rpc.call("add_account")
    rpc.call("apply_eeemail_defaults", account_id)
    rpc.call("add_or_update_transport", account_id, login_params(user, ACCOUNTS))
    check(rpc.call("is_configured", account_id), f"{user} configures against real IMAP/SMTP")
    rpc.call("start_io", account_id)

    check(rpc.call("get_inbox_gating", account_id) is True, f"{user}: gating is on")
    check(rpc.call("get_trash_purge_days", account_id) == 30, f"{user}: expiry is recoverable")
    check(rpc.call("get_encryption_mode", account_id) == "opportunistic",
          f"{user}: encryption is opportunistic")
    return account_id


def step1_upstream_account(rpc: Rpc, user: str) -> int:
    """A stock account, with the one setting that makes email possible at all.

    Upstream defaults `force_encryption` to 1 (`config.rs`, `props(default =
    "1")`), and it is not advisory: `chat.rs:2958` refuses to send an
    unencrypted message, `imap.rs:1694` declines to *download* an unencrypted
    one, and `receive_imf.rs:509` trashes it if it arrives anyway.

    So a Delta Chat client in its shipped configuration cannot exchange
    cleartext in either direction, and eeemail's opportunistic bootstrap
    (ADR 0021) can never begin with one: the first message is dropped before it
    is parsed, and no Autocrypt header is ever seen. That is a genuine interop
    limit and the pass asserts the default rather than papering over it.

    Turning it off here is not harness fudging. It is the configuration Delta
    Chat offers for talking to ordinary email, it is the only one in which
    classic mail flows at all, and eeemail's own accounts reach the same place
    through `email::policy::apply_defaults`. What the pass then proves is
    everything downstream of that single setting.
    """
    account_id = rpc.call("add_account")
    check(rpc.call("get_config", account_id, "force_encryption") == "1",
          f"{user}: a stock client ships refusing cleartext in both directions")
    rpc.call("set_config", account_id, "force_encryption", "0")

    rpc.call("add_or_update_transport", account_id, login_params(user, ACCOUNTS))
    check(rpc.call("is_configured", account_id), f"{user} configures against real IMAP/SMTP")
    rpc.call("start_io", account_id)
    return account_id


def step1b_really_two_codebases(upstream: Rpc, account_id: int) -> None:
    """The assertion that makes every other assertion in this file mean anything.

    Everything below is only interop if the far end really is a different
    codebase. Without this, pointing both `Rpc`s at our own binary -- a one-word
    mistake -- yields a fully green run that proves nothing, which is the exact
    failure mode this whole script exists to fix.
    """
    error = upstream.call_expecting_error("apply_eeemail_defaults", account_id)
    # The code, not the message text: the text is upstream's to change.
    check(error.get("code") == -32601,
          "the far end has never heard of apply_eeemail_defaults",
          f"got {error}")


# ---------------------------------------------------------------------------
# Step 2 -- the Autocrypt bootstrap, hop by hop
# ---------------------------------------------------------------------------

def step2a_first_contact(ee: Rpc, dana: int, upstream: Rpc, erin: int, workdir: str) -> None:
    """dana mails erin: a real email, in the clear, with a Cc and an attachment.

    This is first contact *and* the classic-email leg, deliberately in one
    message. The Cc has to go out before the bootstrap makes the chat
    encrypted: upstream drops recipients whose key is missing from the envelope
    (issue #2), and carol has no key, so a Cc'd encrypted message would test
    that bug rather than the header.
    """
    attachment = os.path.join(workdir, "numbers.txt")
    with open(attachment, "w") as handle:
        handle.write("march,-120\napril,340\n")
    payload = os.path.getsize(attachment)

    ee.call("send_email", dana,
            {"to": [f"erin@{DOMAIN}"], "cc": [f"carol@{DOMAIN}"], "bcc": []},
            SUBJECT, BODY, attachment)

    message = wait_stock(upstream, erin, lambda m: m["subject"] == SUBJECT,
                         "erin to receive dana's first message")

    # Opportunistic means cleartext until a key is known. Our own pass says so;
    # this is a foreign parser agreeing.
    check(message["showPadlock"] is False,
          "first contact is cleartext, and a stock client reports it as cleartext")
    check(BODY in message["text"], "the body survives the round trip",
          f"got {message['text']!r}")
    check(message["fileName"] == "numbers.txt", "the attachment arrives named",
          f"got {message.get('fileName')!r}")
    check(message["fileBytes"] == payload, "and intact",
          f"got {message['fileBytes']} of {payload}")

    headers = carol_headers(SUBJECT)
    check((headers.get("Subject") or "") == SUBJECT,
          "the Subject: is real RFC 5322, in a third party's mailbox",
          f"got {headers.get('Subject')!r}")
    check(f"carol@{DOMAIN}" in (headers.get("Cc") or ""),
          "the Cc: reached a third party and is legible to an ordinary reader",
          f"got {headers.get('Cc')!r}")


def step2b_stock_replies_in_cleartext(upstream: Rpc, erin: int, ee: Rpc, dana: int) -> None:
    """erin replies, and cannot encrypt -- which is why ADR 0021 exists.

    Stock v2.59 imported dana's key at `mimeparser.rs:446` and attached it to no
    contact, so `Chat::is_encrypted` is false and the reply goes out in the
    clear. If this assertion ever fails, upstream has reinstated
    Autocrypt-derived contacts and `email::autocrypt::adopt` should be deleted
    rather than left to race it. This is the tripwire for that.
    """
    message = wait_stock(upstream, erin, lambda m: m["subject"] == SUBJECT,
                         "erin's copy of the first message")
    upstream.call("accept_chat", erin, message["chatId"])
    upstream.call("misc_send_text_message", erin, message["chatId"],
                  f"[{TOKEN}] {REPLY_CLEARTEXT}")

    reply_id = wait_eeemail(ee, dana, lambda r: REPLY_CLEARTEXT in r["preview"],
                            "dana to receive erin's reply")
    crypto = ee.call("get_message_crypto", dana, reply_id)
    check(crypto["encrypted"] is False,
          "a stock client cannot encrypt its first reply, however loudly we advertised")


def step2c_we_adopted_their_key(ee: Rpc, dana: int) -> None:
    """`email::autocrypt::adopt`, firing on a header a foreign codebase wrote."""
    contact = None
    for contact_id in ee.call("get_contact_ids", dana, 0, f"erin@{DOMAIN}"):
        candidate = ee.call("get_contact", dana, contact_id)
        if candidate["isKeyContact"]:
            contact = candidate
            break
    check(contact is not None,
          "an Autocrypt header from a stock client makes a key-contact")
    check(contact["isVerified"] is False,
          "and never a verified one -- ADR 0021 adds a rung below that rung")


def step2d_we_encrypt_and_they_can_read_it(ee: Rpc, dana: int,
                                           upstream: Rpc, erin: int) -> tuple[int, int]:
    """The single most valuable assertion in this file.

    Stock Delta Chat's engine reading mail eeemail encrypted to a key it learned
    from an unauthenticated Autocrypt header. ADR 0021 is a deliberate
    divergence from an upstream decision; this is the only evidence that the
    divergence produces mail upstream can actually read.
    """
    subject = f"[{TOKEN}] Second look"
    sent_id = ee.call("send_email", dana, {"to": [f"erin@{DOMAIN}"], "cc": [], "bcc": []},
                      subject, "This one should be encrypted.", None)
    # Waited for, not read straight back: send_email queues the message, and the
    # encryption state is settled when mimefactory renders it, not when the call
    # returns.
    wait_for(lambda: ee.call("get_message_crypto", dana, sent_id)["encrypted"],
             "dana's outgoing message to be rendered encrypted")
    check(True, "having learned a key, eeemail encrypts")

    message = wait_stock(upstream, erin, lambda m: m["subject"] == subject,
                         "erin to receive the encrypted message")
    check(message["showPadlock"] is True,
          "and a stock Delta Chat engine decrypts and verifies it")
    return message["chatId"], sent_id


def step2e_they_adopted_our_signature(upstream: Rpc, erin: int) -> None:
    """Our OpenPGP signature, checked by a foreign verifier rather than by rPGP
    round-tripping with itself. `receive_imf.rs:588` takes the fingerprint from
    the signature, which is what mints the key-contact on that side."""
    contact = stock_contact(upstream, erin, f"dana@{DOMAIN}", want_key=True)
    check(contact is not None and contact["isKeyContact"] is True,
          "our signature gives a stock client a key-contact for us")


def step2f_the_loop_closes(upstream: Rpc, erin: int, chat_id: int,
                           ee: Rpc, dana: int) -> None:
    """Both ends now encrypt, with nobody having scanned anything.

    That is ADR 0006's promise, and it has never been demonstrated across an
    implementation boundary. The new chat needs accepting on its own: the
    key-contact is a different contact row and so gets a different `Single`
    chat, whose contact-request state the acceptance in 2b did not touch.
    """
    upstream.call("accept_chat", erin, chat_id)
    upstream.call("misc_send_text_message", erin, chat_id, f"[{TOKEN}] {REPLY_ENCRYPTED}")

    reply_id = wait_eeemail(ee, dana, lambda r: REPLY_ENCRYPTED in r["preview"],
                            "dana to receive the encrypted reply",
                            timeout=ARRIVAL_TIMEOUT * 2)
    crypto = ee.call("get_message_crypto", dana, reply_id)
    check(crypto["encrypted"] is True, "a stock client now encrypts back to us")
    check(crypto["verified"] is False,
          "and it is honestly reported as unverified -- nobody scanned anything")


# ---------------------------------------------------------------------------
# Step 3 -- classic email, stock to eeemail
# ---------------------------------------------------------------------------

def step3_classic_inbound(upstream: Rpc, erin: int, chat_id: int,
                          ee: Rpc, dana: int, parent_id: int, workdir: str) -> None:
    """erin replies with a file, and eeemail renders and threads it.

    `email::threading` is our own JWZ implementation (ADR 0008), and "threads
    correctly" in DESIGN.md has never been checked against `In-Reply-To` and
    `References` headers a foreign client generated.

    The parent is step 2d's message, not step 2a's. Adopting erin's key moved
    the correspondence to her key-contact and so to a second chat, and erin is
    replying in that one -- so threading onto the cleartext original would be
    wrong, not right.
    """
    attachment = os.path.join(workdir, "reply.txt")
    with open(attachment, "w") as handle:
        handle.write("checked against the ledger\n")

    upstream.call("send_msg", erin, chat_id,
                  {"text": f"[{TOKEN}] {REPLY_ATTACHMENT}", "file": attachment,
                   "filename": "reply.txt", "viewtype": "File"})

    reply_id = wait_eeemail(ee, dana,
                            lambda r: r["hasAttachment"] and REPLY_ATTACHMENT in r["preview"],
                            "dana to receive erin's attachment",
                            timeout=ARRIVAL_TIMEOUT * 2)
    row = ee.call("get_message_rows", dana, [reply_id])[0]
    check(row["hasAttachment"], "an attachment from a stock client arrives")

    # Non-empty, not a specific value: stock core derives the subject from the
    # chat and offers no API to set one. A user-authored Subject and any Cc are
    # not merely unexercised in this direction, they are absent from that
    # client's model. Thunderbird is what would close that half.
    check(bool(row["subject"]), "it carries a subject eeemail can render",
          f"got {row['subject']!r}")

    theirs = ee.call("get_message_thread", dana, reply_id)
    ours = ee.call("get_message_thread", dana, parent_id)
    check(theirs is not None and theirs == ours,
          "and threads onto what it replied to, from headers a foreign client wrote",
          f"{theirs} vs {ours}")


# ---------------------------------------------------------------------------
# Steps 4 and 5 -- SecureJoin, once in each direction
# ---------------------------------------------------------------------------

def await_verified(rpc: Rpc, account_id: int, addr: str, who: str) -> None:
    def verified():
        for contact_id in rpc.call("get_contact_ids", account_id, 0, addr):
            contact = rpc.call("get_contact", account_id, contact_id)
            if contact["isKeyContact"] and contact["isVerified"]:
                return contact_id
        return None
    wait_for(verified, f"{who} to reach verified", timeout=SECUREJOIN_TIMEOUT)
    check(True, f"{who} reaches verified")


def step4_securejoin_we_invite(ee: Rpc, dana: int, upstream: Rpc, erin: int) -> None:
    """A stock client scans our QR code.

    SecureJoin is a multi-message state machine with encrypted handshake steps
    and token checks. `e2e-pass.py` runs it with the same core on both sides,
    which exercises the code and validates no wire format. This is the first
    time it crosses a build boundary.
    """
    qr = ee.call("get_chat_securejoin_qr_code", dana, None)
    upstream.call("secure_join", erin, qr, timeout=SECUREJOIN_TIMEOUT)
    await_verified(ee, dana, f"erin@{DOMAIN}", "erin, at our end")
    await_verified(upstream, erin, f"dana@{DOMAIN}", "dana, at the stock end")


def step5_securejoin_they_invite(upstream: Rpc, grace: int, ee: Rpc, frank: int) -> None:
    """We scan a stock client's QR code.

    A clean pair, because the joiner and inviter run different code paths and a
    second direction between the already-verified dana and erin would start
    from the answer.
    """
    # Cold, before anything else: a genuinely foreign stranger, classified by
    # ADR 0018's gating.
    frank_contact = upstream.call("create_contact", grace, f"frank@{DOMAIN}", None)
    chat_id = upstream.call("create_chat_by_contact_id", grace, frank_contact)
    upstream.call("misc_send_text_message", grace, chat_id, f"[{TOKEN}] Cold call from a stranger.")

    def held():
        for msg_id in ee.call("get_tagged_messages", frank, "unverified"):
            return msg_id
        return None

    msg_id = wait_for(held, "frank to hold the stranger's mail")
    check("unverified" in ee.call("get_message_tags", frank, msg_id)["system"],
          "a stranger running someone else's client is held, not delivered")
    check(bool(ee.call("get_message", frank, msg_id).get("text")),
          "and stays readable rather than quarantined")

    qr = upstream.call("get_chat_securejoin_qr_code", grace, None)
    ee.call("secure_join", frank, qr, timeout=SECUREJOIN_TIMEOUT)
    await_verified(upstream, grace, f"frank@{DOMAIN}", "frank, at the stock end")
    await_verified(ee, frank, f"grace@{DOMAIN}", "grace, at our end")

    # The cold message above is held on grace's *address* row -- unsigned mail
    # carries no fingerprint -- while SecureJoin verifies her *key* row. Those
    # being different rows is what issue #13 was: `release` selected per row
    # while `is_trusted` decided per person, so this message stayed held, and
    # trusted, until the deadline took it -- destroying it outright, back when
    # `gating::purge` did that rather than sweeping it into Trash. Asserted here
    # because a unit test cannot show it across an implementation boundary.
    def released():
        tags = ee.call("get_message_tags", frank, msg_id)["system"]
        return "unverified" not in tags
    wait_for(released, "the held message to be released by verification", timeout=60)
    check(True, "verifying a stranger releases the mail she sent cold (issue #13)")

    subject = f"[{TOKEN}] Verified across engines"
    ee.call("send_email", frank, {"to": [f"grace@{DOMAIN}"], "cc": [], "bcc": []},
            subject, "This one is verified on both sides.", None)
    message = wait_stock(upstream, grace, lambda m: m["subject"] == subject,
                         "grace to receive the post-verification message",
                         timeout=ARRIVAL_TIMEOUT * 2)
    check(message["showPadlock"] is True,
          "mail to a QR-verified stock contact is encrypted and signed")


# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description="eeemail against a real Delta Chat engine")
    parser.add_argument("--keep", action="store_true",
                        help="leave the accounts directories behind for inspection")
    parser.add_argument("--log", metavar="RUST_LOG",
                        help="pass RUST_LOG to both servers and let them write to stderr")
    parser.add_argument("--fetch-only", action="store_true",
                        help="download and verify the pinned upstream binary, then stop")
    parser.add_argument("--upstream-binary", metavar="PATH",
                        help="use this binary instead of the pinned release")
    args = parser.parse_args()

    tag, expected = read_pin()
    try:
        if args.upstream_binary:
            upstream_bin = args.upstream_binary
        else:
            upstream_bin = fetch_upstream(tag, expected)
        if args.fetch_only:
            print(f"{upstream_bin}\n{version_of(upstream_bin)}")
            return 0
        if not os.path.exists(EEEMAIL_BIN):
            print(f"missing {EEEMAIL_BIN}\n"
                  f"build it with: cd core && cargo build -p deltachat-rpc-server",
                  file=sys.stderr)
            return 2
        print(f"eeemail  {EEEMAIL_BIN}\n         {version_of(EEEMAIL_BIN)}")
        print(f"upstream {upstream_bin}\n         {version_of(upstream_bin)}")
        print(f"token    {TOKEN}\n")
        preflight_mailboxes()
    except Failure as err:
        print(f"  FAIL  {err}", file=sys.stderr)
        return 1

    ee_dir = tempfile.mkdtemp(prefix="eeemail-interop-ee-")
    up_dir = tempfile.mkdtemp(prefix="eeemail-interop-up-")
    ee = Rpc(EEEMAIL_BIN, ee_dir, log=args.log)
    upstream = Rpc(upstream_bin, up_dir, log=args.log)
    failures: list[str] = []

    def run(label: str, fn, *fn_args):
        """Runs one step, and keeps going if it fails.

        This matters more here than in the e2e pass: an interop run is slow and
        network-bound, and one broken hop should still tell you about the rest.
        """
        print(label)
        try:
            return fn(*fn_args)
        except Failure as err:
            print(f"  FAIL  {err}", file=sys.stderr)
            failures.append(f"{label}: {err}")
            return None

    try:
        dana = run("step 1: eeemail accounts", step1_eeemail_account, ee, "dana")
        frank = run("step 1: eeemail accounts (frank)", step1_eeemail_account, ee, "frank")
        erin = run("step 1: stock accounts", step1_upstream_account, upstream, "erin")
        grace = run("step 1: stock accounts (grace)", step1_upstream_account, upstream, "grace")
        if None in (dana, frank, erin, grace):
            raise Failure("no usable accounts; the remaining steps cannot run")

        run("step 1b: the far end really is a different codebase",
            step1b_really_two_codebases, upstream, erin)

        workdir = tempfile.mkdtemp(prefix="eeemail-interop-files-")
        run("step 2a: a real email, in the clear, to a stock client",
            step2a_first_contact, ee, dana, upstream, erin, workdir)
        run("step 2b: the stock client cannot encrypt its reply",
            step2b_stock_replies_in_cleartext, upstream, erin, ee, dana)
        run("step 2c: we adopt their Autocrypt key",
            step2c_we_adopted_their_key, ee, dana)
        encrypted = run("step 2d: we encrypt, and they can read it",
                        step2d_we_encrypt_and_they_can_read_it, ee, dana, upstream, erin)
        chat_id, parent_id = encrypted if encrypted else (None, None)
        run("step 2e: they adopt our signature",
            step2e_they_adopted_our_signature, upstream, erin)
        if chat_id is not None:
            run("step 2f: the loop closes, with nobody having scanned anything",
                step2f_the_loop_closes, upstream, erin, chat_id, ee, dana)
            run("step 3: classic email, stock to eeemail",
                step3_classic_inbound, upstream, erin, chat_id, ee, dana, parent_id, workdir)

        run("step 4: SecureJoin, a stock client scans our code",
            step4_securejoin_we_invite, ee, dana, upstream, erin)
        run("step 5: SecureJoin, we scan a stock client's code",
            step5_securejoin_they_invite, upstream, grace, ee, frank)
    except Failure as err:
        print(f"  FAIL  {err}", file=sys.stderr)
        failures.append(str(err))
    finally:
        ee.close()
        upstream.close()
        if args.keep:
            print(f"accounts left in {ee_dir} and {up_dir}", file=sys.stderr)
        else:
            shutil.rmtree(ee_dir, ignore_errors=True)
            shutil.rmtree(up_dir, ignore_errors=True)

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
