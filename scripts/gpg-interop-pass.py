#!/usr/bin/env python3
"""eeemail against a GnuPG client -- the second OpenPGP implementation.

`interop-pass.py` runs eeemail against Delta Chat's own engine, which closes
the Delta Chat half of issue #5. Both ends of that pass are still rPGP: our
outgoing PGP/MIME has only ever been read by the implementation that wrote it.
This pass is the other side of that -- it proves eeemail's **outgoing**
PGP/MIME and signatures are readable by GnuPG, an OpenPGP implementation that
shares no code with ours.

What it is not: a Thunderbird test. Thunderbird uses RNP, not GnuPG. This is
the closest automatable stand-in, and it narrows the Thunderbird bullet of #5
rather than closing it (issue #14).

    cd server/compose && docker compose up -d --build && python3 smoke-test.py
    cd ../.. && cd core && cargo build -p deltachat-rpc-server && cd ..
    python3 scripts/gpg-interop-pass.py

Run it against the **permissive** profile. The `STRICT_E2EE=1` container
rewrites `Subject` at submission, and every step here matches on the subject.

## Why this shares dcrpc.py

Issue #14 sketched this as stdlib-plus-`gpg` in `smoke-test.py`'s style. Half
of that holds: the GnuPG side is `smtplib`, `imaplib` and the `gpg` binary,
with no RPC anywhere. But the eeemail side has to actually run -- something must
adopt the key and send the encrypted reply -- so it drives
`deltachat-rpc-server` exactly as the other two passes do, and reuses their wire
client for the reason that client was extracted: a framing bug fixed in one copy
and not the other gives a green run that tests nothing.

## What it found

Our encrypted mail carries no `Autocrypt:` header on the outside. The key is
inside, among RFC 9788 protected headers (`hp="cipher"`) along with the real
`Subject` and `To`, so an observer learns neither who is being written to nor
which key. The consequence for a correspondent who is not Delta Chat is that
the key cannot be had before decrypting -- which is fine, since decryption
needs only their own key, but it does mean "import the sender's key from the
header" does not work here the way it does for cleartext Autocrypt mail.

## What a failure here means

`gpg` reporting a bad signature or a failed decryption is a real interop
finding: it means mail eeemail sends cannot be read by a large fraction of the
OpenPGP world. A failure in step 2 (the inbound leg) is a weaker signal --
that path is already covered by `interop-pass.py` step 2c against a foreign
Autocrypt implementation.
"""
import argparse
import base64
import email
import email.policy
import imaplib
import os
import re
import secrets
import smtplib
import ssl
import subprocess
import sys
import tempfile
from email.mime.text import MIMEText

from dcrpc import (
    ARRIVAL_TIMEOUT,
    DOMAIN,
    HOST,
    IMAP_PORT,
    SMTP_PORT,
    Failure,
    Rpc,
    check,
    transport as login_params,
    wait_for,
)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EEEMAIL_BIN = os.path.join(REPO, "core", "target", "debug", "deltachat-rpc-server")

# Mailboxes of this pass's own, for the reason the interop pass has its own:
# a mailbox carrying another pass's completed handshake makes the bootstrap
# this exists to observe unobservable. `heidi` is the GnuPG client and never
# runs any of our code; `ivan` is eeemail.
ACCOUNTS = {"heidi": "heidipw", "ivan": "ivanpw"}
GPG_USER, EE_USER = "heidi", "ivan"
GPG_ADDR = f"{GPG_USER}@{DOMAIN}"
EE_ADDR = f"{EE_USER}@{DOMAIN}"

# Mailboxes persist across runs, so a fixed subject would match a leftover.
TOKEN = secrets.token_hex(4)
INBOUND_SUBJECT = f"[{TOKEN}] Contract draft"
INBOUND_BODY = "Draft attached in the next one. Does clause 4 still read right?"
REPLY_SUBJECT = f"[{TOKEN}] Re: Contract draft"
REPLY_BODY = "Clause 4 is fine. Clause 7 is the one that changed."


# ---------------------------------------------------------------------------
# GnuPG
# ---------------------------------------------------------------------------

def gpg(home: str, *args: str, stdin: bytes | None = None) -> subprocess.CompletedProcess:
    """Runs `gpg` against an isolated keyring.

    `--batch --yes --no-tty` throughout: any prompt is a hang in CI, and a hang
    is much harder to read than a failure.
    """
    command = ["gpg", "--homedir", home, "--batch", "--yes", "--no-tty", *args]
    return subprocess.run(command, input=stdin, capture_output=True, timeout=120)


def gpg_version() -> str:
    try:
        done = subprocess.run(["gpg", "--version"], capture_output=True, text=True, timeout=30)
    except OSError as err:
        raise Failure(f"gpg will not execute: {err}") from err
    return done.stdout.splitlines()[0] if done.stdout else "gpg (unknown version)"


def generate_key(home: str) -> None:
    """A fresh keypair for the GnuPG side, with no passphrase.

    `--quick-generate-key` rather than a `--gen-key` parameter file: it is one
    line, and the parameter-file format is a source of silent defaults.
    """
    done = gpg(home, "--passphrase", "", "--quick-generate-key",
               f"Heidi Held <{GPG_ADDR}>", "default", "default", "never")
    if done.returncode != 0:
        raise Failure(f"key generation failed: {done.stderr.decode(errors='replace').strip()}")


def public_keydata(home: str) -> str:
    """The GnuPG public key, base64'd for an Autocrypt header.

    Exported in OpenPGP's binary form and base64'd, which is exactly what
    Autocrypt Level 1 asks for. Exporting armored and stripping the armor
    afterwards reaches the same bytes through a step that can go wrong.
    """
    done = gpg(home, "--export", GPG_ADDR)
    if done.returncode != 0 or not done.stdout:
        raise Failure(f"key export failed: {done.stderr.decode(errors='replace').strip()}")
    return base64.b64encode(done.stdout).decode("ascii")


def autocrypt_header(addr: str, keydata: str) -> str:
    """An Autocrypt Level 1 header, folded.

    `keydata` runs to several thousand characters and RFC 5322 caps a line at
    998, so folding is not cosmetic. Autocrypt says whitespace inside `keydata`
    is ignored, which is what makes folding inside base64 legal.
    """
    folded = "".join(f"\n {keydata[i:i + 72]}" for i in range(0, len(keydata), 72))
    return f"addr={addr}; prefer-encrypt=mutual; keydata={folded}"


def decrypt_and_verify(home: str, ciphertext: bytes) -> tuple[bytes, list[str]]:
    """Decrypts with GnuPG and returns the plaintext plus its status lines.

    The status lines, not the human-readable stderr: `--status-fd` is a stable
    machine interface, while the text is localized and rewritten between
    releases. `GOODSIG`/`VALIDVSIG` there is the assertion this whole script
    exists to make.
    """
    done = gpg(home, "--status-fd", "2", "--decrypt", stdin=ciphertext)
    status = [line[len("[GNUPG:] "):]
              for line in done.stderr.decode(errors="replace").splitlines()
              if line.startswith("[GNUPG:] ")]
    return done.stdout, status


# ---------------------------------------------------------------------------
# The wire
# ---------------------------------------------------------------------------

def import_sender_key(home: str, plaintext: bytes) -> bool:
    """Imports the sender's public key out of a decrypted payload.

    Core carries no `Autocrypt:` header on the *outside* of encrypted mail. It
    puts it inside, among RFC 9788 protected headers (`hp="cipher"`), together
    with the real `Subject` and `To`. That is better for privacy -- an observer
    learns neither who is being written to nor which key -- and it means a
    correspondent who is not Delta Chat has to decrypt before it can verify,
    which is the order GnuPG does things in anyway.

    Also accepts an armored key block, so this keeps working if the key ever
    arrives as an attachment instead.
    """
    message = email.message_from_bytes(plaintext)
    header = " ".join((message.get("Autocrypt") or "").split())
    found = re.search(r"keydata=([A-Za-z0-9+/=\s]+)$", header)
    if found:
        try:
            key = base64.b64decode(re.sub(r"\s+", "", found.group(1)))
        except Exception:
            key = b""
        if key and gpg(home, "--import", stdin=key).returncode == 0:
            return True

    for block in re.findall(
            rb"-----BEGIN PGP PUBLIC KEY BLOCK-----.*?-----END PGP PUBLIC KEY BLOCK-----",
            plaintext, re.S):
        if gpg(home, "--import", stdin=block).returncode == 0:
            return True
    return False


def tls_context() -> ssl.SSLContext:
    """`entrypoint.sh` regenerates a self-signed certificate on every start."""
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    return context


def submit(message: email.message.Message, user: str) -> None:
    server = smtplib.SMTP(HOST, SMTP_PORT, timeout=30)
    try:
        server.ehlo()
        server.starttls(context=tls_context())
        server.ehlo()
        server.login(f"{user}@{DOMAIN}", ACCOUNTS[user])
        server.send_message(message)
    finally:
        try:
            server.quit()
        except Exception:
            pass


def fetch(user: str, predicate, what: str,
          timeout: int = ARRIVAL_TIMEOUT) -> email.message.Message:
    """Waits for a message in `user`'s mailbox that `predicate` accepts.

    Read at the mailbox rather than through any client of ours, which is the
    point: what GnuPG has to cope with is the bytes on the wire.

    By predicate rather than by subject, because the message this pass cares
    about is encrypted and core minimizes the outer `Subject` of encrypted mail
    to `[...]` -- correctly. Matching on a subject that is deliberately not
    there is a test that can only ever time out.
    """
    def look():
        with imaplib.IMAP4(HOST, IMAP_PORT) as imap:
            imap.starttls(tls_context())
            imap.login(f"{user}@{DOMAIN}", ACCOUNTS[user])
            imap.select("INBOX")
            _, data = imap.search(None, "ALL")
            for num in data[0].split():
                _, fetched = imap.fetch(num, "(RFC822)")
                if not fetched or not isinstance(fetched[0], tuple):
                    continue
                message = email.message_from_bytes(fetched[0][1])
                if predicate(message):
                    return message
        return None

    return wait_for(look, what, timeout=timeout)


def preflight() -> None:
    """Both mailboxes, before anything slow runs.

    These two are additions to the compose file's ACCOUNTS. A server brought up
    from an older compose file, or from the bare `docker run` in
    server/README.md, has neither -- which would otherwise surface as a
    configure failure three steps in, reading like a network problem.
    """
    missing = []
    for user, password in sorted(ACCOUNTS.items()):
        try:
            with imaplib.IMAP4(HOST, IMAP_PORT) as imap:
                imap.starttls(tls_context())
                imap.login(f"{user}@{DOMAIN}", password)
        except Exception:
            missing.append(user)
    if missing:
        raise Failure(
            f"not provisioned: {', '.join(m + '@' + DOMAIN for m in missing)}\n"
            f"    these mailboxes are this pass's own; bring the server up from\n"
            f"    server/compose/docker-compose.yml, recreating it if it predates them")
    check(True, f"both mailboxes accept a login")


# ---------------------------------------------------------------------------
# Step 1 -- the two implementations
# ---------------------------------------------------------------------------

def step1_eeemail_account(rpc: Rpc) -> int:
    """An eeemail account, with its defaults asserted rather than assumed.

    `policy::apply_defaults` early-returns on a configured account, so the call
    has to come before the transport. Asserting the result rather than the call
    is what makes this a check instead of a comment.
    """
    account_id = rpc.call("add_account")
    rpc.call("apply_eeemail_defaults", account_id)
    rpc.call("add_or_update_transport", account_id, login_params(EE_USER, ACCOUNTS))
    check(rpc.call("is_configured", account_id), f"{EE_USER} configures against real IMAP/SMTP")
    rpc.call("start_io", account_id)
    check(rpc.call("get_encryption_mode", account_id) == "opportunistic",
          f"{EE_USER}: encryption is opportunistic")
    check(rpc.call("get_inbox_gating", account_id) is True, f"{EE_USER}: gating is on")
    return account_id


def step1_gpg_keyring(home: str) -> str:
    generate_key(home)
    keydata = public_keydata(home)
    check(len(keydata) > 100, "GnuPG generated a keypair and exported it",
          f"keydata was {len(keydata)} chars")
    return keydata


# ---------------------------------------------------------------------------
# Step 2 -- inbound: a GnuPG client's Autocrypt header
# ---------------------------------------------------------------------------

def step2_inbound(rpc: Rpc, account_id: int, keydata: str) -> int:
    """heidi mails ivan in the clear, advertising her key.

    Weaker evidence than step 3, and deliberately kept: `interop-pass.py` step
    2c already proves we adopt a key from a header a foreign codebase wrote.
    What this leg is really here for is to put a key in front of eeemail so
    that step 3 has something to encrypt to.
    """
    # `MIMEText` rather than `EmailMessage`: the modern policy refuses to store
    # a header containing a line break, and an Autocrypt header has to be folded
    # because `keydata` runs past RFC 5322's 998-character line limit on its
    # own. compat32 stores the folded value verbatim, which is what the spec
    # describes.
    message = MIMEText(INBOUND_BODY)
    message["From"] = f"Heidi Held <{GPG_ADDR}>"
    message["To"] = EE_ADDR
    message["Subject"] = INBOUND_SUBJECT
    message["Autocrypt"] = autocrypt_header(GPG_ADDR, keydata)
    submit(message, GPG_USER)

    # Held, not delivered: heidi is neither verified nor known, and gating is
    # on. Unverified is a view -- the message is downloaded and readable.
    def look():
        for msg_id in rpc.call("get_tagged_messages", account_id, "unverified"):
            row = rpc.call("get_message_rows", account_id, [msg_id])[0]
            if row["subject"] == INBOUND_SUBJECT:
                return msg_id
        return None

    msg_id = wait_for(look, "eeemail to receive and hold a stranger's mail")
    check(True, "a stranger's mail is held rather than delivered (ADR 0018)")

    row = rpc.call("get_message_rows", account_id, [msg_id])[0]
    check(INBOUND_BODY in (row["preview"] or ""), "and is readable while held",
          f"got {row['preview']!r}")
    crypto = rpc.call("get_message_crypto", account_id, msg_id)
    check(crypto["encrypted"] is False, "first contact is cleartext, as opportunistic means")
    return msg_id


def step2b_key_adopted(rpc: Rpc, account_id: int) -> None:
    """eeemail made a key-contact out of the advertised key (ADR 0021).

    Upstream v2.59 mints a key-contact only from a signature or SecureJoin, so
    without ADR 0021 there would be no key here to encrypt to and the pass could
    not continue. Asserted separately from step 3 so that "we never adopted the
    key" and "we adopted it and the PGP/MIME is wrong" cannot be confused.
    """
    def look():
        for contact_id in rpc.call("get_contact_ids", account_id, 0, GPG_ADDR):
            contact = rpc.call("get_contact", account_id, contact_id)
            if contact["address"].lower() == GPG_ADDR and contact["isKeyContact"]:
                return contact
        return None

    contact = wait_for(look, "eeemail to adopt the advertised key")
    check(contact["isVerified"] is False,
          "the adopted key is not verified -- nobody scanned anything")


# ---------------------------------------------------------------------------
# Step 3 -- outbound: our PGP/MIME, read by GnuPG
# ---------------------------------------------------------------------------

def step3_outbound(rpc: Rpc, account_id: int, home: str) -> None:
    """The assertion this file exists for.

    eeemail replies. Because it adopted heidi's key the reply goes out
    encrypted, and GnuPG -- which has never seen a line of rPGP -- has to
    decrypt it and verify the signature on it.
    """
    msg_id = rpc.call("send_email", account_id,
                      {"to": [GPG_ADDR], "cc": [], "bcc": []},
                      REPLY_SUBJECT, REPLY_BODY, None)
    crypto = rpc.call("get_message_crypto", account_id, msg_id)
    check(crypto["encrypted"] is True, "we believe we encrypted the reply",
          f"got {crypto}")

    # Ours is the encrypted one from ivan whose plaintext carries this run's
    # token. The mailbox keeps every previous run's reply, and they all look
    # alike from the outside -- which is the point of encrypting them.
    def is_ours(candidate: email.message.Message) -> bool:
        if candidate.get_content_type() != "multipart/encrypted":
            return False
        if EE_ADDR not in (candidate.get("From") or ""):
            return False
        parts = candidate.get_payload()
        if len(parts) != 2:
            return False
        body, _ = decrypt_and_verify(home, parts[1].get_payload(decode=True))
        return TOKEN.encode() in body

    message = fetch(GPG_USER, is_ours, "GnuPG to receive an encrypted reply it can open")

    # Structure. A message that is not PGP/MIME might still decrypt by accident
    # of inline PGP, and would be a different thing from the one we claim to
    # send.
    check(message.get_content_type() == "multipart/encrypted",
          "the reply is multipart/encrypted", f"got {message.get_content_type()}")
    check((message.get_param("protocol") or "") == "application/pgp-encrypted",
          "with the PGP/MIME protocol parameter (RFC 3156)",
          f"got {message.get_param('protocol')!r}")
    parts = message.get_payload()
    check(parts[0].get_content_type() == "application/pgp-encrypted",
          "the first part being the version part",
          f"got {parts[0].get_content_type()}")
    check("version: 1" in parts[0].get_payload().strip().lower(),
          "which says Version: 1", f"got {parts[0].get_payload()!r}")

    # The subject must not be legible on the wire. Core minimizes it and puts
    # the real one inside the encrypted part.
    check(REPLY_SUBJECT not in (message.get("Subject") or ""),
          "the real subject is not in the outer headers",
          f"got {message.get('Subject')!r}")

    plaintext, status = decrypt_and_verify(home, parts[1].get_payload(decode=True))
    check(any(line.startswith("DECRYPTION_OKAY") for line in status),
          "GnuPG decrypts PGP/MIME that rPGP produced", f"status was {status}")
    check(REPLY_BODY.encode() in plaintext,
          "and the body survives to a foreign implementation",
          f"got {plaintext[:200]!r}")
    check(REPLY_SUBJECT.encode() in plaintext,
          "with the real subject inside the encrypted part, where it belongs")

    # GnuPG needs our public key to check the signature on it, and it will not
    # have arrived by any channel GnuPG knows about: core carries no
    # `Autocrypt:` header on encrypted mail, and puts the sender's key inside
    # the encrypted part instead. Extract it from there.
    imported = import_sender_key(home, plaintext)
    check(imported, "our key is recoverable from inside the encrypted part",
          "no Autocrypt keydata or key block in the decrypted payload")
    if imported:
        _, verified = decrypt_and_verify(home, parts[1].get_payload(decode=True))
        check(any(line.startswith("GOODSIG") for line in verified),
              "and our signature verifies under GnuPG", f"status was {verified}")


def step4_release(rpc: Rpc, account_id: int, msg_id: int) -> None:
    """Writing to heidi made her known, which releases what she sent cold.

    Free to assert here and worth having: it is the same release path issue #13
    was about, exercised against mail from something that is not our code.
    """
    def look():
        tags = rpc.call("get_message_tags", account_id, msg_id)
        return "unverified" not in [str(t).lower() for t in tags.get("system", [])]

    wait_for(look, "the held message to be released", timeout=30)
    check(True, "replying to the sender released the mail she sent cold")


def main() -> int:
    parser = argparse.ArgumentParser(description="eeemail against a GnuPG client")
    parser.add_argument("--keep", action="store_true",
                        help="leave the accounts directory and keyring behind")
    parser.add_argument("--log", metavar="RUST_LOG",
                        help="pass RUST_LOG to the server and let it write to stderr")
    args = parser.parse_args()

    try:
        if not os.path.exists(EEEMAIL_BIN):
            print(f"missing {EEEMAIL_BIN}\n"
                  f"build it with: cd core && cargo build -p deltachat-rpc-server",
                  file=sys.stderr)
            return 2
        print(f"eeemail  {EEEMAIL_BIN}")
        print(f"gnupg    {gpg_version()}")
        print(f"token    {TOKEN}\n")
        preflight()
    except Failure as err:
        print(f"  FAIL  {err}", file=sys.stderr)
        return 1

    ee_dir = tempfile.mkdtemp(prefix="eeemail-gpg-ee-")
    gpg_home = tempfile.mkdtemp(prefix="eeemail-gpg-home-")
    os.chmod(gpg_home, 0o700)
    rpc = Rpc(EEEMAIL_BIN, ee_dir, log=args.log)
    failures: list[str] = []

    def run(label: str, fn, *fn_args):
        print(label)
        try:
            return fn(*fn_args)
        except Failure as err:
            print(f"  FAIL  {err}", file=sys.stderr)
            failures.append(f"{label}: {err}")
            return None

    try:
        account_id = run("step 1: an eeemail account", step1_eeemail_account, rpc)
        keydata = run("step 1: a GnuPG keyring", step1_gpg_keyring, gpg_home)
        if account_id is None or keydata is None:
            raise Failure("no usable endpoints; the remaining steps cannot run")

        msg_id = run("step 2: inbound, a GnuPG client's Autocrypt header",
                     step2_inbound, rpc, account_id, keydata)
        run("step 2b: the key is adopted", step2b_key_adopted, rpc, account_id)
        run("step 3: outbound, our PGP/MIME read by GnuPG",
            step3_outbound, rpc, account_id, gpg_home)
        if msg_id is not None:
            run("step 4: the held message is released", step4_release, rpc, account_id, msg_id)
    except Failure as err:
        print(f"  FAIL  {err}", file=sys.stderr)
        failures.append(str(err))
    finally:
        rpc.close()
        # gpg-agent holds the socket in GNUPGHOME open; without this the
        # directory cannot be removed and the run leaks one per invocation.
        subprocess.run(["gpgconf", "--homedir", gpg_home, "--kill", "all"],
                       capture_output=True)
        if not args.keep:
            import shutil
            shutil.rmtree(ee_dir, ignore_errors=True)
            shutil.rmtree(gpg_home, ignore_errors=True)
        else:
            print(f"\nkept {ee_dir}\nkept {gpg_home}")

    if failures:
        print(f"\n{len(failures)} FAILED", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1
    print("\nALL STEPS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
