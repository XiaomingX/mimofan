#!/usr/bin/env bash
# EXPERIMENTAL — one-shot Mimofan + Telegram bridge setup for a fresh
# AWS Lightsail Ubuntu 24.04 VM (issue #1990 smoke lane).
#
# Run ON THE VM as root:
#   sudo SECRETS_FILE=/tmp/cw-secrets.env bash setup-vm.sh
#
# SECRETS_FILE is a chmod-600 env file you scp'd up, containing:
#   TELEGRAM_BOT_TOKEN=...            # from @BotFather
#   MIMOFAN_PROVIDER=deepseek       # or arcee / xiaomi-mimo / ...
#   PROVIDER_KEY_NAME=DEEPSEEK_API_KEY
#   PROVIDER_KEY_VALUE=...
#   TELEGRAM_CHAT_ALLOWLIST=123456789 # optional; empty = first-pairing mode
# The file is shredded after the values land in /etc/mimofan/*.env.
#
# Reuses the repo's existing provider-agnostic scripts:
#   scripts/tencent-lighthouse/bootstrap-ubuntu.sh
#   scripts/tencent-lighthouse/install-services.sh (MIMOFAN_BRIDGE=telegram)
#   scripts/tencent-lighthouse/doctor.sh
# Uses prebuilt release binaries instead of a Rust build.
set -euo pipefail

RELEASE_TAG="${RELEASE_TAG:-v0.8.57}"
REPO_URL="${REPO_URL:-https://github.com/XiaomingX/mimo-tui.git}"
REPO_BRANCH="${REPO_BRANCH:-main}"
SECRETS_FILE="${SECRETS_FILE:-/tmp/cw-secrets.env}"

[[ "$EUID" -eq 0 ]] || { echo "run as root (sudo)" >&2; exit 1; }
[[ -f "$SECRETS_FILE" ]] || { echo "SECRETS_FILE not found: $SECRETS_FILE" >&2; exit 1; }

# shellcheck disable=SC1090
. "$SECRETS_FILE"
: "${TELEGRAM_BOT_TOKEN:?missing in SECRETS_FILE}"
: "${MIMOFAN_PROVIDER:?missing in SECRETS_FILE}"
: "${PROVIDER_KEY_NAME:?missing in SECRETS_FILE}"
: "${PROVIDER_KEY_VALUE:?missing in SECRETS_FILE}"
TELEGRAM_CHAT_ALLOWLIST="${TELEGRAM_CHAT_ALLOWLIST:-}"

echo "== [1/8] clone repo (${REPO_BRANCH}) =="
apt-get update -q
apt-get install -y -q git curl ca-certificates
if [[ ! -d /tmp/mimofan/.git ]]; then
  git clone --depth 1 --branch "$REPO_BRANCH" "$REPO_URL" /tmp/mimofan
fi

echo "== [2/8] bootstrap (user, dirs, packages, ufw, env skeletons) =="
MIMOFAN_REPO_URL="$REPO_URL" MIMOFAN_REPO_BRANCH="$REPO_BRANCH" \
  bash /tmp/mimofan/scripts/tencent-lighthouse/bootstrap-ubuntu.sh

echo "== [3/8] install prebuilt ${RELEASE_TAG} binaries (no Rust build) =="
# The systemd unit hardcodes /home/mimofan/.cargo/bin/mimofan, so we put
# the release binaries exactly there.
BIN_DIR=/home/mimofan/.cargo/bin
install -d -o mimofan -g mimofan "$BIN_DIR"
BASE="https://github.com/XiaomingX/mimo-tui/releases/download/${RELEASE_TAG}"
TMP=$(mktemp -d)
curl -fsSL -o "$TMP/mimofan" "$BASE/mimofan-linux-x64"
curl -fsSL -o "$TMP/mimofan-tui" "$BASE/mimofan-tui-linux-x64"
curl -fsSL -o "$TMP/sha256.txt" "$BASE/mimofan-artifacts-sha256.txt"
( cd "$TMP"
  grep -E ' (mimofan|mimofan-tui)-linux-x64$' sha256.txt \
    | sed 's/mimofan-linux-x64/mimofan/; s/mimofan-tui-linux-x64/mimofan-tui/' \
    | sha256sum -c - )
install -m 0755 -o mimofan -g mimofan "$TMP/mimofan" "$BIN_DIR/mimofan"
install -m 0755 -o mimofan -g mimofan "$TMP/mimofan-tui" "$BIN_DIR/mimofan-tui"
rm -rf "$TMP"
sudo -u mimofan "$BIN_DIR/mimofan" --version
sudo -u mimofan "$BIN_DIR/mimofan-tui" --version

echo "== [4/8] install services (telegram bridge) =="
MIMOFAN_BRIDGE=telegram bash /tmp/mimofan/scripts/tencent-lighthouse/install-services.sh

echo "== [5/8] write secrets into /etc/mimofan/*.env =="
RUNTIME_ENV=/etc/mimofan/runtime.env
BRIDGE_ENV=/etc/mimofan/telegram-bridge.env
RUNTIME_TOKEN="dst_$(openssl rand -hex 24)"

set_kv() { # file key value  (replace or append; never echoes the value)
  local file="$1" key="$2" value="$3"
  if grep -qE "^${key}=" "$file"; then
    # use | delimiter; tokens never contain |
    sed -i "s|^${key}=.*|${key}=${value}|" "$file"
  else
    printf '%s=%s\n' "$key" "$value" >> "$file"
  fi
}
set_kv "$RUNTIME_ENV" MIMOFAN_RUNTIME_TOKEN "$RUNTIME_TOKEN"
set_kv "$RUNTIME_ENV" MIMOFAN_PROVIDER "$MIMOFAN_PROVIDER"
set_kv "$RUNTIME_ENV" "$PROVIDER_KEY_NAME" "$PROVIDER_KEY_VALUE"
set_kv "$BRIDGE_ENV" MIMOFAN_RUNTIME_TOKEN "$RUNTIME_TOKEN"
set_kv "$BRIDGE_ENV" TELEGRAM_BOT_TOKEN "$TELEGRAM_BOT_TOKEN"
if [[ -n "$TELEGRAM_CHAT_ALLOWLIST" ]]; then
  set_kv "$BRIDGE_ENV" TELEGRAM_CHAT_ALLOWLIST "$TELEGRAM_CHAT_ALLOWLIST"
  set_kv "$BRIDGE_ENV" TELEGRAM_ALLOW_UNLISTED false
else
  echo "[warn] no TELEGRAM_CHAT_ALLOWLIST given: enabling first-pairing mode"
  echo "[warn] (TELEGRAM_ALLOW_UNLISTED=true). DM the bot /status, copy the"
  echo "[warn] chat_id into TELEGRAM_CHAT_ALLOWLIST, set ALLOW_UNLISTED=false,"
  echo "[warn] then: systemctl restart mimofan-telegram-bridge"
  set_kv "$BRIDGE_ENV" TELEGRAM_ALLOW_UNLISTED true
fi
chmod 0640 "$RUNTIME_ENV" "$BRIDGE_ENV"
chown root:mimofan "$RUNTIME_ENV" "$BRIDGE_ENV"
shred -u "$SECRETS_FILE"
echo "secrets written; $SECRETS_FILE shredded"

echo "== [5b/8] install gh CLI (for autonomous agent PR workflow) =="
if ! command -v gh &>/dev/null; then
  apt-get install -y -q software-properties-common
  # cli.github.com recommends the official APT repo for Ubuntu
  (type -p wget &>/dev/null || apt-get install -y -q wget)
  mkdir -p -m 755 /etc/apt/keyrings
  wget -qO- https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    | tee /etc/apt/keyrings/githubcli-archive-keyring.gpg >/dev/null
  chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    | tee /etc/apt/sources.list.d/github-cli.list >/dev/null
  apt-get update -q
  apt-get install -y -q gh
fi

echo "== [5c/8] create 4G swapfile (idempotent) =="
if [[ ! -f /swapfile ]]; then
  fallocate -l 4G /swapfile
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  echo '/swapfile none swap sw 0 0' >> /etc/fstab
  echo "swapfile created and activated"
else
  echo "swapfile already exists, skipping"
fi

echo "== [6/8] pre-create runtime ReadWritePaths (unit fails without them) =="
install -d -o mimofan -g mimofan -m 0700 \
  /home/mimofan/.mimofan /home/mimofan/.deepseek

echo "== [7/8] validate config =="
sudo -u mimofan node /opt/mimofan/telegram-bridge/scripts/validate-config.mjs \
  --env "$BRIDGE_ENV" --runtime-env "$RUNTIME_ENV" \
  --workspace-root /opt/whalebro --check-filesystem

echo "== [8/8] start + doctor =="
systemctl start mimofan-runtime
for _ in $(seq 1 20); do
  curl -fsS --max-time 2 http://127.0.0.1:7878/health >/dev/null 2>&1 && break
  sleep 1
done
curl -fsS --max-time 3 http://127.0.0.1:7878/health; echo
systemctl start mimofan-telegram-bridge
sleep 3
MIMOFAN_BRIDGE=telegram bash /tmp/mimofan/scripts/tencent-lighthouse/doctor.sh

echo
echo "== Setup complete. Phone smoke checklist (docs/REMOTE_VM_US.md): =="
echo "  1. DM the bot: /status"
echo "  2. /menu (tappable controls)"
echo "  3. prompt: summarize git status in /opt/whalebro/mimofan"
echo "  4. /threads then a Resume button"
echo "  5. trigger a shell approval; test Allow/Deny buttons and /allow|/deny"
echo "  6. /interrupt during an active turn"
echo "  7. sudo reboot; confirm both services return: systemctl status mimofan-runtime mimofan-telegram-bridge"
