#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run as root: sudo bash scripts/tencent-lighthouse/install-services.sh" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MIMOFAN_USER="${MIMOFAN_USER:-${DEEPSEEK_USER:-mimofan}}"
MIMOFAN_ROOT="${MIMOFAN_ROOT:-${DEEPSEEK_ROOT:-/opt/mimofan}}"
BRIDGE_KIND="${MIMOFAN_BRIDGE:-${DEEPSEEK_BRIDGE:-feishu}}"

case "${BRIDGE_KIND}" in
  feishu|lark)
    BRIDGE_SRC="integrations/feishu-bridge"
    BRIDGE_DST="${MIMOFAN_ROOT}/bridge"
    BRIDGE_UNIT="mimofan-feishu-bridge.service"
    BRIDGE_ENV="/etc/mimofan/feishu-bridge.env"
    BRIDGE_ENV_EXAMPLE="deploy/tencent-lighthouse/examples/feishu-bridge.env.example"
    BRIDGE_STATE_DIR="/var/lib/mimofan-feishu-bridge"
    VALIDATOR="integrations/feishu-bridge/scripts/validate-config.mjs"
    ;;
  telegram)
    BRIDGE_SRC="integrations/telegram-bridge"
    BRIDGE_DST="${MIMOFAN_ROOT}/telegram-bridge"
    BRIDGE_UNIT="mimofan-telegram-bridge.service"
    BRIDGE_ENV="/etc/mimofan/telegram-bridge.env"
    BRIDGE_ENV_EXAMPLE="deploy/tencent-lighthouse/examples/telegram-bridge.env.example"
    BRIDGE_STATE_DIR="/var/lib/mimofan-telegram-bridge"
    VALIDATOR="integrations/telegram-bridge/scripts/validate-config.mjs"
    ;;
  *)
    echo "Unknown bridge '${BRIDGE_KIND}'. Use MIMOFAN_BRIDGE=feishu or MIMOFAN_BRIDGE=telegram." >&2
    exit 1
    ;;
esac

install -d -m 0750 -o root -g "${MIMOFAN_USER}" /etc/mimofan
install -d -m 0700 -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${BRIDGE_STATE_DIR}"
install -d -o "${MIMOFAN_USER}" -g "${MIMOFAN_USER}" "${BRIDGE_DST}"

if [[ ! -f /etc/mimofan/runtime.env && -f "${REPO_ROOT}/deploy/tencent-lighthouse/examples/runtime.env.example" ]]; then
  install -m 0640 -o root -g "${MIMOFAN_USER}" \
    "${REPO_ROOT}/deploy/tencent-lighthouse/examples/runtime.env.example" \
    /etc/mimofan/runtime.env
fi

if [[ ! -f "${BRIDGE_ENV}" && -f "${REPO_ROOT}/${BRIDGE_ENV_EXAMPLE}" ]]; then
  install -m 0640 -o root -g "${MIMOFAN_USER}" \
    "${REPO_ROOT}/${BRIDGE_ENV_EXAMPLE}" \
    "${BRIDGE_ENV}"
fi
rsync -a --delete \
  --exclude node_modules \
  "${REPO_ROOT}/${BRIDGE_SRC}/" \
  "${BRIDGE_DST}/"
chown -R "${MIMOFAN_USER}:${MIMOFAN_USER}" "${BRIDGE_DST}"

if [[ -f "${BRIDGE_DST}/package-lock.json" ]]; then
  sudo -u "${MIMOFAN_USER}" npm --prefix "${BRIDGE_DST}" ci --omit=dev
else
  sudo -u "${MIMOFAN_USER}" npm --prefix "${BRIDGE_DST}" install --omit=dev
fi

install -m 0644 "${REPO_ROOT}/deploy/tencent-lighthouse/systemd/mimofan-runtime.service" /etc/systemd/system/mimofan-runtime.service
install -m 0644 "${REPO_ROOT}/deploy/tencent-lighthouse/systemd/${BRIDGE_UNIT}" "/etc/systemd/system/${BRIDGE_UNIT}"

systemctl daemon-reload
systemctl enable mimofan-runtime "${BRIDGE_UNIT}"

cat <<'EOF'
Services installed but not started.

Before starting, verify:
  /etc/mimofan/runtime.env
EOF
cat <<EOF
  ${BRIDGE_ENV}
  sudo -u ${MIMOFAN_USER} node ${REPO_ROOT}/${VALIDATOR} --env ${BRIDGE_ENV} --runtime-env /etc/mimofan/runtime.env --workspace-root /opt/whalebro --check-filesystem
Then run:
  sudo systemctl start mimofan-runtime
  sudo systemctl start ${BRIDGE_UNIT}
  sudo MIMOFAN_BRIDGE=${BRIDGE_KIND} bash /opt/whalebro/mimofan/scripts/tencent-lighthouse/doctor.sh
  sudo journalctl -u ${BRIDGE_UNIT} -f
EOF
