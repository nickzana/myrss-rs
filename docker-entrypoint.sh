#!/bin/sh
set -eu

# Bind-mounted data dirs are often owned by a host uid; make them writable.
mkdir -p /var/lib/myrss/data
chown -R myrss:myrss /var/lib/myrss 2>/dev/null || true

mkdir -p /run/myrss-secrets
if [ -d /run/secrets ]; then
  for src in /run/secrets/*; do
    [ -f "$src" ] || continue
    install -o myrss -g myrss -m 600 "$src" "/run/myrss-secrets/$(basename "$src")"
  done
fi

exec setpriv --reuid=myrss --regid=myrss --clear-groups /usr/local/bin/myrss "$@"
