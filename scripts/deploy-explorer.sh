#!/usr/bin/env bash
# Publishes explore.the-coin.cloud on the website machine from this git checkout.
#
# First time (copies the files, configures nginx and gets the HTTPS certificate):
#   sudo scripts/deploy-explorer.sh --setup
#
# Updates (after `git pull`):
#   sudo scripts/deploy-explorer.sh
#
# Optional: a different web root as the last argument (default /var/www/explore.the-coin.cloud).
set -euo pipefail
cd "$(dirname "$0")/.."

DOMAIN=explore.the-coin.cloud
SETUP=0
if [ "${1:-}" = "--setup" ]; then SETUP=1; shift; fi
WEBROOT="${1:-/var/www/$DOMAIN}"
SITE_CONF=/etc/nginx/sites-available/$DOMAIN

info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ -f explore/index.html ] || die "run from the repository (explore/ not found)"
mkdir -p "$WEBROOT" || die "cannot create $WEBROOT (run with sudo)"
[ -w "$WEBROOT" ] || die "cannot write to $WEBROOT (run with sudo)"

info "copying the explorer to $WEBROOT"
if command -v rsync >/dev/null; then
  rsync -a --delete --exclude nginx/ --chmod=D755,F644 explore/ "$WEBROOT/"
else
  rm -rf "${WEBROOT:?}/assets"
  cp -a explore/index.html explore/assets "$WEBROOT/"
  chmod -R a+rX "$WEBROOT"
fi

if [ "$SETUP" = 1 ]; then
  [ "$(id -u)" = 0 ] || die "--setup needs root (sudo)"
  command -v nginx >/dev/null || die "nginx is not installed"
  command -v certbot >/dev/null || die "certbot is not installed (apt install certbot)"
  mkdir -p /var/www/certbot
  if [ ! -f "/etc/letsencrypt/live/$DOMAIN/fullchain.pem" ]; then
    info "temporary HTTP-only site to obtain the certificate"
    cat > "$SITE_CONF" <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $DOMAIN;
    location /.well-known/acme-challenge/ { root /var/www/certbot; }
    location / { return 404; }
}
EOF
    ln -sf "$SITE_CONF" "/etc/nginx/sites-enabled/$DOMAIN"
    nginx -t && systemctl reload nginx
    info "requesting the certificate for $DOMAIN"
    certbot certonly --webroot -w /var/www/certbot -d "$DOMAIN" --non-interactive --agree-tos \
      --register-unsafely-without-email --keep-until-expiring \
      || die "certbot failed: check that $DOMAIN points to this machine and port 80 is open"
  fi
  info "installing the nginx site"
  cp explore/nginx/$DOMAIN.conf "$SITE_CONF"
  if [ "$WEBROOT" != "/var/www/$DOMAIN" ]; then
    sed -i "s|root  /var/www/$DOMAIN;|root  $WEBROOT;|" "$SITE_CONF"
  fi
  ln -sf "$SITE_CONF" "/etc/nginx/sites-enabled/$DOMAIN"
  nginx -t && systemctl reload nginx
fi

echo
info "explore.the-coin.cloud published (commit $(git rev-parse --short HEAD 2>/dev/null || echo unknown))"
echo "Open: https://$DOMAIN/"
