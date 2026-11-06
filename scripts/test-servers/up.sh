#!/usr/bin/env bash
# Start the messenger test servers. Safe to run repeatedly.
# All credentials used here are throwaway test values from env.sh.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=env.sh
. "$here/env.sh"

die() { echo "up.sh: $*" >&2; exit 1; }

running() { [ "$(podman inspect -f '{{.State.Running}}' "$1" 2>/dev/null)" = true ]; }
exists() { podman container exists "$1"; }

require_free_port() {
  if [ -n "$(ss -Hltn "sport = :$1")" ]; then
    die "port $1 is already in use by something else; change it in env.sh"
  fi
}

wait_for_url() {
  local url=$1 tries=${2:-60}
  for _ in $(seq "$tries"); do
    if curl -fsS -m 2 -o /dev/null "$url" 2>/dev/null; then return 0; fi
    sleep 1
  done
  return 1
}

ensure_volume() { podman volume exists "$1" || podman volume create --label fs-msgtest=1 "$1" >/dev/null; }

# The image's HEALTHCHECK is off so that nothing but the entrypoint runs in it.
common_run_args=(--init --no-healthcheck --label fs-msgtest=1 --pull=never
  --security-opt no-new-privileges --memory 2g --pids-limit 4096)

stalwart_api() {
  curl -fsS -m 30 -u "$FS_MSGTEST_MAIL_ADMIN_USER:$FS_MSGTEST_MAIL_ADMIN_PASSWORD" \
    -H 'Content-Type: application/json' "$FS_MSGTEST_JMAP_URL/jmap/" \
    -d "$1"
}
stalwart_call() {
  stalwart_api "{\"using\":[\"urn:ietf:params:jmap:core\",\"urn:stalwart:jmap\"],\"methodCalls\":$1}"
}

start_stalwart() {
  if ! running "$FS_MSGTEST_STALWART_CONTAINER"; then
    if exists "$FS_MSGTEST_STALWART_CONTAINER"; then
      podman start "$FS_MSGTEST_STALWART_CONTAINER" >/dev/null
    else
      for p in "$FS_MSGTEST_SMTP_PORT" "$FS_MSGTEST_IMAP_PORT" "$FS_MSGTEST_JMAP_PORT" "$FS_MSGTEST_HTTPS_PORT"; do
        require_free_port "$p"
      done
      ensure_volume "$FS_MSGTEST_STALWART_VOLUME_ETC"
      ensure_volume "$FS_MSGTEST_STALWART_VOLUME_DATA"
      # STALWART_RECOVERY_ADMIN pins the admin credential, which up.sh needs for
      # bootstrap and for creating users. STALWART_PUBLIC_URL makes the JMAP
      # session document advertise an address that actually reaches the server.
      # The image binds ports below 1024 through file capabilities, which
      # no-new-privileges disables, so the sysctl allows them inside this
      # container's own network namespace instead.
      podman run -d --name "$FS_MSGTEST_STALWART_CONTAINER" "${common_run_args[@]}" \
        --hostname "$FS_MSGTEST_MAIL_HOSTNAME" \
        --sysctl net.ipv4.ip_unprivileged_port_start=0 \
        -p "$FS_MSGTEST_HOST:$FS_MSGTEST_JMAP_PORT:8080" \
        -p "$FS_MSGTEST_HOST:$FS_MSGTEST_HTTPS_PORT:443" \
        -p "$FS_MSGTEST_HOST:$FS_MSGTEST_SMTP_PORT:465" \
        -p "$FS_MSGTEST_HOST:$FS_MSGTEST_IMAP_PORT:993" \
        -v "$FS_MSGTEST_STALWART_VOLUME_ETC:/etc/stalwart" \
        -v "$FS_MSGTEST_STALWART_VOLUME_DATA:/var/lib/stalwart" \
        -e "STALWART_RECOVERY_ADMIN=$FS_MSGTEST_MAIL_ADMIN_USER:$FS_MSGTEST_MAIL_ADMIN_PASSWORD" \
        -e "STALWART_PUBLIC_URL=$FS_MSGTEST_JMAP_URL" \
        "$FS_MSGTEST_STALWART_IMAGE" >/dev/null
    fi
  fi
  wait_for_url "$FS_MSGTEST_JMAP_URL/healthz/live" || die "Stalwart did not become live; see: podman logs $FS_MSGTEST_STALWART_CONTAINER"
}

bootstrap_stalwart() {
  # Bootstrap only answers while the server is unconfigured. The admin secret it
  # returns is deliberately discarded: the pinned admin in env.sh stays valid.
  local state
  state=$(stalwart_call '[["x:Bootstrap/get",{"ids":["singleton"]},"c1"]]')
  if [ "$(jq -r '.methodResponses[0][0]' <<<"$state")" != 'x:Bootstrap/get' ] ||
     [ "$(jq -r '.methodResponses[0][1].list | length' <<<"$state")" = 0 ]; then
    return 0
  fi
  echo "stalwart: first start, submitting Bootstrap object"
  local body result
  body=$(jq -cn --arg host "$FS_MSGTEST_MAIL_HOSTNAME" --arg domain "$FS_MSGTEST_MAIL_DOMAIN" '
    [["x:Bootstrap/set",{update:{singleton:{
      defaultHostname:$host, defaultDomain:$domain,
      requestTlsCertificate:false, generateDkimKeys:false,
      tracer:{"@type":"Stdout",level:"info",ansi:false,multiline:false,lossy:false,enable:true,events:{},eventsPolicy:"exclude"}
    }}},"c1"]]')
  result=$(stalwart_call "$body")
  jq -e '.methodResponses[0][1].updated.singleton' <<<"$result" >/dev/null || die "Bootstrap failed: $(jq -c . <<<"$result")"
  podman restart -t 10 "$FS_MSGTEST_STALWART_CONTAINER" >/dev/null
  wait_for_url "$FS_MSGTEST_JMAP_URL/healthz/live" || die "Stalwart did not come back after bootstrap"
}

ensure_stalwart_users() {
  local domains domain_id names create='{}' body result
  domains=$(stalwart_call '[["x:Domain/query",{"filter":{}},"c1"],["x:Domain/get",{"#ids":{"resultOf":"c1","name":"x:Domain/query","path":"/ids"},"properties":["id","name"]},"c2"]]')
  domain_id=$(jq -r --arg d "$FS_MSGTEST_MAIL_DOMAIN" '.methodResponses[1][1].list[] | select(.name==$d) | .id' <<<"$domains")
  [ -n "$domain_id" ] || die "domain $FS_MSGTEST_MAIL_DOMAIN not found: $domains"
  names=$(stalwart_call '[["x:Account/query",{"filter":{}},"c1"],["x:Account/get",{"#ids":{"resultOf":"c1","name":"x:Account/query","path":"/ids"},"properties":["name"]},"c2"]]' | jq -c '[.methodResponses[1][1].list[].name]')
  add_user() {
    if ! jq -e --arg n "$1" 'index($n)' <<<"$names" >/dev/null; then
      create=$(jq -c --arg n "$1" --arg p "$2" --arg d "$domain_id" \
        '. + {($n):{"@type":"User",name:$n,domainId:$d,roles:{"@type":"User"},credentials:{"0":{"@type":"Password",secret:$p}}}}' <<<"$create")
    fi
  }
  add_user "${FS_MSGTEST_MAIL_USER1%@*}" "$FS_MSGTEST_MAIL_USER1_PASSWORD"
  add_user "${FS_MSGTEST_MAIL_USER2%@*}" "$FS_MSGTEST_MAIL_USER2_PASSWORD"
  if [ "$create" = '{}' ]; then echo "stalwart: test users already exist"; return 0; fi
  body=$(jq -cn --argjson c "$create" '[["x:Account/set",{create:$c},"c1"]]')
  result=$(stalwart_call "$body")
  jq -e --argjson c "$create" '(.methodResponses[0][1].created // {} | length) == ($c | length)' <<<"$result" >/dev/null ||
    die "creating users failed: $(jq -c . <<<"$result")"
  echo "stalwart: created test users: $(jq -r 'keys | join(", ")' <<<"$create")"
}

# The HTTP health endpoint stays up even when a mail listener failed to bind, so
# wait for a real TLS handshake on the mail ports.
wait_for_tls() {
  for _ in $(seq 30); do
    if timeout 5 openssl s_client -connect "$FS_MSGTEST_HOST:$1" -brief </dev/null >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  die "no TLS on port $1; see: podman logs $FS_MSGTEST_STALWART_CONTAINER"
}

start_tuwunel() {
  if ! running "$FS_MSGTEST_TUWUNEL_CONTAINER"; then
    if exists "$FS_MSGTEST_TUWUNEL_CONTAINER"; then
      podman start "$FS_MSGTEST_TUWUNEL_CONTAINER" >/dev/null
    else
      require_free_port "$FS_MSGTEST_MATRIX_PORT"
      ensure_volume "$FS_MSGTEST_TUWUNEL_VOLUME_DATA"
      # Federation is off: this homeserver talks to nobody else. The login rate
      # limits are off so that repeated test runs do not hit M_LIMIT_EXCEEDED.
      podman run -d --name "$FS_MSGTEST_TUWUNEL_CONTAINER" "${common_run_args[@]}" \
        --stop-timeout 60 \
        -p "$FS_MSGTEST_HOST:$FS_MSGTEST_MATRIX_PORT:8008" \
        -v "$FS_MSGTEST_TUWUNEL_VOLUME_DATA:/var/lib/tuwunel" \
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
  fi
  wait_for_url "$FS_MSGTEST_MATRIX_URL/_matrix/client/versions" || die "tuwunel did not become ready; see: podman logs $FS_MSGTEST_TUWUNEL_CONTAINER"
}

podman image exists "$FS_MSGTEST_STALWART_IMAGE" || podman pull -q "$FS_MSGTEST_STALWART_IMAGE" >/dev/null
podman image exists "$FS_MSGTEST_TUWUNEL_IMAGE" || podman pull -q "$FS_MSGTEST_TUWUNEL_IMAGE" >/dev/null

start_stalwart
bootstrap_stalwart
ensure_stalwart_users
wait_for_tls "$FS_MSGTEST_SMTP_PORT"
wait_for_tls "$FS_MSGTEST_IMAP_PORT"
start_tuwunel

echo "up: Stalwart JMAP $FS_MSGTEST_JMAP_URL  SMTP $FS_MSGTEST_HOST:$FS_MSGTEST_SMTP_PORT  IMAP $FS_MSGTEST_HOST:$FS_MSGTEST_IMAP_PORT"
echo "up: tuwunel $FS_MSGTEST_MATRIX_URL"
echo "up: run check.sh next; it also registers the two Matrix users."
