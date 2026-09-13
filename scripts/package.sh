#!/usr/bin/env bash
# Builds release binaries and creates distributable archives in dist/:
#   dist/thecoin-<target>.tar.gz and dist/thecoin-<target>.tar.gz.sha256
#
# Usage: scripts/package.sh [target]    (default: host target)
#   e.g. scripts/package.sh x86_64-unknown-linux-musl   (needs: rustup target add ... + musl-tools)
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${1:-}"
if [ -n "$TARGET" ]; then
  cargo build --release --locked --target "$TARGET" -p thecoin-node -p thecoin-wallet
  OUT="target/$TARGET/release"
else
  cargo build --release --locked -p thecoin-node -p thecoin-wallet
  OUT="target/release"
  TARGET=$(rustc -vV | awk '/host:/ {print $2}')
fi
VERSION=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
STAGE="dist/stage/thecoin"
rm -rf dist/stage && mkdir -p "$STAGE"
cp "$OUT/thecoind" "$OUT/thecoin-wallet" "$STAGE/"
cp README.md LICENSE-MIT LICENSE-APACHE installer/install.sh installer/uninstall.sh "$STAGE/" 2>/dev/null || true
echo "$VERSION" > "$STAGE/VERSION"
NAME="thecoin-$TARGET.tar.gz"
tar -C dist/stage -czf "dist/$NAME" thecoin
(cd dist && sha256sum "$NAME" > "$NAME.sha256")
rm -rf dist/stage
echo "created dist/$NAME ($(du -h "dist/$NAME" | cut -f1))"
cat "dist/$NAME.sha256"
