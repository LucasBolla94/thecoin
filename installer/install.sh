#!/usr/bin/env bash
# =============================================================================
#  The Coin — node installer                         https://the-coin.cloud
#
#  curl -fsSL https://the-coin.cloud/install.sh | sudo bash
#
#  What it does:
#   1. Detects the CPU architecture and downloads the release binaries
#      (thecoind + thecoin-wallet), verifying their SHA-256 checksum.
#      Falls back to building from source when no binary is available.
#   2. Creates the unprivileged system user `thecoin` and /var/lib/thecoin.
#   3. Creates (or asks for) the wallet address that receives mining rewards.
#   4. Writes /etc/thecoin/thecoind.toml and a hardened systemd service.
#   5. Opens the P2P port in ufw (if active) and starts the node.
#
#  Options (also accepted as environment variables):
#   --network <mainnet|testnet>     THECOIN_NETWORK   (default mainnet)
#   --miner-address <tc1...>        THECOIN_MINER_ADDRESS
#   --no-mine                       THECOIN_NO_MINE=1
#   --threads <n>                   THECOIN_THREADS   (0 = cores-1)
#   --version <x.y.z|latest>        THECOIN_VERSION
#   --from-source                   THECOIN_FROM_SOURCE=1
#   --public-api                    THECOIN_PUBLIC_API=1 (bind API on 0.0.0.0)
#   --yes                           non-interactive, accept defaults
# =============================================================================
set -euo pipefail

NETWORK="${THECOIN_NETWORK:-mainnet}"
MINER_ADDRESS="${THECOIN_MINER_ADDRESS:-}"
NO_MINE="${THECOIN_NO_MINE:-0}"
THREADS="${THECOIN_THREADS:-0}"
VERSION="${THECOIN_VERSION:-latest}"
FROM_SOURCE="${THECOIN_FROM_SOURCE:-0}"
PUBLIC_API="${THECOIN_PUBLIC_API:-0}"
ASSUME_YES=0
SITE_RELEASES="${THECOIN_RELEASES_URL:-https://the-coin.cloud/releases}"
GITHUB_REPO="${THECOIN_GITHUB_REPO:-the-coin-cloud/thecoin}"
BIN_DIR=/usr/local/bin
DATA_DIR=/var/lib/thecoin
CONF_DIR=/etc/thecoin
SERVICE_USER=thecoin

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mWARN:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --network) NETWORK="$2"; shift 2 ;;
    --miner-address) MINER_ADDRESS="$2"; shift 2 ;;
    --no-mine) NO_MINE=1; shift ;;
    --threads) THREADS="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --public-api) PUBLIC_API=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

case "$NETWORK" in
  mainnet) P2P_PORT=7333; API_PORT=7334; HRP=tc ;;
  testnet) P2P_PORT=17333; API_PORT=17334; HRP=tct ;;
  *) die "network must be mainnet or testnet" ;;
esac

# Interactive input works even when the script is piped into bash.
TTY=/dev/tty
can_prompt() { [ "$ASSUME_YES" = 0 ] && [ -r "$TTY" ] && [ -w "$TTY" ]; }
ask() { # ask "question" default -> REPLY
  local q="$1" def="${2:-}"
  if can_prompt; then
    printf '%s ' "$q" > "$TTY"; read -r REPLY < "$TTY" || REPLY=""
    [ -z "$REPLY" ] && REPLY="$def"
  else
    REPLY="$def"
  fi
}

bold ""
bold "  ████████ ██   ██ ███████      ██████  ██████  ██ ███    ██"
bold "     ██    ██   ██ ██          ██      ██    ██ ██ ████   ██"
bold "     ██    ███████ █████       ██      ██    ██ ██ ██ ██  ██"
bold "     ██    ██   ██ ██          ██      ██    ██ ██ ██  ██ ██"
bold "     ██    ██   ██ ███████      ██████  ██████  ██ ██   ████"
bold "                    node installer · $NETWORK"
echo

# ---------------------------------------------------------------- checks ----
[ "$(id -u)" = 0 ] || die "run as root: curl -fsSL https://the-coin.cloud/install.sh | sudo bash"
[ "$(uname -s)" = Linux ] || die "this installer supports Linux only (build from source on other systems)"
command -v systemctl >/dev/null || die "systemd is required"
for c in curl tar sha256sum; do command -v "$c" >/dev/null || die "missing command: $c"; done

case "$(uname -m)" in
  x86_64|amd64) TARGET=x86_64-unknown-linux-musl ;;
  aarch64|arm64) TARGET=aarch64-unknown-linux-musl ;;
  *) warn "no prebuilt binary for $(uname -m); building from source"; FROM_SOURCE=1; TARGET="" ;;
esac

MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
SWAP_MB=$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo)
DISK_GB=$(df -Pk / | awk 'NR==2 {print int($4/1024/1024)}')
CPUS=$(nproc)
info "system: ${CPUS} vCPU, ${MEM_MB} MB RAM, ${SWAP_MB} MB swap, ${DISK_GB} GB free disk"
[ "$MEM_MB" -ge 900 ] || warn "less than 1 GB RAM — the node may be slow"
[ "$DISK_GB" -ge 5 ] || warn "less than 5 GB free disk"
if [ "$MEM_MB" -lt 2000 ] && [ "$SWAP_MB" -eq 0 ] && [ ! -e /swapfile ]; then
  ask "No swap found. Create a 1 GB swap file for stability? [Y/n]" "y"
  if [[ "$REPLY" =~ ^[YySs]$ ]]; then
    info "creating /swapfile (1 GB)"
    fallocate -l 1G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile
    grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
  fi
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# ------------------------------------------------------------- binaries ----
download_release() {
  local name="thecoin-${TARGET}.tar.gz" urls=()
  if [ "$VERSION" = latest ]; then
    urls+=("$SITE_RELEASES/latest/$name" "https://github.com/$GITHUB_REPO/releases/latest/download/$name")
  else
    urls+=("$SITE_RELEASES/v$VERSION/$name" "https://github.com/$GITHUB_REPO/releases/download/v$VERSION/$name")
  fi
  for u in "${urls[@]}"; do
    info "downloading $u"
    if curl -fsSL --retry 3 -o "$TMP/$name" "$u" && curl -fsSL --retry 3 -o "$TMP/$name.sha256" "$u.sha256"; then
      (cd "$TMP" && sha256sum -c "$name.sha256" >/dev/null) || die "checksum verification FAILED for $u"
      info "checksum OK"
      tar -xzf "$TMP/$name" -C "$TMP"
      return 0
    fi
  done
  return 1
}

build_from_source() {
  info "building from source (this can take 10-30 minutes on a small VPS)"
  command -v git >/dev/null || { apt-get update -qq && apt-get install -y -qq git; }
  if ! command -v cc >/dev/null; then
    if command -v apt-get >/dev/null; then apt-get update -qq && apt-get install -y -qq build-essential pkg-config;
    elif command -v dnf >/dev/null; then dnf install -y gcc make;
    else die "install a C compiler (gcc) and retry"; fi
  fi
  if ! command -v cargo >/dev/null && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  fi
  export PATH="$HOME/.cargo/bin:$PATH"
  local ref="main"; [ "$VERSION" != latest ] && ref="v$VERSION"
  git clone --depth 1 --branch "$ref" "https://github.com/$GITHUB_REPO.git" "$TMP/src"
  (cd "$TMP/src" && CARGO_BUILD_JOBS=$CPUS cargo build --release --locked -p thecoin-node -p thecoin-wallet)
  mkdir -p "$TMP/thecoin"
  cp "$TMP/src/target/release/thecoind" "$TMP/src/target/release/thecoin-wallet" "$TMP/thecoin/"
}

if [ "$FROM_SOURCE" = 1 ] || ! download_release; then
  build_from_source
fi
SRC_DIR="$TMP/thecoin"
[ -x "$SRC_DIR/thecoind" ] || SRC_DIR=$(dirname "$(find "$TMP" -type f -name thecoind | head -1)")
[ -x "$SRC_DIR/thecoind" ] || die "thecoind binary not found in release"

if systemctl is-active --quiet thecoind 2>/dev/null; then
  info "stopping running node for upgrade"
  systemctl stop thecoind
fi
install -m 0755 "$SRC_DIR/thecoind" "$BIN_DIR/thecoind"
install -m 0755 "$SRC_DIR/thecoin-wallet" "$BIN_DIR/thecoin-wallet"
info "installed $("$BIN_DIR/thecoind" --version) and $("$BIN_DIR/thecoin-wallet" --version)"

# ------------------------------------------------------------ user/dirs ----
if ! id "$SERVICE_USER" >/dev/null 2>&1; then
  useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
fi
install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR"
install -d -m 0755 "$CONF_DIR"

# --------------------------------------------------------------- wallet ----
REAL_USER="${SUDO_USER:-root}"
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
WALLET_FILE="$REAL_HOME/.thecoin/wallet-$NETWORK.json"
CONF_FILE="$CONF_DIR/thecoind.toml"

if [ -z "$MINER_ADDRESS" ] && [ -f "$CONF_FILE" ]; then
  MINER_ADDRESS=$(awk -F'"' '/^address *=/ {print $2; exit}' "$CONF_FILE" || true)
fi

if [ "$NO_MINE" != 1 ] && [ -z "$MINER_ADDRESS" ]; then
  if [ -f "$WALLET_FILE" ]; then
    info "existing wallet found at $WALLET_FILE"
    if can_prompt; then
      echo "Enter the wallet password to read the mining address:" > "$TTY"
      MINER_ADDRESS=$(sudo -u "$REAL_USER" "$BIN_DIR/thecoin-wallet" --network "$NETWORK" -w "$WALLET_FILE" address < "$TTY" 2> "$TTY" | tail -1 || true)
    fi
  elif can_prompt; then
    echo > "$TTY"
    echo "Mining rewards need a wallet address." > "$TTY"
    echo "  [1] Create a new wallet now (recommended)" > "$TTY"
    echo "  [2] I already have an address" > "$TTY"
    echo "  [3] Do not mine" > "$TTY"
    ask "Choose [1/2/3]:" "1"
    case "$REPLY" in
      1)
        sudo -u "$REAL_USER" "$BIN_DIR/thecoin-wallet" --network "$NETWORK" -w "$WALLET_FILE" create < "$TTY" > "$TTY" 2>&1 || die "wallet creation failed"
        echo "Enter the wallet password again to confirm the mining address:" > "$TTY"
        MINER_ADDRESS=$(sudo -u "$REAL_USER" "$BIN_DIR/thecoin-wallet" --network "$NETWORK" -w "$WALLET_FILE" address < "$TTY" 2> "$TTY" | tail -1 || true)
        ;;
      2) ask "Address (${HRP}1...):" ""; MINER_ADDRESS="$REPLY" ;;
      *) NO_MINE=1 ;;
    esac
  else
    warn "no terminal available and no --miner-address given: installing without mining"
    NO_MINE=1
  fi
fi

if [ "$NO_MINE" != 1 ]; then
  [[ "$MINER_ADDRESS" == ${HRP}1* ]] || die "invalid mining address '$MINER_ADDRESS' (expected ${HRP}1...)"
fi

# --------------------------------------------------------------- config ----
API_BIND="127.0.0.1:$API_PORT"; [ "$PUBLIC_API" = 1 ] && API_BIND="0.0.0.0:$API_PORT"
MINING_ENABLED=true; [ "$NO_MINE" = 1 ] && MINING_ENABLED=false
if [ -f "$CONF_FILE" ]; then
  info "keeping existing $CONF_FILE"
  if [ "$NO_MINE" != 1 ] && ! grep -q "^address *= *\"$MINER_ADDRESS\"" "$CONF_FILE"; then
    sed -i "s|^address *=.*|address = \"$MINER_ADDRESS\"|" "$CONF_FILE"
  fi
else
  cat > "$CONF_FILE" <<EOF
# The Coin node configuration — see docs/OPERATIONS.md
network = "$NETWORK"
data_dir = "$DATA_DIR"

[p2p]
listen = "0.0.0.0:$P2P_PORT"
max_inbound = 32
max_outbound = 8

[rpc]
enabled = true
# Keep on 127.0.0.1 unless this node serves the public explorer/website.
listen = "$API_BIND"

[mining]
enabled = $MINING_ENABLED
address = "$MINER_ADDRESS"
# 0 = automatic (CPU cores - 1)
threads = $THREADS
# Governance: proposal ids you support (hex)
signal = []

[storage]
cache_mb = 64
prune = false
address_index = true

[mempool]
max_mb = 32
EOF
  chmod 0644 "$CONF_FILE"
fi

# -------------------------------------------------------------- systemd ----
MEM_HIGH=$(( MEM_MB * 70 / 100 ))
cat > /etc/systemd/system/thecoind.service <<EOF
[Unit]
Description=The Coin full node (thecoind)
Documentation=https://the-coin.cloud/docs.html
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
ExecStart=$BIN_DIR/thecoind --config $CONF_FILE
Restart=on-failure
RestartSec=10
TimeoutStopSec=60
LimitNOFILE=8192
Nice=5
MemoryHigh=${MEM_HIGH}M

# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictNamespaces=true
LockPersonality=true
ReadWritePaths=$DATA_DIR
CapabilityBoundingSet=
AmbientCapabilities=

[Install]
WantedBy=multi-user.target
EOF

if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q "Status: active"; then
  info "opening P2P port $P2P_PORT/tcp in ufw"
  ufw allow "$P2P_PORT/tcp" >/dev/null
fi

systemctl daemon-reload
systemctl enable --now thecoind >/dev/null
info "waiting for the node API..."
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" >/dev/null 2>&1; then break; fi
  sleep 1
done

echo
if curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" >/dev/null 2>&1; then
  HEIGHT=$(curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" | sed -n 's/.*"height":\([0-9]*\).*/\1/p' | head -1)
  bold "✔ The Coin node is running (network: $NETWORK, height: ${HEIGHT:-0})"
else
  warn "the node did not answer yet — check: journalctl -u thecoind -f"
fi
cat <<EOF

  Logs:            journalctl -u thecoind -f
  Status:          systemctl status thecoind
  Node API:        curl http://127.0.0.1:$API_PORT/api/v1/status
  Config:          $CONF_FILE
  Data:            $DATA_DIR
EOF
if [ "$NO_MINE" != 1 ]; then
  echo "  Mining to:       $MINER_ADDRESS"
fi
cat <<EOF
  Wallet:          thecoin-wallet --network $NETWORK balance
  Uninstall:       curl -fsSL https://the-coin.cloud/uninstall.sh | sudo bash

  Keep your recovery phrase offline. Welcome to The Coin!
EOF
