# eeemail test mail server: Postfix (transport) + Dovecot (spool).
#
# Adapted from chatmail/relay's cmdeploy configuration (MIT), with relay
# account provisioning removed -- accounts here are traditional user@domain,
# created from server/compose/accounts. See docs/adr/0007-server-template.md.
#
# This image is for testing and local development. It is NOT hardened for
# production: it generates a self-signed certificate and reads passwords from
# a plaintext file.
FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      postfix postfix-pcre \
      dovecot-core dovecot-imapd dovecot-lmtpd \
      openssl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Dovecot runs mail as an unprivileged vmail user; Maildir lives under its home.
RUN groupadd -g 5000 vmail \
    && useradd -u 5000 -g vmail -d /var/vmail -m -s /usr/sbin/nologin vmail

COPY config/postfix/ /etc/postfix/
COPY config/dovecot/dovecot.conf /etc/dovecot/dovecot.conf
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# 25 SMTP, 587 submission, 465 submissions, 143 IMAP, 993 IMAPS
EXPOSE 25 143 465 587 993

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
