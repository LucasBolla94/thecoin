#!/usr/bin/env bash
# The Coin — uninstaller. Wallet files in user home directories are NEVER removed.
#   curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash            (keeps chain data)
#   curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash -s -- --purge   (also deletes chain data)
set -euo pipefail
PURGE=0
[ "${1:-}" = "--purge" ] && PURGE=1
[ "$(id -u)" = 0 ] || { echo "run as root" >&2; exit 1; }

systemctl disable --now thecoind 2>/dev/null || true
rm -f /etc/systemd/system/thecoind.service
systemctl daemon-reload
rm -f /usr/local/bin/thecoind /usr/local/bin/thecoin-wallet
if [ "$PURGE" = 1 ]; then
  rm -rf /var/lib/thecoin /etc/thecoin
  id thecoin >/dev/null 2>&1 && userdel thecoin || true
  echo "The Coin removed (including blockchain data and config)."
else
  echo "The Coin removed. Blockchain data kept in /var/lib/thecoin and config in /etc/thecoin (use --purge to delete)."
fi
echo "Your wallet files (~/.thecoin/wallet-*.json) were not touched."
