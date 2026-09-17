#!/usr/bin/env bash
# Builds release binaries and creates distributable archives in dist/:
#   dist/thecoin-<target>.tar.gz and dist/thecoin-<target>.tar.gz.sha256
#
# Usage: scripts/package.sh [target]    (default: host target)
#   e.g. scripts/package.sh aarch64-unknown-linux-gnu   (needs: rustup target add ... + a C++ cross toolchain,
#   because the RandomX proof of work is a C++ library built with cmake)
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${1:-}"
# The tccl command-line tool lives in the language repository; build it from
# the tag pinned in Cargo.toml.
TCCL_TAG=$(awk -F'"' '/^tccl *=/ {for (i = 1; i < NF; i++) if ($i ~ /tag *= *$/) print $(i + 1)}' Cargo.toml)
[ -n "$TCCL_TAG" ] || { echo "cannot read the tccl tag from Cargo.toml" >&2; exit 1; }
TCCL_SRC="target/tccl-src/$TCCL_TAG"
[ -d "$TCCL_SRC" ] || git clone -q --depth 1 --branch "$TCCL_TAG" https://github.com/LucasBolla94/tccl "$TCCL_SRC"
if [ -n "$TARGET" ]; then
  cargo build --release --locked --target "$TARGET" -p thecoin-node -p thecoin-wallet
  cargo build --release --locked --target "$TARGET" --manifest-path "$TCCL_SRC/Cargo.toml" -p tccl-cli
  OUT="target/$TARGET/release"
  TCCL_OUT="$TCCL_SRC/target/$TARGET/release"
else
  cargo build --release --locked -p thecoin-node -p thecoin-wallet
  cargo build --release --locked --manifest-path "$TCCL_SRC/Cargo.toml" -p tccl-cli
  OUT="target/release"
  TCCL_OUT="$TCCL_SRC/target/release"
  TARGET=$(rustc -vV | awk '/host:/ {print $2}')
fi
VERSION=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
STAGE="dist/stage/thecoin"
rm -rf dist/stage && mkdir -p "$STAGE"
cp "$OUT/thecoind" "$OUT/thecoin-wallet" "$TCCL_OUT/tccl" "$STAGE/"
cp README.md LICENSE-MIT LICENSE-APACHE installer/install.sh installer/uninstall.sh installer/thecoin "$STAGE/" 2>/dev/null || true
echo "$VERSION" > "$STAGE/VERSION"
NAME="thecoin-$TARGET.tar.gz"
tar -C dist/stage -czf "dist/$NAME" thecoin
(cd dist && sha256sum "$NAME" > "$NAME.sha256")
rm -rf dist/stage
echo "created dist/$NAME ($(du -h "dist/$NAME" | cut -f1))"
cat "dist/$NAME.sha256"
