#!/bin/sh
# Smoke test: boot the built image the way a self-hoster does — minimal config,
# MYRSS_PUBLIC_URL via env, and a bind-mounted data dir owned by the host uid.
set -eu
IMG=${1:-myrss:test}
TMP=$(mktemp -d)
CID=""
cleanup() {
  [ -z "$CID" ] || docker rm -f "$CID" >/dev/null 2>&1 || true
  rm -rf "$TMP"
}
trap cleanup EXIT

# No data_dir and no public_url: exercises the ./data default under WORKDIR
# and the MYRSS_PUBLIC_URL env override.
cat > "$TMP/config.toml" <<'EOF'
listen = "0.0.0.0:8080"
[auth]
mode = "proxy"
[auth.proxy]
trusted_peer_cidrs = ["0.0.0.0/0"]
user_header = "X-User"
EOF

# Owned by the host uid, not uid 1000: exercises the entrypoint chown.
mkdir -p "$TMP/data"

CID=$(docker run -d --rm \
  -p 127.0.0.1::8080 \
  -e MYRSS_PUBLIC_URL=http://localhost:8080 \
  -v "$TMP/config.toml:/etc/myrss/config.toml:ro" \
  -v "$TMP/data:/var/lib/myrss" \
  "$IMG")
PORT=$(docker port "$CID" 8080/tcp | head -1 | sed 's/.*://')

ok=""
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
    ok=1
    break
  fi
  sleep 1
done
if [ -z "$ok" ]; then
  docker logs "$CID" >&2 || true
  echo "smoke: /health never responded" >&2
  exit 1
fi

# The default data_dir must land inside the mounted volume.
docker exec "$CID" test -f /var/lib/myrss/data/myrss.db \
  || { echo "smoke: database not created under /var/lib/myrss/data" >&2; exit 1; }

echo "smoke ok"
