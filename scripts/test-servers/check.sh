#!/usr/bin/env bash
# Prove the test servers work end to end. Exits non-zero on the first failure.
# Every run uses a fresh nonce, so an old message or event can never pass.
# Passwords and tokens are throwaway test values from env.sh.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=env.sh
. "$here/env.sh"

pass() { echo "PASS  $*"; }
fail() { echo "FAIL  $*" >&2; exit 1; }
trap 'rc=$?; [ $rc -eq 0 ] && echo "check.sh: ALL PASSED" || echo "check.sh: FAILED (exit $rc)" >&2' EXIT

nonce=$(date +%s)-$RANDOM$RANDOM

# ------------------------------------------------------------- containers ----
check_container() {
  local name=$1 digest=$2 got bindings
  [ "$(podman inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" = true ] || fail "$name is not running (run up.sh)"
  got=$(podman inspect -f '{{.ImageDigest}}' "$name")
  [ "$got" = "$digest" ] || fail "$name runs $got, expected $digest"
  bindings=$(podman inspect "$name" | jq -r '[.[0].NetworkSettings.Ports[] | select(.) | .[].HostIp] | unique | join(",")')
  [ "$bindings" = "$FS_MSGTEST_HOST" ] || fail "$name publishes on '$bindings', expected only $FS_MSGTEST_HOST"
  pass "$name running, digest $digest, published on $bindings only"
}
check_container "$FS_MSGTEST_STALWART_CONTAINER" "$FS_MSGTEST_STALWART_DIGEST"
check_container "$FS_MSGTEST_TUWUNEL_CONTAINER" "$FS_MSGTEST_TUWUNEL_DIGEST"

stalwart_version=$(podman logs "$FS_MSGTEST_STALWART_CONTAINER" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -o 'version = "[^"]*"' | tail -1 | cut -d'"' -f2)
[ "$stalwart_version" = "$FS_MSGTEST_STALWART_VERSION" ] || fail "Stalwart reports version '$stalwart_version', expected $FS_MSGTEST_STALWART_VERSION"
pass "Stalwart reports version $stalwart_version"

tuwunel_version=$(curl -fsS -m 10 "$FS_MSGTEST_MATRIX_URL/_tuwunel/server_version" | jq -r .version)
case "$tuwunel_version" in
  "$FS_MSGTEST_TUWUNEL_VERSION"|"$FS_MSGTEST_TUWUNEL_VERSION"[!0-9]*) ;;
  *) fail "tuwunel reports version '$tuwunel_version', expected $FS_MSGTEST_TUWUNEL_VERSION" ;;
esac
pass "tuwunel reports version $tuwunel_version"

# ------------------------------------------------------------------- mail ----
# Stalwart uses a self-signed certificate here, hence -k on the TLS connections.
subject="fs-msgtest-$nonce"
body="body-$nonce"
message=$(printf 'From: %s\r\nTo: %s\r\nSubject: %s\r\nDate: %s\r\nMessage-ID: <%s@%s>\r\n\r\n%s\r\n' \
  "$FS_MSGTEST_MAIL_USER1" "$FS_MSGTEST_MAIL_USER2" "$subject" "$(date -R)" "$nonce" "$FS_MSGTEST_MAIL_DOMAIN" "$body")

curl -sS -k -m 30 --ssl-reqd "smtps://$FS_MSGTEST_HOST:$FS_MSGTEST_SMTP_PORT" \
  --user "$FS_MSGTEST_MAIL_USER1:$FS_MSGTEST_MAIL_USER1_PASSWORD" \
  --mail-from "$FS_MSGTEST_MAIL_USER1" --mail-rcpt "$FS_MSGTEST_MAIL_USER2" \
  -T - <<<"$message" || fail "SMTP submission failed"
pass "SMTP submission: $FS_MSGTEST_MAIL_USER1 -> $FS_MSGTEST_MAIL_USER2, subject $subject"

imap() {
  curl -sS -k -m 30 --user "$FS_MSGTEST_MAIL_USER2:$FS_MSGTEST_MAIL_USER2_PASSWORD" "$@"
}
imap_url="imaps://$FS_MSGTEST_HOST:$FS_MSGTEST_IMAP_PORT/INBOX"
uid=''
for _ in $(seq 30); do
  uid=$(imap "$imap_url" -X "UID SEARCH SUBJECT \"$subject\"" | tr -d '\r' | sed -n 's/^\* SEARCH \([0-9][0-9]*\).*/\1/p')
  [ -n "$uid" ] && break
  sleep 1
done
[ -n "$uid" ] || fail "IMAP: message did not arrive in $FS_MSGTEST_MAIL_USER2's INBOX within 30s"
imap_message=$(imap "$imap_url/;UID=$uid")
grep -q "$body" <<<"$imap_message" || fail "IMAP: body of UID $uid does not contain $body"
grep -qi "^From: $FS_MSGTEST_MAIL_USER1" <<<"$imap_message" || fail "IMAP: From header of UID $uid is wrong"
pass "IMAP: read from $FS_MSGTEST_MAIL_USER2 INBOX, UID $uid, body and sender match"

jmap_auth=(-u "$FS_MSGTEST_MAIL_USER2:$FS_MSGTEST_MAIL_USER2_PASSWORD")
session=$(curl -fsS -m 10 "${jmap_auth[@]}" "$FS_MSGTEST_JMAP_URL/jmap/session") || fail "JMAP: session request failed"
api_url=$(jq -r .apiUrl <<<"$session")
account=$(jq -r '.primaryAccounts["urn:ietf:params:jmap:mail"]' <<<"$session")
[ "$api_url" = "$FS_MSGTEST_JMAP_URL/jmap/" ] || fail "JMAP: session advertises unreachable apiUrl $api_url"
request=$(jq -cn --arg a "$account" --arg s "$subject" '{
  using:["urn:ietf:params:jmap:core","urn:ietf:params:jmap:mail"],
  methodCalls:[
    ["Mailbox/query",{accountId:$a,filter:{role:"inbox"}},"m"],
    ["Email/query",{accountId:$a,filter:{subject:$s}},"q"],
    ["Email/get",{accountId:$a,"#ids":{resultOf:"q",name:"Email/query",path:"/ids"},
      properties:["subject","from","mailboxIds","textBody","bodyValues"],fetchTextBodyValues:true},"g"]]}')
email='null'
for _ in $(seq 30); do
  response=$(curl -fsS -m 10 "${jmap_auth[@]}" -H 'Content-Type: application/json' "$api_url" -d "$request") || fail "JMAP: API call failed"
  email=$(jq -c '.methodResponses[2][1].list[0] // null' <<<"$response")
  [ "$email" != null ] && break
  sleep 1
done
[ "$email" != null ] || fail "JMAP: message not found within 30s: $response"
inbox=$(jq -r '.methodResponses[0][1].ids[0]' <<<"$response")
jq -e --arg s "$subject" --arg b "$body" --arg f "$FS_MSGTEST_MAIL_USER1" --arg i "$inbox" '
  .subject==$s and .from[0].email==$f and .mailboxIds[$i]==true
  and (.bodyValues[.textBody[0].partId].value | contains($b))' <<<"$email" >/dev/null ||
  fail "JMAP: message content does not match: $email"
pass "JMAP: read via session apiUrl $api_url, account $account, in Inbox, body and sender match"

# ----------------------------------------------------------------- matrix ----
mx() { # METHOD PATH [TOKEN] [JSON]
  local args=(-sS -m 30 -X "$1" -H 'Content-Type: application/json')
  [ -n "${3:-}" ] && args+=(-H "Authorization: Bearer $3")
  [ -n "${4:-}" ] && args+=(-d "$4")
  curl "${args[@]}" "$FS_MSGTEST_MATRIX_URL/_matrix/client/v3$2"
}

# Prints the outcome of the two-step (UIA) registration with the given token.
register() { # USER PASSWORD TOKEN
  local first session second
  first=$(mx POST /register '' "$(jq -cn --arg u "$1" --arg p "$2" '{username:$u,password:$p,inhibit_login:true}')")
  if [ "$(jq -r '.errcode // empty' <<<"$first")" = M_USER_IN_USE ]; then echo exists; return 0; fi
  jq -e '[.flows[]?.stages[]?] | index("m.login.registration_token")' <<<"$first" >/dev/null ||
    { echo "no-token-flow: $first"; return 0; }
  session=$(jq -r .session <<<"$first")
  second=$(mx POST /register '' "$(jq -cn --arg u "$1" --arg p "$2" --arg t "$3" --arg s "$session" \
    '{username:$u,password:$p,inhibit_login:true,auth:{type:"m.login.registration_token",token:$t,session:$s}}')")
  if jq -e .user_id <<<"$second" >/dev/null; then echo registered
  elif [ "$(jq -r '.errcode // empty' <<<"$second")" = M_USER_IN_USE ]; then echo exists
  else echo "rejected: $(jq -c . <<<"$second")"; fi
}

outcome=$(register "fs-msgtest-negative-$RANDOM$RANDOM" "fs-msgtest-unused" "wrong-token-$nonce")
case "$outcome" in
  rejected:*) pass "Matrix: registration with a wrong token is rejected" ;;
  *) fail "Matrix: wrong token was not rejected: $outcome" ;;
esac

ensure_user() { # USER PASSWORD
  local outcome
  outcome=$(register "$1" "$2" "$FS_MSGTEST_MATRIX_REGISTRATION_TOKEN")
  case "$outcome" in
    registered) pass "Matrix: registered @$1:$FS_MSGTEST_MATRIX_SERVER_NAME with the registration token" ;;
    exists) pass "Matrix: @$1:$FS_MSGTEST_MATRIX_SERVER_NAME already registered by an earlier run" ;;
    *) fail "Matrix: registering $1: $outcome" ;;
  esac
}
ensure_user "$FS_MSGTEST_MATRIX_USER1" "$FS_MSGTEST_MATRIX_USER1_PASSWORD"
ensure_user "$FS_MSGTEST_MATRIX_USER2" "$FS_MSGTEST_MATRIX_USER2_PASSWORD"

login() { # USER PASSWORD
  local response
  response=$(mx POST /login '' "$(jq -cn --arg u "$1" --arg p "$2" \
    '{type:"m.login.password",identifier:{type:"m.id.user",user:$u},password:$p}')")
  jq -er .access_token <<<"$response" || fail "Matrix: login of $1 failed: $response"
}
alice_token=$(login "$FS_MSGTEST_MATRIX_USER1" "$FS_MSGTEST_MATRIX_USER1_PASSWORD")
bob_token=$(login "$FS_MSGTEST_MATRIX_USER2" "$FS_MSGTEST_MATRIX_USER2_PASSWORD")
pass "Matrix: both users logged in"

bob_id="@$FS_MSGTEST_MATRIX_USER2:$FS_MSGTEST_MATRIX_SERVER_NAME"
created=$(mx POST /createRoom "$alice_token" "$(jq -cn --arg b "$bob_id" --arg n "$subject" '{
  preset:"private_chat",name:$n,invite:[$b],
  initial_state:[{type:"m.room.encryption",state_key:"",content:{algorithm:"m.megolm.v1.aes-sha2"}}]}')")
room_id=$(jq -er .room_id <<<"$created") || fail "Matrix: createRoom failed: $created"
room_path=$(jq -rn --arg r "$room_id" '$r|@uri')

joined=$(mx POST "/rooms/$room_path/join" "$bob_token" '{}')
jq -e .room_id <<<"$joined" >/dev/null || fail "Matrix: bob could not join: $joined"

encryption=$(mx GET "/rooms/$room_path/state/m.room.encryption/" "$bob_token")
jq -e '.algorithm=="m.megolm.v1.aes-sha2"' <<<"$encryption" >/dev/null || fail "Matrix: m.room.encryption state missing: $encryption"
pass "Matrix: room $room_id has m.room.encryption m.megolm.v1.aes-sha2, read back as bob"

# The server treats encrypted events as opaque, so this is a placeholder of the
# right shape. It proves event transport, not Olm/Megolm cryptography.
ciphertext="placeholder-not-real-ciphertext-$nonce"
sent=$(mx PUT "/rooms/$room_path/send/m.room.encrypted/txn-$nonce" "$alice_token" "$(jq -cn --arg c "$ciphertext" '{
  algorithm:"m.megolm.v1.aes-sha2",sender_key:"fs-msgtest",device_id:"FSMSGTEST",session_id:"fs-msgtest",ciphertext:$c}')")
event_id=$(jq -er .event_id <<<"$sent") || fail "Matrix: send failed: $sent"
pass "Matrix: alice sent m.room.encrypted event $event_id"

found=false
for _ in $(seq 15); do
  sync=$(curl -sS -m 30 -G -H "Authorization: Bearer $bob_token" "$FS_MSGTEST_MATRIX_URL/_matrix/client/v3/sync" \
    --data-urlencode 'timeout=1000' --data-urlencode 'filter={"room":{"timeline":{"limit":50}}}')
  if jq -e --arg r "$room_id" --arg e "$event_id" --arg c "$ciphertext" '
      .rooms.join[$r].timeline.events | any(.event_id==$e and .type=="m.room.encrypted" and .content.ciphertext==$c)' <<<"$sync" >/dev/null; then
    found=true
    break
  fi
  sleep 1
done
[ "$found" = true ] || fail "Matrix: event $event_id not seen in bob's /sync"
jq -e --arg r "$room_id" '.rooms.join[$r] | [.state.events[]?, .timeline.events[]?] | any(.type=="m.room.encryption")' <<<"$sync" >/dev/null ||
  fail "Matrix: /sync does not show m.room.encryption for $room_id"
pass "Matrix: bob fetched event $event_id and m.room.encryption through /sync"

mx POST /logout "$alice_token" '{}' >/dev/null
mx POST /logout "$bob_token" '{}' >/dev/null
