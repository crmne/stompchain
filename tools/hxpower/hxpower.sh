#!/bin/bash
# Power-cycle an HX device through a Home Assistant smart plug.
#
# The device's editor session can wedge badly enough that only removing power
# clears it (see PROTOCOL.md), which during development means waiting on a
# human. This does it unattended.
#
#   hxpower.sh login     # once: exchange your password for a refresh token
#   hxpower.sh off | on | cycle | status
#
# Configuration comes from the environment, or from
# ~/.config/stompchain/hxpower.env if that file exists:
#
#   HA_URL=https://your-home-assistant       required
#   HA_ENTITY=switch.hx_stomp                the smart plug entity
#   HA_USER=you                              your Home Assistant user
#   OP_ITEM=op://vault/item/password         1Password item read at login
#
# `login` reads the password from 1Password once and stores only the refresh
# token, in ~/.config/stompchain/ha.json with owner-only permissions. The
# password itself is never written anywhere.
set -euo pipefail

CONF="$HOME/.config/stompchain/hxpower.env"
# shellcheck disable=SC1090
[ -f "$CONF" ] && . "$CONF"

HA="${HA_URL:?set HA_URL (or put it in $CONF)}"
ENTITY="${HA_ENTITY:-switch.hx_stomp}"
CLIENT_ID="$HA/"
STORE="$HOME/.config/stompchain/ha.json"
OP_ITEM="${OP_ITEM:-}"
USER_NAME="${HA_USER:?set HA_USER (or put it in $CONF)}"

die() { echo "$*" >&2; exit 1; }
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print(d$1)" 2>/dev/null; }

login() {
    local password flow_id code
    if [ -n "$OP_ITEM" ]; then
        command -v op >/dev/null || die "1Password CLI (op) not found"
        password="$(op read "$OP_ITEM")" || die "could not read $OP_ITEM"
    else
        read -rs -p "Home Assistant password for $USER_NAME: " password; echo
    fi

    flow_id=$(curl -sS -X POST "$HA/auth/login_flow" \
        -H 'Content-Type: application/json' \
        -d "{\"client_id\":\"$CLIENT_ID\",\"handler\":[\"homeassistant\",null],\"redirect_uri\":\"$CLIENT_ID\"}" \
        | json "['flow_id']") || die "login_flow failed"
    [ -n "$flow_id" ] || die "no flow id; is $HA reachable?"

    # The password goes in on stdin, never on a command line or in the
    # environment, so it cannot show up in `ps` or a shell history.
    code=$(printf '%s' "$password" | python3 -c '
import json, sys, urllib.request
ha, flow, client, user = sys.argv[1:5]
password = sys.stdin.read()
body = json.dumps({"username": user, "password": password, "client_id": client}).encode()
req = urllib.request.Request(f"{ha}/auth/login_flow/{flow}", body,
                             {"Content-Type": "application/json"})
try:
    result = json.load(urllib.request.urlopen(req, timeout=15))
except Exception as e:
    sys.exit(f"login failed: {e}")
if "result" not in result:
    sys.exit("login rejected: %s" % result.get("errors", result))
print(result["result"])
' "$HA" "$flow_id" "$CLIENT_ID" "$USER_NAME") || die "authentication failed"
    [ -n "$code" ] || die "no authorisation code returned"

    mkdir -p "$(dirname "$STORE")"
    curl -sS -X POST "$HA/auth/token" \
        -d "grant_type=authorization_code&code=$code&client_id=$CLIENT_ID" >"$STORE.tmp"
    python3 -c "
import json,sys
d=json.load(open('$STORE.tmp'))
if 'refresh_token' not in d: sys.exit('no refresh token: %s' % d)
json.dump({'refresh_token': d['refresh_token']}, open('$STORE','w'))
"
    rm -f "$STORE.tmp"
    chmod 600 "$STORE"
    echo "stored a refresh token in $STORE (the password was not saved)"
}

# Short-lived access tokens are minted per run; only the refresh token persists.
access_token() {
    [ -f "$STORE" ] || die "not logged in — run: $0 login"
    local refresh
    refresh=$(json "['refresh_token']" <"$STORE")
    curl -sS -X POST "$HA/auth/token" \
        -d "grant_type=refresh_token&refresh_token=$refresh&client_id=$CLIENT_ID" \
        | json "['access_token']"
}

call() {
    local token="$1" path="$2" body="${3:-}"
    if [ -n "$body" ]; then
        curl -sS -X POST "$HA/api/$path" -H "Authorization: Bearer $token" \
            -H 'Content-Type: application/json' -d "$body"
    else
        curl -sS "$HA/api/$path" -H "Authorization: Bearer $token"
    fi
}

state() { call "$1" "states/$ENTITY" | json "['state']"; }

switch() {
    local token="$1" what="$2"
    call "$token" "services/switch/turn_$what" "{\"entity_id\":\"$ENTITY\"}" >/dev/null
}

main() {
    case "${1:-cycle}" in
    login) login ;;
    status)
        echo "$ENTITY is $(state "$(access_token)")"
        ;;
    on | off)
        local token
        token=$(access_token)
        switch "$token" "$1"
        sleep 2
        echo "$ENTITY is $(state "$token")"
        ;;
    cycle)
        local token
        token=$(access_token)
        echo "powering down $ENTITY"
        switch "$token" off
        # The unit must fully discharge or it keeps its editor session, which
        # is the entire reason for cycling it.
        sleep "${OFF_SECONDS:-8}"
        switch "$token" on
        echo "powered back up; waiting for USB to re-enumerate"
        for _ in $(seq 1 60); do
            sleep 2
            # system_profiler reports nothing on some machines; ask libusb.
            if "$(dirname "$0")/../usbprobe/usbprobe" 2>/dev/null | grep -q "0e41:"; then
                echo "device is back"
                return 0
            fi
        done
        die "device did not re-enumerate within 120s"
        ;;
    *) die "usage: $0 [login|on|off|cycle|status]" ;;
    esac
}

main "$@"
