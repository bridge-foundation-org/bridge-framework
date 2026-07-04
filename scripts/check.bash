#!/usr/bin/env bash
#
# Run the same checks as Bridge CI: Rust tests + frontend build + e2e.
#
# Usage:
#   ./check.bash [--all]
#
# Examples:
#   ./check.bash
#   ./check.bash --all

set -euo pipefail
IFS=$'\n\t'

WORK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$WORK_DIR"

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> cargo build --workspace"
cargo build --workspace

echo "==> frontend npm ci/build"
(
  cd frontend
  if [[ -f package-lock.json ]]; then
    npm ci
  else
    npm install
  fi
  npm run build
)

if [[ "${1:-}" == "--all" ]]; then
  echo "==> e2e tests"
  cargo test -p e2e-tests -- --test-threads=1
fi

echo "==> check passed"
