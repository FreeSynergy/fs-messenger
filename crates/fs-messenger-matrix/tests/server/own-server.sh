#!/usr/bin/env bash
# A tuwunel of its own for these tests, so that another agent's up.sh or down.sh cannot take it away.
# Same pinned image and settings as scripts/test-servers/up.sh; another name, another port, no Stalwart.
#
#   bash own-server.sh up     # then: FS_MSGTEST_MATRIX_URL=http://127.0.0.1:18108 cargo test ...
#   bash own-server.sh down
#
# All credentials are the throwaway test values of env.sh.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../../../../scripts/test-servers/env.sh
. "$here/../../../../scripts/test-servers/env.sh"
name=fs-msgtest-matrix-crate-tuwunel
port=${FS_MSGTEST_OWN_MATRIX_PORT:-18108}

case "${1:-}" in
  up)
    podman image exists "$FS_MSGTEST_TUWUNEL_IMAGE" || podman pull -q "$FS_MSGTEST_TUWUNEL_IMAGE" >/dev/null
    if [ "$(podman inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" != true ]; then
      podman rm -f -t 5 "$name" >/dev/null 2>&1 || true
      podman run -d --name "$name" --init --no-healthcheck --label fs-msgtest=1 --pull=never \
        --security-opt no-new-privileges --memory 2g --pids-limit 4096 --stop-timeout 60 \
        -p "$FS_MSGTEST_HOST:$port:8008" \
        --tmpfs /var/lib/tuwunel:rw,size=512m \
        -e "TUWUNEL_SERVER_NAME=$FS_MSGTEST_MATRIX_SERVER_NAME" \
        -e TUWUNEL_DATABASE_PATH=/var/lib/tuwunel \
        -e 'TUWUNEL_ADDRESS=["0.0.0.0"]' \
        -e TUWUNEL_PORT=8008 \
        -e TUWUNEL_ROCKSDB_ALLOW_FALLOCATE=false \
        -e TUWUNEL_ALLOW_REGISTRATION=true \
        -e "TUWUNEL_REGISTRATION_TOKEN=$FS_MSGTEST_MATRIX_REGISTRATION_TOKEN" \
        -e TUWUNEL_GRANT_ADMIN_TO_FIRST_USER=false \
        -e TUWUNEL_ALLOW_FEDERATION=false \
        -e 'TUWUNEL_TRUSTED_SERVERS=[]' \
        -e TUWUNEL_RATE_LIMITING__LOGIN__ACCOUNT__BURST_COUNT=0 \
        -e TUWUNEL_RATE_LIMITING__LOGIN__FAILED__BURST_COUNT=0 \
        "$FS_MSGTEST_TUWUNEL_IMAGE" >/dev/null
    fi
    for _ in $(seq 60); do
      if curl -fsS -m 2 -o /dev/null "http://$FS_MSGTEST_HOST:$port/_matrix/client/versions" 2>/dev/null; then
        echo "up: http://$FS_MSGTEST_HOST:$port"
        exit 0
      fi
      sleep 1
    done
    echo "tuwunel did not become ready; see: podman logs $name" >&2
    exit 1
    ;;
  down)
    podman rm -f -t 15 "$name" >/dev/null 2>&1 || true
    echo "removed $name"
    ;;
  *)
    echo "usage: own-server.sh up|down" >&2
    exit 2
    ;;
esac
