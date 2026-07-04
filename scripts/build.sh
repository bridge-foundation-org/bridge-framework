#!/usr/bin/env bash
#
# Build release Rust binaries and the Vite frontend.
#
# Usage:
#   ./scripts/build.sh

set -euo pipefail
IFS=$'\n\t'

WORK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$WORK_DIR/dist"
cd "$WORK_DIR"

echo "==> cargo build --release --workspace"
cargo build --release --workspace

echo "==> frontend build"
(
  cd frontend
  if [[ -f package-lock.json ]]; then
    npm ci
  else
    npm install
  fi
  npm run build
)

echo "==> staging dist/"
rm -rf "$DIST"
mkdir -p "$DIST/bin" "$DIST/frontend" "$DIST/docs"

cp target/release/bridge.exe "$DIST/bin/" 2>/dev/null || cp target/release/bridge "$DIST/bin/" 2>/dev/null || true
cp target/release/daemon.exe "$DIST/bin/" 2>/dev/null || cp target/release/daemon "$DIST/bin/" 2>/dev/null || true
cp target/release/miniredis.exe "$DIST/bin/" 2>/dev/null || cp target/release/miniredis "$DIST/bin/" 2>/dev/null || true
cp -r frontend/dist/* "$DIST/frontend/"
cp -r docs/* "$DIST/docs/"

cat > "$DIST/README.txt" <<EOF
Bridge Framework build output

bin/       - release CLI (bridge), daemon, and miniredis binaries
frontend/  - static Vite build (serve with any static host)
docs/      - markdown documentation

Run daemon: ./bin/daemon
Run CLI:    ./bin/bridge ping
EOF

echo "==> build complete: $DIST"
