#!/usr/bin/env bash
# The Coin — uninstaller. Wallet files in user home directories are NEVER removed.
#   curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash                  (keeps chain data and config)
#   curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash -s -- --purge    (also deletes chain data and config)
#   sudo thecoin uninstall [--purge]
set -euo pipefail
PURGE=0
[ "${1:-}" = "--purge" ] && PURGE=1
[ "$(id -u)" = 0 ] || { echo "run as root (sudo)" >&2; exit 1; }

# Remember where the wallet lives before config is removed.
WALLET_FILE=""
PASSWORD_FILE=""
if [ -r /etc/thecoin/install.env ]; then
  WALLET_FILE=$(sed -n 's/^THECOIN_WALLET_FILE=//p' /etc/thecoin/install.env | tail -1)
  PASSWORD_FILE=$(sed -n 's/^THECOIN_PASSWORD_FILE=//p' /etc/thecoin/install.env | tail -1)
fi

systemctl disable --now thecoind 2>/dev/null || true
rm -f /etc/systemd/system/thecoind.service
systemctl daemon-reload
rm -f /usr/local/bin/thecoind /usr/local/bin/thecoin-wallet /usr/local/bin/thecoin /usr/local/bin/tccl
rm -rf /usr/local/lib/thecoin
if [ "$PURGE" = 1 ]; then
  rm -rf /var/lib/thecoin /etc/thecoin
  if id thecoin >/dev/null 2>&1; then userdel thecoin || true; fi
  echo "The Coin removed (including blockchain data and configuration)."
else
  echo "The Coin removed. Blockchain data kept in /var/lib/thecoin and config in /etc/thecoin (use --purge to delete)."
fi
echo "Your wallet files were NOT touched."
if [ -n "$WALLET_FILE" ] && [ -f "$WALLET_FILE" ]; then
  echo "  Wallet:          $WALLET_FILE"
fi
if [ -n "$PASSWORD_FILE" ] && [ -f "$PASSWORD_FILE" ]; then
  echo "  Wallet password: $PASSWORD_FILE"
fi
echo "Keep your recovery phrase safe; delete wallet files only after backing it up."
