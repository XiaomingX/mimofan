#!/usr/bin/env bash
# Source into an interactive agent shell (tmux, ssh) to export the provider
# key and set defaults that systemd normally handles via EnvironmentFile=.
#
# Usage (as the mimofan user):
#   . /opt/whalebro/mimofan/scripts/remote-smoke/agent-session.sh
#   mimofan models           # should list deepseek-v4-pro
#   gh auth status             # should show the fine-grained PAT
#
# The runtime.env file is 0640 root:mimofan, readable by the mimofan user.
set -a
# shellcheck disable=SC1091
. /etc/mimofan/runtime.env
set +a
export MIMOFAN_MODEL="${MIMOFAN_MODEL:-deepseek-v4-pro}"
