#!/usr/bin/env bash
# Hosts the website (the-coin.cloud) and the explorer (explore.the-coin.cloud)
# on THIS machine, which also runs a node. Use it when the site lives on the
# same server as a seed node.
#
#   sudo scripts/host-site-here.sh            # files + nginx over HTTP
#   sudo scripts/host-site-here.sh --tls      # the same, then HTTPS certificates
#                                             # (needs the DNS records already
#                                             #  pointing at this machine)
#
# Idempotent: run it again after `git pull` to update the published files.
set -euo pipefail
cd "$(dirname "$0")/.."

SITE=the-coin.cloud
EXPLORER=explore.$SITE
WITH_TLS=0
[ "${1:-}" = "--tls" ] && WITH_TLS=1

info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mWARN:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "run with sudo"
command -v nginx >/dev/null || die "nginx is not installed"

# ---- files ----------------------------------------------------------------
info "publishing the website to /var/www/$SITE"
mkdir -p "/var/www/$SITE" /var/www/certbot /var/cache/nginx/thecoin_api
scripts/deploy-site.sh "/var/www/$SITE" >/dev/null
info "publishing the explorer to /var/www/$EXPLORER"
scripts/deploy-explorer.sh "/var/www/$EXPLORER" >/dev/null

# ---- nginx ----------------------------------------------------------------
install -m 0644 website/nginx/snippets/thecoin-proxy.conf /etc/nginx/snippets/thecoin-proxy.conf
http_only() { # http_only <domain> <root>
  cat > "/etc/nginx/sites-available/$1" <<EOF
server {
    listen 80;
    listen [::]:80;
    server_name $1${3:+ $3};
    root /var/www/$2;
    index index.html;
    charset utf-8;
    location /.well-known/acme-challenge/ { root /var/www/certbot; }
    location / { try_files \$uri \$uri/ =404; }
    location = /install.sh   { default_type text/plain; }
    location = /uninstall.sh { default_type text/plain; }
    location /releases/  { autoindex on; }
    location /whitepaper/ { autoindex on; types { application/pdf pdf; } }
    location /tccl/ { autoindex on; charset utf-8; types { application/pdf pdf; text/plain tccl; } default_type text/plain; }
    location /api/ {
        proxy_pass http://127.0.0.1:17334;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
    }
}
EOF
  ln -sf "/etc/nginx/sites-available/$1" "/etc/nginx/sites-enabled/$1"
}

if [ "$WITH_TLS" = 1 ] && [ -f "/etc/letsencrypt/live/$SITE/fullchain.pem" ]; then
  info "installing the HTTPS sites"
  install -m 0644 website/nginx/the-coin.cloud-selfhosted.conf "/etc/nginx/sites-available/$SITE"
  install -m 0644 explore/nginx/$EXPLORER.conf "/etc/nginx/sites-available/$EXPLORER"
  ln -sf "/etc/nginx/sites-available/$SITE" "/etc/nginx/sites-enabled/$SITE"
  ln -sf "/etc/nginx/sites-available/$EXPLORER" "/etc/nginx/sites-enabled/$EXPLORER"
else
  info "installing the sites over HTTP"
  http_only "$SITE" "$SITE" "www.$SITE"
  http_only "$EXPLORER" "$EXPLORER"
fi
nginx -t && systemctl reload nginx

# ---- certificates ---------------------------------------------------------
if [ "$WITH_TLS" = 1 ] && [ ! -f "/etc/letsencrypt/live/$SITE/fullchain.pem" ]; then
  command -v certbot >/dev/null || die "certbot is not installed (apt install certbot)"
  myip=$(curl -s -4 --max-time 10 https://ifconfig.me || true)
  for d in "$SITE" "www.$SITE" "$EXPLORER"; do
    got=$(getent ahostsv4 "$d" | awk '{print $1}' | head -1)
    [ "$got" = "$myip" ] || warn "$d points to ${got:-nothing} and this machine is $myip — the certificate for it will fail"
  done
  info "requesting certificates"
  certbot certonly --webroot -w /var/www/certbot -d "$SITE" -d "www.$SITE" \
    --non-interactive --agree-tos --register-unsafely-without-email --keep-until-expiring || die "certbot failed for $SITE"
  certbot certonly --webroot -w /var/www/certbot -d "$EXPLORER" \
    --non-interactive --agree-tos --register-unsafely-without-email --keep-until-expiring || die "certbot failed for $EXPLORER"
  exec "$0" --tls
fi

echo
info "published from commit $(git -c safe.directory="$PWD" rev-parse --short HEAD 2>/dev/null || echo unknown)"
echo "  https://$SITE      (or http:// until the certificates exist)"
echo "  https://$EXPLORER"
