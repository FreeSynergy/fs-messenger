#!/usr/bin/env bash
# Remove the test containers and volumes, and nothing else. The names are listed
# explicitly and each must start with fs-msgtest-. All test data is lost.
# Pulled images stay in the local store so the next up.sh needs no download.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=env.sh
. "$here/env.sh"

for name in "$FS_MSGTEST_STALWART_CONTAINER" "$FS_MSGTEST_TUWUNEL_CONTAINER" \
            "$FS_MSGTEST_STALWART_VOLUME_ETC" "$FS_MSGTEST_STALWART_VOLUME_DATA" \
            "$FS_MSGTEST_TUWUNEL_VOLUME_DATA"; do
  case "$name" in
    fs-msgtest-*) ;;
    *) echo "down.sh: refusing to touch '$name': missing the fs-msgtest- prefix" >&2; exit 1 ;;
  esac
done

for c in "$FS_MSGTEST_STALWART_CONTAINER" "$FS_MSGTEST_TUWUNEL_CONTAINER"; do
  if podman container exists "$c"; then
    podman rm -f -t 15 "$c" >/dev/null
    echo "removed container $c"
  fi
done

for v in "$FS_MSGTEST_STALWART_VOLUME_ETC" "$FS_MSGTEST_STALWART_VOLUME_DATA" "$FS_MSGTEST_TUWUNEL_VOLUME_DATA"; do
  if podman volume exists "$v"; then
    podman volume rm "$v" >/dev/null
    echo "removed volume $v"
  fi
done
