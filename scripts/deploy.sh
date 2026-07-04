#!/usr/bin/env bash
#
# Build and package a deploy bundle for self-hosting.
#
# Usage:
#   ./scripts/deploy.sh [output-dir]
#
# Default output: ./deploy-bundle

set -euo pipefail
IFS=$'\n\t'

WORK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$WORK_DIR/deploy-bundle}"
cd "$WORK_DIR"

"$WORK_DIR/scripts/build.sh"

echo "==> packaging deploy bundle at $OUT"
rm -rf "$OUT"
mkdir -p "$OUT"
cp -r dist/* "$OUT/"

cat > "$OUT/deploy.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export BRIDGE_TCP_ADDR="${BRIDGE_TCP_ADDR:-127.0.0.1:7878}"
export BRIDGE_HTTP_ADDR="${BRIDGE_HTTP_ADDR:-127.0.0.1:8787}"
echo "Starting Bridge daemon on $BRIDGE_TCP_ADDR (HTTP $BRIDGE_HTTP_ADDR)"
exec "$DIR/bin/daemon"
EOF
chmod +x "$OUT/deploy.sh" 2>/dev/null || true

if command -v tar >/dev/null 2>&1; then
  tar -czf "$WORK_DIR/bridge-deploy-bundle.tar.gz" -C "$OUT" .
  echo "==> archive: $WORK_DIR/bridge-deploy-bundle.tar.gz"
fi

echo "==> deploy bundle ready"
echo "    1. Copy $OUT to your server"
echo "    2. Run ./deploy.sh to start the daemon"
echo "    3. Serve frontend/ with nginx, Caddy, or any static host"
