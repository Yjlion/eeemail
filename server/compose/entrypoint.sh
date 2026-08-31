#!/bin/bash
# eeemail test mail server entrypoint.
#
# Provisions traditional user@domain accounts from ACCOUNTS, generates a
# self-signed certificate, and runs Postfix + Dovecot in the foreground.
#
# ACCOUNTS format: whitespace/newline separated "localpart:password" pairs.
# Addresses are localpart@MAIL_DOMAIN.
set -euo pipefail

MAIL_DOMAIN="${MAIL_DOMAIN:-eeemail.test}"
ACCOUNTS="${ACCOUNTS:-alice:alicepw bob:bobpw}"

echo "eeemail test mail server: domain=${MAIL_DOMAIN}"

# --- TLS -------------------------------------------------------------------
# Self-signed and regenerated on every start. Clients must be told to accept
# it: core exposes `imap_certificate_checks=accept_invalid_certificates`.
mkdir -p /etc/dovecot/ssl
if [ ! -s /etc/dovecot/ssl/cert.pem ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
    -keyout /etc/dovecot/ssl/key.pem \
    -out /etc/dovecot/ssl/cert.pem \
    -subj "/CN=${MAIL_DOMAIN}" \
    -addext "subjectAltName=DNS:${MAIL_DOMAIN},DNS:localhost" 2>/dev/null
  chmod 600 /etc/dovecot/ssl/key.pem
fi

# --- Accounts --------------------------------------------------------------
# passwd-file with PLAIN scheme. Fine for a disposable test server; never for
# production, where these belong in a real passdb.
: > /etc/dovecot/users
for entry in $ACCOUNTS; do
  user="${entry%%:*}"
  pass="${entry#*:}"
  if [ -z "$user" ] || [ "$user" = "$entry" ]; then
    echo "ACCOUNTS entry must be 'user:password', got '${entry}'" >&2
    exit 1
  fi
  addr="${user}@${MAIL_DOMAIN}"
  echo "${addr}:{PLAIN}${pass}" >> /etc/dovecot/users
  install -d -o vmail -g vmail -m 0700 "/var/vmail/${addr}"
  echo "  account ${addr}"
done
# Readable by Dovecot's auth process (runs as the dovecot user), not world.
chown root:dovecot /etc/dovecot/users
chmod 640 /etc/dovecot/users

# --- Postfix ---------------------------------------------------------------
sed -i "s/MAIL_DOMAIN/${MAIL_DOMAIN}/g" /etc/postfix/main.cf
postconf -e "mydestination = ${MAIL_DOMAIN}, localhost, localhost.localdomain"

# The `strict` profile mounts this file in; only then do we rewrite headers.
if [ -f /etc/postfix/submission_header_cleanup ] && [ "${STRICT_E2EE:-0}" = "1" ]; then
  echo "  strict profile: submission header cleanup enabled"
  # Only the submission paths get the rewriting cleanup service. Do NOT touch
  # the global smtpd_sender_restrictions here: that would apply
  # reject_sender_login_mismatch to port 25 and refuse all inbound mail.
  postconf -P "submission/inet/cleanup_service_name=subcleanup"
  postconf -P "smtps/inet/cleanup_service_name=subcleanup"
  cat >> /etc/postfix/master.cf <<'EOF'
subcleanup unix n       -       n       -       0       cleanup
  -o header_checks=pcre:/etc/postfix/submission_header_cleanup
EOF
fi

newaliases 2>/dev/null || true

# --- Run -------------------------------------------------------------------
# Dovecot must start first: Postfix's submission SASL and LMTP both depend on
# sockets Dovecot creates under /var/spool/postfix/private/.
mkdir -p /var/spool/postfix/private
dovecot -F &
dovecot_pid=$!

for _ in $(seq 1 50); do
  [ -S /var/spool/postfix/private/auth ] && break
  sleep 0.1
done
if [ ! -S /var/spool/postfix/private/auth ]; then
  echo "dovecot did not create the SASL auth socket" >&2
  exit 1
fi

postfix start-fg &
postfix_pid=$!

term() { kill -TERM "$dovecot_pid" "$postfix_pid" 2>/dev/null || true; }
trap term TERM INT

echo "eeemail test mail server ready"
wait -n "$dovecot_pid" "$postfix_pid"
exit $?
