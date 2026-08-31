#!/usr/bin/env python3
"""Smoke test for the eeemail test mail server.

Verifies the properties later phases depend on, and the ones whose absence
would be a security bug. Run against a started server:

    python3 server/compose/smoke-test.py [--host 127.0.0.1] [--strict]
"""
import argparse, email, email.policy, imaplib, smtplib, ssl, sys, time
from email.message import EmailMessage

def imap_connect(host, port, user, pw):
    """IMAP over STARTTLS. The server disallows plaintext auth (correctly), and
    the test certificate is self-signed, so verification is turned off here --
    the equivalent of core's `imap_certificate_checks=accept_invalid_certificates`."""
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    m = imaplib.IMAP4(host, port)
    m.starttls(ctx)
    m.login(user, pw)
    return m

FAILURES = []

def check(name):
    def deco(fn):
        def run(*a, **kw):
            try:
                detail = fn(*a, **kw)
                print(f"  PASS  {name}" + (f": {detail}" if detail else ""))
                return True
            except Exception as e:
                print(f"  FAIL  {name}: {e}")
                FAILURES.append(name)
                return False
        return run
    return deco

def submit(host, port, user, pw, msg, domain):
    s = smtplib.SMTP(host, port, timeout=20)
    try:
        s.ehlo(); s.starttls(); s.ehlo()
        s.login(f"{user}@{domain}", pw)
        s.send_message(msg)
    finally:
        try: s.quit()
        except Exception: pass

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--domain", default="eeemail.test")
    ap.add_argument("--submission-port", type=int, default=2587)
    ap.add_argument("--smtp-port", type=int, default=2525)
    ap.add_argument("--imap-port", type=int, default=2143)
    ap.add_argument("--strict", action="store_true",
                    help="assert strict-profile behavior (outer Subject minimized)")
    args = ap.parse_args()
    H, D = args.host, args.domain
    subject = f"eeemail smoke {time.time():.0f}"

    @check("SMTP submission with STARTTLS + SASL, message accepted")
    def t_submit():
        m = EmailMessage()
        m["From"] = f"alice@{D}"; m["To"] = f"bob@{D}"
        m["Subject"] = subject; m.set_content("hello from alice")
        submit(H, args.submission_port, "alice", "alicepw", m, D)

    @check("sender-login mismatch rejected (alice may not send as bob)")
    def t_forge():
        m = EmailMessage()
        m["From"] = f"bob@{D}"; m["To"] = f"bob@{D}"
        m["Subject"] = "forged"; m.set_content("x")
        try:
            submit(H, args.submission_port, "alice", "alicepw", m, D)
        except (smtplib.SMTPRecipientsRefused, smtplib.SMTPSenderRefused):
            return
        raise AssertionError("forged sender was ACCEPTED")

    @check("port 25 is not an open relay")
    def t_relay():
        s = smtplib.SMTP(H, args.smtp_port, timeout=20)
        try:
            s.ehlo()
            s.sendmail(f"alice@{D}", "outsider@example.org", "Subject: relay\n\nx")
        except smtplib.SMTPRecipientsRefused:
            return
        finally:
            try: s.quit()
            except Exception: pass
        raise AssertionError("relayed to an external domain unauthenticated")

    @check("port 25 accepts inbound mail addressed to a local account")
    def t_inbound():
        s = smtplib.SMTP(H, args.smtp_port, timeout=20)
        try:
            s.ehlo()
            s.sendmail("outsider@example.org", f"bob@{D}",
                       f"From: outsider@example.org\r\nTo: bob@{D}\r\n"
                       f"Subject: inbound\r\n\r\nfrom outside\r\n")
        finally:
            try: s.quit()
            except Exception: pass

    msgs = {}

    @check("IMAP login and LMTP delivery")
    def t_deliver():
        for _ in range(30):
            m = imap_connect(H, args.imap_port, f"bob@{D}", "bobpw")
            m.select("INBOX")
            ids = m.search(None, "ALL")[1][0].split()
            found = {}
            for i in ids:
                raw = m.fetch(i, "(RFC822)")[1][0][1]
                em = email.message_from_bytes(raw, policy=email.policy.default)
                found[em.get("Subject", "")] = em
            if len(found) >= 2:
                msgs.update(found)
                msgs["_caps"] = m.capabilities
                m.logout()
                return f"{len(found)} messages in INBOX"
            m.logout(); time.sleep(1)
        raise AssertionError("messages did not arrive within 30s")

    @check("IMAP refuses plaintext auth without TLS")
    def t_plaintext():
        m = imaplib.IMAP4(H, args.imap_port)
        try:
            m.login(f"bob@{D}", "bobpw")
        except imaplib.IMAP4.error:
            return "PRIVACYREQUIRED as expected"
        finally:
            try: m.logout()
            except Exception: pass
        raise AssertionError("plaintext auth was ALLOWED on a non-TLS connection")

    @check("IMAP advertises the capabilities eeemail needs")
    def t_caps():
        caps = msgs.get("_caps", ())
        missing = [c for c in ("IDLE", "METADATA", "QUOTA") if c not in caps]
        if missing:
            raise AssertionError(f"missing {missing}")
        return "IDLE, METADATA, QUOTA"

    @check("Subject handling matches the profile")
    def t_subject():
        subjects = [k for k in msgs if k != "_caps"]
        if args.strict:
            if subject in subjects:
                raise AssertionError("strict profile did not minimize the outer Subject")
            # Must be non-vacuous: the message has to have arrived, minimized.
            if "[...]" not in subjects:
                raise AssertionError(
                    f"expected a '[...]' Subject from the cleanup; got {subjects}")
            return "outer Subject minimized to '[...]'"
        if subject not in subjects:
            raise AssertionError(
                f"permissive profile must preserve Subject; got {subjects}")
        return "preserved verbatim"

    @check("message body intact")
    def t_body():
        for k, m in msgs.items():
            if k == "_caps":
                continue
            body = m.get_body(preferencelist=("plain",))
            if body is not None and "hello from alice" in body.get_content():
                return
        raise AssertionError("submitted message body not found intact")

    for t in (t_submit, t_forge, t_relay, t_inbound, t_deliver, t_plaintext,
              t_caps, t_subject, t_body):
        t()

    print()
    if FAILURES:
        print(f"FAILED: {len(FAILURES)} check(s): {', '.join(FAILURES)}")
        return 1
    print("ALL CHECKS PASSED")
    return 0

if __name__ == "__main__":
    sys.exit(main())
