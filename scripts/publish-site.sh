#!/usr/bin/env bash
# Assembles everything the-coin.cloud serves into one directory:
#
#   <out>/                      website pages and assets
#   <out>/install.sh            node installer
#   <out>/uninstall.sh
#   <out>/whitepaper/the-coin-whitepaper-v0.1.pdf
#   <out>/releases/latest/thecoin-<target>.tar.gz(.sha256)
#   <out>/releases/v<version>/thecoin-<target>.tar.gz(.sha256)
#   <out>/releases/latest/SHA256SUMS
#
# Usage: scripts/publish-site.sh [out-dir]   (default: dist/site)
# Put release archives in dist/ first (scripts/package.sh or the GitHub release).
# Then copy to the web server, e.g.:
#   rsync -av --delete dist/site/ user@site-machine:/var/www/the-coin.cloud/
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-dist/site}"
VERSION=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)

rm -rf "$OUT"
mkdir -p "$OUT/whitepaper" "$OUT/releases/latest" "$OUT/releases/v$VERSION"
cp website/*.html "$OUT/"
cp -r website/assets "$OUT/"
install -m 0644 installer/install.sh installer/uninstall.sh "$OUT/"
cp docs/whitepaper/the-coin-whitepaper-v0.1.pdf "$OUT/whitepaper/"

shopt -s nullglob
archives=(dist/thecoin-*.tar.gz)
if [ ${#archives[@]} -eq 0 ]; then
  echo "warning: no release archives in dist/ — run scripts/package.sh first" >&2
fi
for a in "${archives[@]}"; do
  name=$(basename "$a")
  for d in latest "v$VERSION"; do
    cp "$a" "$OUT/releases/$d/"
    (cd dist && sha256sum "$name") > "$OUT/releases/$d/$name.sha256"
  done
done
if [ ${#archives[@]} -gt 0 ]; then
  for d in latest "v$VERSION"; do
    (cd "$OUT/releases/$d" && sha256sum thecoin-*.tar.gz > SHA256SUMS)
  done
fi
echo "site bundle ready in $OUT ($(du -sh "$OUT" | cut -f1))"
find "$OUT" -maxdepth 3 -type f | sort
