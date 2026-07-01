#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run as root: sudo bash scripts/tencent-lighthouse/bootstrap-ubuntu.sh" >&2
  exit 1
fi

MIMOFAN_USER="${MIMOFAN_USER:-${DEEPSEEK_USER:-mimofan}}"
MIMOFAN_ROOT="${MIMOFAN_ROOT:-${DEEPSEEK_ROOT:-/opt/mimofan}}"
WHALEBRO_ROOT="${WHALEBRO_ROOT:-/opt/whalebro}"
REPO_URL="${MIMOFAN_REPO_URL:-${DEEPSEEK_REPO_URL:-https://github.com/XiaomingX/mimo-tui.git}}"
WHALEBRO_EXTRA_REPOS="${WHALEBRO_EXTRA_REPOS:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SOURCE_BRANCH="$(git -C "${SOURCE_ROOT}" branch --show-current 2>/dev/null || true)"
REPO_BRANCH="${MIMOFAN_REPO_BRANCH:-${DEEPSEEK_REPO_BRANCH:-${SOURCE_BRANCH:-main}}}"

apt-get update
apt-get install -y \
  ca-certificates \
  curl \
  git \
  iproute2 \
  openssh-client \
  build-essential \
  pkg-config \
  libdbus-1-dev \
  libssl-dev \
  nodejs \
  npm \
  rsync \
  tmux \
  fail2ban \
  ufw

node_major="$(node -p "Number(process.versions.node.split('.')[0])")"
if (( node_major < 18 )); then
  echo "Node.js 18+ is required for the phone bridges; install a newer Node.js before running install-services.sh." >&2
fi

if ! id -u "${MIMOFAN_USER}" >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash "${MIMOFAN_USER}"
fi

install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${MIMOFAN_ROOT}"
install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${MIMOFAN_ROOT}/bridge"
install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${MIMOFAN_ROOT}/telegram-bridge"
install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${WHALEBRO_ROOT}"
install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${WHALEBRO_ROOT}/worktrees"
install -d -m 0750 -o root -g "${MIMOFAN_USER}" /etc/mimofan
install -d -m 0700 -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" /var/lib/mimofan-feishu-bridge
install -d -m 0700 -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" /var/lib/mimofan-telegram-bridge

if [[ ! -d "${WHALEBRO_ROOT}/mimofan/.git" ]]; then
  sudo -u "${MIMOFAN_USER}" git clone --branch "${REPO_BRANCH}" "${REPO_URL}" "${WHALEBRO_ROOT}/mimofan"
fi

for repo_spec in ${WHALEBRO_EXTRA_REPOS}; do
  repo_name="${repo_spec%%=*}"
  repo_url="${repo_spec#*=}"
  if [[ -z "${repo_name}" || -z "${repo_url}" || "${repo_name}" == "${repo_url}" ]]; then
    echo "Skipping malformed WHALEBRO_EXTRA_REPOS entry: ${repo_spec}" >&2
    continue
  fi
  if [[ ! -d "${WHALEBRO_ROOT}/${repo_name}/.git" ]]; then
    sudo -u "${MIMOFAN_USER}" git clone "${repo_url}" "${WHALEBRO_ROOT}/${repo_name}" || {
      echo "Warning: failed to clone optional repo ${repo_name} from ${repo_url}" >&2
    }
  fi
done

if [[ ! -f /etc/mimofan/runtime.env ]]; then
  cat >/etc/mimofan/runtime.env <<'EOF'
MIMOFAN_RUNTIME_TOKEN=replace-with-long-random-token
MIMOFAN_RUNTIME_PORT=7878
MIMOFAN_RUNTIME_WORKERS=2
MIMOFAN_PROVIDER=deepseek
DEEPSEEK_API_KEY=replace-with-provider-key
RUST_LOG=info
EOF
  chown root:"${MIMOFAN_USER}" /etc/mimofan/runtime.env
  chmod 0640 /etc/mimofan/runtime.env
fi

if [[ ! -f /etc/mimofan/feishu-bridge.env ]]; then
  cat >/etc/mimofan/feishu-bridge.env <<'EOF'
FEISHU_APP_ID=cli_xxxxxxxxxxxxxxxx
FEISHU_APP_SECRET=replace-with-app-secret
FEISHU_DOMAIN=feishu
MIMOFAN_RUNTIME_URL=http://127.0.0.1:7878
MIMOFAN_RUNTIME_TOKEN=replace-with-same-token-as-runtime-env
MIMOFAN_WORKSPACE=/opt/whalebro
MIMOFAN_MODEL=auto
MIMOFAN_MODE=agent
MIMOFAN_ALLOW_SHELL=true
MIMOFAN_TRUST_MODE=false
MIMOFAN_AUTO_APPROVE=false
MIMOFAN_CHAT_ALLOWLIST=
MIMOFAN_ALLOW_UNLISTED=false
FEISHU_THREAD_MAP_PATH=/var/lib/mimofan-feishu-bridge/thread-map.json
FEISHU_ALLOW_GROUPS=false
FEISHU_REQUIRE_PREFIX_IN_GROUP=true
FEISHU_GROUP_PREFIX=/cw
FEISHU_MAX_REPLY_CHARS=3500
MIMOFAN_TURN_TIMEOUT_MS=900000
EOF
  chown root:"${MIMOFAN_USER}" /etc/mimofan/feishu-bridge.env
  chmod 0640 /etc/mimofan/feishu-bridge.env
fi

ufw allow OpenSSH
ufw --force enable

cat <<EOF

Base server setup complete.

Next:
1. Install Rust 1.88+ for ${MIMOFAN_USER}; rustup is the usual path.
2. Build/install both binaries:
   sudo -iu ${MIMOFAN_USER}
   cd ${WHALEBRO_ROOT}/mimofan
   cargo install --path crates/cli --locked --force
   cargo install --path crates/tui --locked --force
3. Copy integrations/feishu-bridge or integrations/telegram-bridge to ${MIMOFAN_ROOT} and run pnpm install.
4. Edit /etc/mimofan/runtime.env and the selected bridge env file.
5. Install systemd units with scripts/tencent-lighthouse/install-services.sh.
6. After the env files are edited and services are started, run:
   sudo bash scripts/tencent-lighthouse/doctor.sh

EOF
