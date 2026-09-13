#!/usr/bin/env bash
# Publishes the-coin.cloud on the website machine, straight from this git checkout.
#
#   git pull
#   sudo scripts/deploy-site.sh                 # default web root: /var/www/the-coin.cloud
#   sudo scripts/deploy-site.sh /srv/www/site   # another web root
#
# It assembles pages, assets, installer, helper, whitepapers, TCCL cookbook and
# examples (scripts/publish-site.sh), downloads the release binaries of the
# current version from GitHub (if that release exists) and verifies their
# SHA-256, then copies everything into the web root. Files already in the web
# root that are not part of the bundle (e.g. older releases) are kept.
set -euo pipefail
cd "$(dirname "$0")/.."

WEBROOT="${1:-/var/www/the-coin.cloud}"
REPO="${THECOIN_GITHUB_REPO:-LucasBolla94/thecoin}"
VERSION=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)

info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mWARN:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ -d "$WEBROOT" ] || die "web root $WEBROOT does not exist (create it or pass the right path)"
[ -w "$WEBROOT" ] || die "cannot write to $WEBROOT (run with sudo)"
for c in curl sha256sum awk; do command -v "$c" >/dev/null || die "missing command: $c"; done

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

info "assembling the site bundle (version $VERSION, commit $(git rev-parse --short HEAD 2>/dev/null || echo unknown))"
scripts/publish-site.sh "$TMP/site" >/dev/null 2>"$TMP/publish.log" || { cat "$TMP/publish.log" >&2; die "bundle failed"; }

# Release binaries from GitHub (published by .github/workflows/release.yml on tag v$VERSION).
REL="https://github.com/$REPO/releases/download/v$VERSION"
if curl -fsSL --retry 2 -o "$TMP/SHA256SUMS" "$REL/SHA256SUMS" 2>/dev/null; then
  info "downloading release binaries v$VERSION from GitHub"
  mkdir -p "$TMP/releases"
  while read -r sum name; do
    [ -n "$name" ] || continue
    curl -fsSL --retry 3 -o "$TMP/releases/$name" "$REL/$name" || die "download failed: $name"
    echo "$sum  $name" > "$TMP/releases/$name.sha256"
  done < "$TMP/SHA256SUMS"
  (cd "$TMP/releases" && sha256sum -c ../SHA256SUMS >/dev/null) || die "checksum verification FAILED — nothing was published"
  for d in latest "v$VERSION"; do
    mkdir -p "$TMP/site/releases/$d"
    cp "$TMP/releases/"* "$TMP/SHA256SUMS" "$TMP/site/releases/$d/"
  done
else
  warn "release v$VERSION not found on GitHub: /releases/ is not updated (the installer will build from source until the release exists)"
fi

info "copying into $WEBROOT"
if command -v rsync >/dev/null; then
  rsync -a --chmod=D755,F644 "$TMP/site/" "$WEBROOT/"
else
  cp -a "$TMP/site/." "$WEBROOT/"
  chmod -R a+rX "$WEBROOT"
fi

echo
info "published:"
for p in index.html tccl.html validator.html install.sh thecoin whitepaper/the-coin-whitepaper-v0.2.pdf tccl/tccl-cookbook.pdf; do
  if [ -f "$WEBROOT/$p" ]; then echo "  ✔ /$p"; else echo "  ✘ /$p (missing)"; fi
done
if [ -d "$WEBROOT/releases/latest" ]; then
  echo "  ✔ /releases/latest: $(find "$WEBROOT/releases/latest" -name '*.tar.gz' | wc -l) archive(s)"
fi
echo
echo "Check in a browser (hard refresh): https://the-coin.cloud/"
echo "If the nginx config changed in website/nginx/, install it and run: sudo nginx -t && sudo systemctl reload nginx"
