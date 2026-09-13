#!/usr/bin/env bash
# =============================================================================
#  The Coin — node installer                         https://the-coin.cloud
#
#  Become a validator/miner with ONE command (no questions asked):
#
#    curl -fsSL https://the-coin.cloud/install.sh | sudo bash -s -- --yes
#
#  Interactive (asks before creating the wallet):
#
#    curl -fsSL https://the-coin.cloud/install.sh | sudo bash
#
#  What it does:
#   1. Pre-flight checks: OS/arch, systemd, RAM/disk, clock sync, firewall.
#   2. Downloads the release binaries (thecoind + thecoin-wallet) and verifies
#      their SHA-256 checksum. Falls back to building from source.
#   3. Creates the unprivileged system user `thecoin` and /var/lib/thecoin.
#   4. Creates a wallet for the mining rewards (or uses --miner-address).
#      In non-interactive mode the wallet gets a strong random password stored
#      in a root-only file next to it, and the recovery phrase is shown at the end.
#   5. Writes /etc/thecoin/thecoind.toml and a hardened systemd service.
#   6. Installs the `thecoin` helper command and starts the node.
#   Re-running the installer upgrades the binaries and keeps config + wallet.
#
#  Options (also accepted as environment variables):
#   --network <mainnet|testnet>     THECOIN_NETWORK   (default mainnet)
#   --miner-address <tc1...>        THECOIN_MINER_ADDRESS
#   --no-mine                       THECOIN_NO_MINE=1
#   --threads <n>                   THECOIN_THREADS   (0 = cores-1)
#   --version <x.y.z|latest>        THECOIN_VERSION
#   --from-source                   THECOIN_FROM_SOURCE=1
#   --public-api                    THECOIN_PUBLIC_API=1 (bind API on 0.0.0.0)
#   --yes, -y                       non-interactive: never ask, accept defaults
# =============================================================================
set -euo pipefail

NETWORK="${THECOIN_NETWORK:-mainnet}"
MINER_ADDRESS="${THECOIN_MINER_ADDRESS:-}"
NO_MINE="${THECOIN_NO_MINE:-0}"
THREADS="${THECOIN_THREADS:-0}"
VERSION="${THECOIN_VERSION:-latest}"
FROM_SOURCE="${THECOIN_FROM_SOURCE:-0}"
PUBLIC_API="${THECOIN_PUBLIC_API:-0}"
ASSUME_YES="${THECOIN_YES:-0}"
SITE_URL="${THECOIN_SITE_URL:-https://the-coin.cloud}"
SITE_RELEASES="${THECOIN_RELEASES_URL:-$SITE_URL/releases}"
GITHUB_REPO="${THECOIN_GITHUB_REPO:-LucasBolla94/thecoin}"
RAW_GITHUB="https://raw.githubusercontent.com/$GITHUB_REPO/main"
BIN_DIR=/usr/local/bin
LIB_DIR=/usr/local/lib/thecoin
DATA_DIR=/var/lib/thecoin
CONF_DIR=/etc/thecoin
CONF_FILE="$CONF_DIR/thecoind.toml"
INSTALL_ENV="$CONF_DIR/install.env"
SERVICE_USER=thecoin

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
ok()   { printf '\033[32m ✔\033[0m %s\n' "$*"; }
warn() { printf '\033[33mWARN:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --network) NETWORK="${2:?--network needs a value}"; shift 2 ;;
    --miner-address) MINER_ADDRESS="${2:?--miner-address needs a value}"; shift 2 ;;
    --no-mine) NO_MINE=1; shift ;;
    --threads) THREADS="${2:?--threads needs a value}"; shift 2 ;;
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --public-api) PUBLIC_API=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    -h|--help) sed -n '2,36p' "$0" 2>/dev/null || echo "see https://the-coin.cloud/validator.html"; exit 0 ;;
    *) die "unknown option: $1 (use --help)" ;;
  esac
done

case "$NETWORK" in
  mainnet) P2P_PORT=7333; API_PORT=7334; HRP=tc ;;
  testnet) P2P_PORT=17333; API_PORT=17334; HRP=tct ;;
  *) die "network must be mainnet or testnet" ;;
esac
[[ "$THREADS" =~ ^[0-9]+$ ]] || die "--threads must be a number"
VERSION="${VERSION#v}"

# Interactive input works even when the script is piped into bash.
TTY=/dev/tty
can_prompt() { [ "$ASSUME_YES" != 1 ] && { : < "$TTY"; } 2>/dev/null && [ -w "$TTY" ]; }
ask() { # ask "question" default -> REPLY
  local q="$1" def="${2:-}"
  if can_prompt; then
    printf '%s ' "$q" > "$TTY"; read -r REPLY < "$TTY" || REPLY=""
    if [ -z "$REPLY" ]; then REPLY="$def"; fi
  else
    REPLY="$def"
  fi
  return 0
}

bold ""
bold "  ████████ ██   ██ ███████      ██████  ██████  ██ ███    ██"
bold "     ██    ██   ██ ██          ██      ██    ██ ██ ████   ██"
bold "     ██    ███████ █████       ██      ██    ██ ██ ██ ██  ██"
bold "     ██    ██   ██ ██          ██      ██    ██ ██ ██  ██ ██"
bold "     ██    ██   ██ ███████      ██████  ██████  ██ ██   ████"
bold "                    node installer · $NETWORK"
echo

# ------------------------------------------------------------ pre-flight ----
info "pre-flight checks"
[ "$(id -u)" = 0 ] || die "please run as root, e.g.: curl -fsSL $SITE_URL/install.sh | sudo bash -s -- --yes"
[ "$(uname -s)" = Linux ] || die "this installer supports Linux only (on other systems build from source: https://github.com/$GITHUB_REPO)"
command -v systemctl >/dev/null && [ -d /run/systemd/system ] || die "systemd is required (containers without systemd are not supported by the installer)"
for c in curl tar sha256sum awk sed; do command -v "$c" >/dev/null || die "missing command: $c (install it with your package manager and retry)"; done

OS_NAME="Linux"
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  OS_NAME=$(. /etc/os-release && echo "${PRETTY_NAME:-Linux}")
fi
case "$(uname -m)" in
  x86_64|amd64) TARGET=x86_64-unknown-linux-musl ;;
  aarch64|arm64) TARGET=aarch64-unknown-linux-musl ;;
  *) warn "no prebuilt binary for $(uname -m); the node will be built from source"; FROM_SOURCE=1; TARGET="" ;;
esac
ok "system: $OS_NAME, $(uname -m)"

MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
SWAP_MB=$(awk '/SwapTotal/ {print int($2/1024)}' /proc/meminfo)
DISK_GB=$(df -Pk / | awk 'NR==2 {print int($4/1024/1024)}')
CPUS=$(nproc)
ok "resources: ${CPUS} vCPU, ${MEM_MB} MB RAM, ${SWAP_MB} MB swap, ${DISK_GB} GB free disk"
[ "$MEM_MB" -ge 900 ] || warn "less than 1 GB RAM — the node will work but may be slow"
[ "$DISK_GB" -ge 5 ] || warn "less than 5 GB free disk — the blockchain grows over time"

if command -v timedatectl >/dev/null; then
  if [ "$(timedatectl show -p NTPSynchronized --value 2>/dev/null || true)" = yes ]; then
    ok "clock synchronized (NTP)"
  else
    warn "the system clock is not NTP-synchronized. Block timestamps are validated by the network;"
    warn "enable time sync with: sudo timedatectl set-ntp true"
  fi
fi

if [ "$MEM_MB" -lt 2000 ] && [ "$SWAP_MB" -eq 0 ] && [ ! -e /swapfile ]; then
  ask "No swap found. Create a 1 GB swap file for stability? [Y/n]" "y"
  if [[ "$REPLY" =~ ^[YySs] ]]; then
    info "creating /swapfile (1 GB)"
    if fallocate -l 1G /swapfile 2>/dev/null && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile; then
      grep -q '^/swapfile' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
      ok "swap enabled"
    else
      rm -f /swapfile
      warn "could not create swap (continuing without it)"
    fi
  fi
fi

TMP=$(mktemp -d)
chmod 700 "$TMP"
trap 'rm -rf "$TMP"' EXIT

# ------------------------------------------------------------- binaries ----
download_release() {
  [ -n "$TARGET" ] || return 1
  local name="thecoin-${TARGET}.tar.gz" urls=()
  if [ "$VERSION" = latest ]; then
    urls+=("$SITE_RELEASES/latest/$name" "https://github.com/$GITHUB_REPO/releases/latest/download/$name")
  else
    urls+=("$SITE_RELEASES/v$VERSION/$name" "https://github.com/$GITHUB_REPO/releases/download/v$VERSION/$name")
  fi
  for u in "${urls[@]}"; do
    info "downloading $u"
    if curl -fsSL --retry 3 -o "$TMP/$name" "$u" && curl -fsSL --retry 3 -o "$TMP/$name.sha256" "$u.sha256"; then
      (cd "$TMP" && sha256sum -c "$name.sha256" >/dev/null) || die "checksum verification FAILED for $u — refusing to install"
      ok "checksum verified"
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
  cp "$TMP/src/installer/thecoin" "$TMP/src/installer/uninstall.sh" "$TMP/thecoin/" 2>/dev/null || true
}

if [ "$FROM_SOURCE" = 1 ] || ! download_release; then
  build_from_source
fi
SRC_DIR="$TMP/thecoin"
if [ ! -x "$SRC_DIR/thecoind" ]; then
  found=$(find "$TMP" -type f -name thecoind | head -1)
  [ -n "$found" ] || die "thecoind binary not found in the release"
  SRC_DIR=$(dirname "$found")
fi

UPGRADE=0
if systemctl is-active --quiet thecoind 2>/dev/null; then
  info "stopping the running node for the upgrade"
  systemctl stop thecoind
  UPGRADE=1
fi
install -m 0755 "$SRC_DIR/thecoind" "$BIN_DIR/thecoind"
install -m 0755 "$SRC_DIR/thecoin-wallet" "$BIN_DIR/thecoin-wallet"
ok "installed $("$BIN_DIR/thecoind" --version) and $("$BIN_DIR/thecoin-wallet" --version)"

# ------------------------------------------------ helper + uninstaller ----
# Prefer the copy next to this script (same version as the installer), then the
# release archive, then download from the site / GitHub.
fetch_helper() { # fetch_helper <file> <dest>
  local file="$1" dest="$2" script_dir=""
  case "$0" in */*) script_dir=$(cd "$(dirname "$0")" 2>/dev/null && pwd || true) ;; esac
  for candidate in "${script_dir:+$script_dir/$file}" "$SRC_DIR/$file"; do
    if [ -n "$candidate" ] && [ -f "$candidate" ]; then install -m 0755 "$candidate" "$dest"; return 0; fi
  done
  for u in "$SITE_URL/$file" "$RAW_GITHUB/installer/$file"; do
    if curl -fsSL --retry 2 -o "$TMP/$file.dl" "$u" 2>/dev/null && head -1 "$TMP/$file.dl" | grep -q '^#!'; then
      install -m 0755 "$TMP/$file.dl" "$dest"; return 0
    fi
  done
  return 1
}
install -d -m 0755 "$LIB_DIR"
if fetch_helper thecoin "$BIN_DIR/thecoin"; then ok "installed helper command: thecoin"; else warn "could not install the 'thecoin' helper command"; fi
fetch_helper uninstall.sh "$LIB_DIR/uninstall.sh" || warn "could not store the uninstaller locally"

# ------------------------------------------------------------ user/dirs ----
if ! id "$SERVICE_USER" >/dev/null 2>&1; then
  useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin "$SERVICE_USER"
fi
install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$DATA_DIR"
install -d -m 0755 "$CONF_DIR"

# --------------------------------------------------------------- wallet ----
REAL_USER="${SUDO_USER:-root}"
id "$REAL_USER" >/dev/null 2>&1 || REAL_USER=root
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
[ -n "$REAL_HOME" ] || REAL_HOME=/root
WALLET_DIR="$REAL_HOME/.thecoin"
WALLET_FILE="$WALLET_DIR/wallet-$NETWORK.json"
PASSWORD_FILE="$WALLET_DIR/wallet-$NETWORK.password"
WALLET_CREATED=0
MNEMONIC=""

# Runs a command as the wallet owner, keeping THECOIN_WALLET_PASSWORD if set.
as_owner() {
  if [ "$REAL_USER" = root ]; then
    "$@"
  elif command -v runuser >/dev/null; then
    runuser -u "$REAL_USER" -- "$@"
  else
    sudo --preserve-env=THECOIN_WALLET_PASSWORD -u "$REAL_USER" "$@"
  fi
}

wallet() { as_owner "$BIN_DIR/thecoin-wallet" --network "$NETWORK" -w "$WALLET_FILE" "$@"; }

# Non-interactive wallet: random password in a root-only file, phrase shown at the end.
create_wallet_auto() {
  info "creating a wallet for the mining rewards ($WALLET_FILE)"
  as_owner mkdir -p "$WALLET_DIR"
  chmod 700 "$WALLET_DIR"
  local pw out
  pw=$(head -c 32 /dev/urandom | base64 | tr -d '\n=')
  ( umask 077; printf '%s\n' "$pw" > "$PASSWORD_FILE" )
  chown root:root "$PASSWORD_FILE"
  chmod 600 "$PASSWORD_FILE"
  out=$(THECOIN_WALLET_PASSWORD="$pw" wallet create 2>&1) || { rm -f "$PASSWORD_FILE"; die "wallet creation failed: $out"; }
  pw=""
  MINER_ADDRESS=$(printf '%s\n' "$out" | sed -n 's/^Address: *//p' | tail -1)
  MNEMONIC=$(printf '%s\n' "$out" | grep -oE '[0-9]+\. [a-z]+' | awk '{print $2}' | tr '\n' ' ' | sed 's/ $//')
  WALLET_CREATED=1
  ok "wallet created; mining address $MINER_ADDRESS"
}

# Interactive wallet: the user chooses the password.
create_wallet_interactive() {
  info "creating a wallet for the mining rewards ($WALLET_FILE)"
  as_owner mkdir -p "$WALLET_DIR"
  chmod 700 "$WALLET_DIR"
  # Passwords are read from the terminal by the wallet itself; stdout is kept to find the address.
  wallet create < "$TTY" 2> "$TTY" | tee "$TMP/create.out" > "$TTY" || die "wallet creation failed"
  MINER_ADDRESS=$(sed -n 's/^Address: *//p' "$TMP/create.out" | tail -1)
  rm -f "$TMP/create.out"
  WALLET_CREATED=1
}

read_wallet_address() {
  if [ -f "$PASSWORD_FILE" ]; then
    THECOIN_WALLET_PASSWORD="$(cat "$PASSWORD_FILE")" wallet address 2>/dev/null | tail -1 || true
  elif can_prompt; then
    echo "Enter the wallet password to read the mining address:" > "$TTY"
    wallet address < "$TTY" 2> "$TTY" | tail -1 || true
  fi
}

if [ -z "$MINER_ADDRESS" ] && [ -f "$CONF_FILE" ]; then
  # Upgrade: keep what the existing configuration says.
  MINER_ADDRESS=$(awk -F'"' '/^\[/ { sec = $0 } sec == "[mining]" && /^address *=/ { print $2; exit }' "$CONF_FILE" || true)
  CONF_MINING=$(awk '/^\[/ { sec = $0 } sec == "[mining]" && /^enabled *=/ { gsub(/ /, ""); split($0, kv, "="); print kv[2]; exit }' "$CONF_FILE" || true)
  if [ "$CONF_MINING" = false ]; then
    NO_MINE=1
    info "keeping mining disabled as configured in $CONF_FILE (use --miner-address to enable)"
  elif [ -n "$MINER_ADDRESS" ]; then
    info "keeping the mining address from $CONF_FILE"
  fi
fi

if [ "$NO_MINE" != 1 ] && [ -z "$MINER_ADDRESS" ]; then
  if [ -f "$WALLET_FILE" ]; then
    info "existing wallet found at $WALLET_FILE"
    MINER_ADDRESS=$(read_wallet_address)
    if [ -z "$MINER_ADDRESS" ]; then
      warn "could not read the existing wallet without its password; installing without mining."
      warn "re-run with --miner-address ${HRP}1... to enable mining"
      NO_MINE=1
    fi
  elif can_prompt; then
    echo > "$TTY"
    echo "Mining rewards need a wallet address." > "$TTY"
    echo "  [1] Create a new wallet now, with a password I choose (recommended)" > "$TTY"
    echo "  [2] Create a new wallet automatically (random password stored on this machine)" > "$TTY"
    echo "  [3] I already have an address" > "$TTY"
    echo "  [4] Do not mine" > "$TTY"
    ask "Choose [1/2/3/4] (default 1):" "1"
    case "$REPLY" in
      2) create_wallet_auto ;;
      3) ask "Address (${HRP}1...):" ""; MINER_ADDRESS="$REPLY" ;;
      4) NO_MINE=1 ;;
      *) create_wallet_interactive ;;
    esac
  else
    create_wallet_auto
  fi
fi

if [ "$NO_MINE" != 1 ]; then
  [[ "$MINER_ADDRESS" == ${HRP}1* ]] || die "invalid mining address '$MINER_ADDRESS' (expected ${HRP}1...)"
fi

# --------------------------------------------------------------- config ----
API_BIND="127.0.0.1:$API_PORT"; [ "$PUBLIC_API" = 1 ] && API_BIND="0.0.0.0:$API_PORT"
MINING_ENABLED=true; [ "$NO_MINE" = 1 ] && MINING_ENABLED=false
# Sets `key = value` inside the [mining] section of the existing config.
set_mining_key() {
  awk -v key="$1" -v val="$2" '
    /^\[/ { sec = $0 }
    sec == "[mining]" && $0 ~ "^" key " *=" { print key " = " val; next }
    { print }
  ' "$CONF_FILE" > "$CONF_FILE.tmp" && cat "$CONF_FILE.tmp" > "$CONF_FILE" && rm -f "$CONF_FILE.tmp"
}
if [ -f "$CONF_FILE" ]; then
  info "keeping existing $CONF_FILE"
  if [ "$NO_MINE" = 1 ]; then
    set_mining_key enabled false
  elif [ -n "$MINER_ADDRESS" ]; then
    set_mining_key enabled true
    set_mining_key address "\"$MINER_ADDRESS\""
    grep -q '^address *=' "$CONF_FILE" || warn "no 'address' line in $CONF_FILE; add it under [mining]"
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

# Where the helper finds the wallet (no secrets in this file).
cat > "$INSTALL_ENV" <<EOF
# Written by the The Coin installer; used by the 'thecoin' helper command.
THECOIN_NETWORK=$NETWORK
THECOIN_API_PORT=$API_PORT
THECOIN_P2P_PORT=$P2P_PORT
THECOIN_WALLET_OWNER=$REAL_USER
THECOIN_WALLET_FILE=$WALLET_FILE
THECOIN_PASSWORD_FILE=$PASSWORD_FILE
EOF
chmod 0644 "$INSTALL_ENV"

# -------------------------------------------------------------- systemd ----
MEM_HIGH=$(( MEM_MB * 70 / 100 ))
cat > /etc/systemd/system/thecoind.service <<EOF
[Unit]
Description=The Coin full node (thecoind)
Documentation=https://the-coin.cloud/validator.html
After=network-online.target time-sync.target
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

# ------------------------------------------------------------- firewall ----
FIREWALL_NOTE=""
if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q "Status: active"; then
  info "opening P2P port $P2P_PORT/tcp in ufw"
  ufw allow "$P2P_PORT/tcp" >/dev/null
  ok "ufw: port $P2P_PORT/tcp allowed"
elif command -v ufw >/dev/null; then
  FIREWALL_NOTE="ufw is installed but inactive. If you enable it later, first run: sudo ufw allow $P2P_PORT/tcp"
else
  FIREWALL_NOTE="no ufw found. If you use another firewall (iptables/nftables/firewalld), allow TCP port $P2P_PORT."
fi

systemctl daemon-reload
if [ "$UPGRADE" = 1 ]; then
  systemctl enable thecoind >/dev/null 2>&1
  systemctl restart thecoind
else
  systemctl enable --now thecoind >/dev/null 2>&1
fi
info "waiting for the node API..."
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" >/dev/null 2>&1; then break; fi
  sleep 1
done

# ------------------------------------------------------------------ done ----
echo
if curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" >/dev/null 2>&1; then
  HEIGHT=$(curl -fsS "http://127.0.0.1:$API_PORT/api/v1/status" | sed -n 's/.*"height":\([0-9]*\).*/\1/p' | head -1)
  bold "✔ The Coin node is running (network: $NETWORK, height: ${HEIGHT:-0})"
else
  warn "the node did not answer yet — check: thecoin logs"
fi
echo
echo "  Config:          $CONF_FILE"
echo "  Data:            $DATA_DIR"
if [ "$NO_MINE" != 1 ]; then
  echo "  Mining to:       $MINER_ADDRESS"
else
  echo "  Mining:          disabled (validating and relaying only)"
fi
if [ -f "$WALLET_FILE" ]; then
  echo "  Wallet:          $WALLET_FILE"
fi
if [ -f "$PASSWORD_FILE" ]; then
  echo "  Wallet password: stored in $PASSWORD_FILE (root only)"
fi

if [ "$WALLET_CREATED" = 1 ] && [ -n "$MNEMONIC" ]; then
  # Plain ASCII so the frame lines up in any terminal and locale.
  box_line() { printf '  | %-72s |\n' "$1"; }
  rule='  +--------------------------------------------------------------------------+'
  echo
  printf '\033[1;33m'
  echo "$rule"
  box_line "!!  RECOVERY PHRASE - WRITE THESE WORDS ON PAPER, KEEP THEM OFFLINE  !!"
  box_line ""
  box_line "Anyone with these words controls your coins. Nobody can recover them"
  box_line "for you. Show them again at any time with:   sudo thecoin mnemonic"
  echo "$rule"
  i=0; row=""
  for w in $MNEMONIC; do
    i=$((i + 1))
    row="$row$(printf '%2d. %-13s' "$i" "$w")"
    if [ $((i % 4)) -eq 0 ]; then box_line "$row"; row=""; fi
  done
  if [ -n "$row" ]; then box_line "$row"; fi
  echo "$rule"
  printf '\033[0m'
  MNEMONIC=""
fi

cat <<EOF

  What now?
    thecoin status        node, sync, mining and balance at a glance
    thecoin logs          live logs (Ctrl+C to exit)
    thecoin balance       wallet balance
    thecoin update        upgrade to the latest version
    Explorer:             https://the-coin.cloud/explorer.html
    Guide:                https://the-coin.cloud/validator.html

  Network: other nodes must reach TCP port $P2P_PORT on this machine.
  On cloud providers (AWS, GCP, Azure, Oracle, OVH, Hetzner...) also allow it
  in the provider's firewall / security group.
EOF
[ -n "$FIREWALL_NOTE" ] && echo "  $FIREWALL_NOTE"
echo
echo "  Welcome to The Coin!"
