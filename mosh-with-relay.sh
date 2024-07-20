#!/bin/bash

error_echo() {
    >&2 printf "\033[1;31m%s\033[0m\n" "$@"
}

OPS_PING='\xff\x15'
OPS_PONG='\xff\x16'


help() {
    cat << EOF
Connects to remote host using mosh

Usage: $(basename "$0") <RELAY_SERVER> <TARGET_HOST>

Argunments:
      HOST           What you'd normally type to ssh into remote host
                     e.g. user@hostname, ssh-alias
      PORT           Integer that represents ssh port
                     e.g. 9000

Options:
      -u, --upload   test the upload speed only
      -d, --download test the download speed only
      -f, --factor   set the multiplication factor of blocks to transmits to the
                     remote host to test the speed.
                     (default: 1.0, which corresponds to 1.0 MiB)

      --help         display this help message and exit
      --version      output version information and exit

EOF
}

mosh_client_binary="mosh-client"
relay_server_udprelay_binary="udprelay-rust"
relay_server_udprelay_binary="./udprelay-rust"
RELAY_PORT=60017
RELAY_PSK=uNYDA5QRcvYgp2gfS5v5

# shellcheck disable=SC2116,SC2028
EOL=$(echo '\00\07\01\00')
if [ "$#" != 0 ]; then
  set -- "$@" "$EOL"
  while [ "$1" != "$EOL" ]; do
    opt="$1"; shift
    case "$opt" in
      --help)
        help
        exit
        ;;
      -m|--mosh-client-binary)
        mosh_client_binary="$1"
        shift
        ;;
      --relay-port)
        RELAY_PORT="$1"
        shift
        ;;
      --relay-server-psk)
        RELAY_PSK="$1"
        shift
        ;;
      --via)
        RELAY_SERVER_SSH_NAME="$1"
        shift
        ;;
      -v|--verbose)
        VERBOSE="set -x"
        ;;
      --*=*)  # convert '--name=arg' to '--name' 'arg'
        set -- "${opt%%=*}" "${opt#*=}" "$@";;
      -[!-]?*)  # convert '-abc' to '-a' '-b' '-c'
        # shellcheck disable=SC2046  # we want word splitting
        set -- $(echo "${opt#-}" | sed 's/\(.\)/ -\1/g') "$@";;
      --)  # process remaining arguments as positional
        while [ "$1" != "$EOL" ]; do set -- "$@" "$1"; shift; done;;
      -*)
        echo "Error: Unsupported flag '$opt'" >&2
        exit 1
        ;;
      *)
        # set back any unused args
        set -- "$@" "$opt"
    esac
  done
  shift # remove the EOL token
fi

if [ "$#" -ne 1 ]; then
    help
    exit 1
fi
TARGET_SSH_SERVER="$1"

on_exit() {
    status=$?
    rm -f "$TMP_SSH_CONFIG"
    # shellcheck disable=SC2181
    [ "$status" -eq 0 ] && exit
    # non-zero exit status
    error_echo "> Error occured. Reason:"
    [ -z "$MODE" ] && MODE=Destination
    case "$status" in
        40) error_echo "- [$MODE] Unknown nc type"
        ;;
        41) error_echo "- [$MODE] nc / socat is not installed"
        ;;
        42) error_echo "- [$MODE] Failed to send PSK to relay server"
        ;;
        43) error_echo "- [$MODE] MOSH_SERVER_KEY seems to be empty"
        ;;
        44) error_echo "- [$MODE] CLIENT_PORT is in-use"
        ;;
        45) error_echo "- [$MODE] Failed to send message. Connection refused? Relay server not reachable?"
        ;;
        46) error_echo "- [$MODE] Unknown error message in when starting mosh-server"
        ;;
        47) error_echo "- [$MODE] Unable to start any new mosh-server from given port range [$SPORT_RANGE_START-$SPORT_RANGE_END]"
        ;;
        49) error_echo "- [$MODE] relay-server had already been started."
        ;;
        127) error_echo "- [$MODE] Command not found?"
        ;;
        134) error_echo "- [$MODE] likely to be nc failing (core dump?) due to binded port"
        ;;
        *) error_echo "- [$MODE] Unknown. Not an exit code that we had set: $status."
        ;;
    esac
    exit "$status"
}
trap on_exit EXIT

$VERBOSE

SSH_ARGS=()
if command -v assh >/dev/null 2>&1; then
    TMP_SSH_CONFIG="$(mktemp)"
    assh config build | sed 's/# HostName:/HostName/' >"$TMP_SSH_CONFIG"
    SSH_ARGS+=(-F "$TMP_SSH_CONFIG")
fi

retrieve_hostname_from_ssh_config() {
    # convert from ssh alias to homename
    # if assh exists, get hostname from it
    ssh "${SSH_ARGS[@]}" -G "$1" | awk '$1 == "hostname" { print $2 }'
}

RELAY_SERVER_HOSTNAME="$(retrieve_hostname_from_ssh_config $RELAY_SERVER_SSH_NAME)"


if [ -z "$RELAY_SERVER_SSH_NAME" ]; then
    # directly use mosh to connects
    exec mosh --ssh="ssh $(echo "${SSH_ARGS[@]}")" "$TARGET_SSH_SERVER"
fi


# EstablishConnection=(255 5)

# we will store some helper functions here, as we want to
# pass them via ssh tunneel, and yet we don't want to re-defining
# them twice (once in souce and once in variables)
read -r -d '' HELPERS_DEF <<'EOF'
set -e
set -o pipefail

has_cmd() {
    command -v "$1" >/dev/null
}

_detect_nc_type() {
    if 2>&1 nc -h | grep -q 'GNU netcat'; then
        return 0
        elif 2>&1 nc -h | grep -q 'OpenBSD netcat'; then
        return 1
    fi
    exit 40
}

universial_nc() {
    target_port="$2"
    source_port="$3"
    [ -z "$source_port" ] && source_port="$2"
    if has_cmd socat; then
        socat - "UDP4:$1:$target_port,sourceport=$source_port"
        # socat STDIN "UDP-SENDTO:$1:$target_port,sourceport=$source_port"
    elif has_cmd nc; then
        _detect_nc_type; _type="$?"
        if [ "$_type" -eq 0 ]; then
            nc -cu "$1" -p "$source_port" "$target_port"
            # the previous command might returns non zero status
            true
        elif [ "$_type" -eq 1 ]; then
            nc -u -q0 "$1" "$target_port"
        fi
    else
        exit 41
    fi
}

get_free_port() {
    SPORT_RANGE_START=$1
    SPORT_RANGE_END=$2
    TRYING_PORT=$SPORT_RANGE_START
    while true; do
        # try next available port (we want a port that nc fails to connects, i.e., free)
        echo | nc -cu localhost $TRYING_PORT >/dev/null 2>&1 && TRYING_PORT=$(( $TRYING_PORT + 1 )) || break
        [ $TRYING_PORT -gt $SPORT_RANGE_END ] && exit 47
    done
    echo "$TRYING_PORT"
}

send_psk() {
    get_establish_message "$1" "$2" | universial_nc "$3" "$4" "$5" || exit 45
}

format_hex() {
    printf '\\x%x' $@
}

get_establish_message() {
    PSK="$1"
    session_secret="$2"
    printf "$(format_hex 255 5 "${#PSK}" "${#session_secret}")"
    printf '%s' "$PSK" "$session_secret"
}
EOF
# source the defined helper functions
. <(echo "$HELPERS_DEF")

MODE=Local

CPORT_RANGE_START=55500
CPORT_RANGE_END=55550
SPORT_RANGE_START=55500
SPORT_RANGE_END=55550


CLIENT_PORT="$(get_free_port $CPORT_RANGE_START $CPORT_RANGE_END)"
session_secret="$(mktemp -u XXXXXXXXXXXXXXXX)"

RELAY_SERVER_IP="$(getent hosts "$RELAY_SERVER_HOSTNAME" | awk '{ print $1 }' | head -n1)"
# RELAY_SERVER_IP=127.0.0.1

# start relay server
# ssh "$RELAY_SERVER_HOSTNAME" 'bash -s'<<EOF
if [ "$(printf $OPS_PONG)" != "$(printf $OPS_PING | socat -t 0.6 - UDP4:$RELAY_SERVER_IP:$RELAY_PORT 2>/dev/null)" ]; then
    # server is not up
    ssh "$RELAY_SERVER_SSH_NAME" 'bash -s'<<EOF
    "$relay_server_udprelay_binary" "$RELAY_PORT" -d
EOF
fi


MODE=SSH
# connects to destination-server
# the following forward a copy of all the helpers, then search for a free udp-port,
# sends a udp package to the relay-server from the to-be server port (hole punching),
# then finally starting the mosh server.
SERVER_RESPONSE="$(ssh "$TARGET_SSH_SERVER" 'bash -s'<<EOF
$VERBOSE
$HELPERS_DEF
SERVER_PORT="\$(get_free_port $SPORT_RANGE_START $SPORT_RANGE_END)"
send_psk "$RELAY_PSK" "$session_secret" "$RELAY_SERVER_IP" "$RELAY_PORT" "\$SERVER_PORT" >/dev/null
mosh-server new  -p "\$SERVER_PORT"  # 2>/dev/null
EOF
)"
MOSH_SERVER_KEY="$(echo "$SERVER_RESPONSE" | sed -n 's/^.*MOSH CONNECT [0-9]\+ \(.*\)$/\1/ p')"


MODE=Local
[ -n "$MOSH_SERVER_KEY" ] || exit 43

###############################
# connects to relay-server
# handshake to establish binding ports
send_psk "$RELAY_PSK" "$session_secret" "$RELAY_SERVER_IP" "$RELAY_PORT" "$CLIENT_PORT"

# launch the actual mosh client
export MOSH_KEY="$MOSH_SERVER_KEY"
export MOSH_CLIENT_PORT="$CLIENT_PORT"

exec "$mosh_client_binary" "$RELAY_SERVER_IP" "$RELAY_PORT"
