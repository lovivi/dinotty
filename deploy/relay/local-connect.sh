#!/usr/bin/env bash
# local-connect.sh — register this Dinotty with a cloud relay.
#
# Usage:
#   bash local-connect.sh "<relay-line-from-server-install>" "<password>"
#
# Or via the one-liner the server-install prints (it pipes the
# variables in). Either way the script:
#
#   1. Generates a persistent `desktop_id` (UUID stored in
#      ~/.dinotty/relay-desktop-id) so the same ID is reused on
#      subsequent runs.
#   2. Reads the Dinotty server token from
#      ~/.config/dinotty/token (or $DINOTTY_TOKEN, or prompts).
#   3. POSTs to /relay/desktop/register on the relay.
#   4. Installs a systemd user unit that runs the desktop binary
#      in `--relay-outbound` mode to keep a persistent WS to the
#      relay alive (auto-restart on disconnect).
#   5. Prints a QR PNG (requires `qrencode`) and a copyable URL
#      so the user can scan it on the phone.

set -euo pipefail

RELAY_URL="${1:?usage: $0 '<relay-line-from-server-install>' '<password>'}"
PASSWORD="${2:?usage: $0 '<relay-line-from-server-install>' '<password>'}"

# ---- 1. Desktop ID ----
DINOTTY_HOME="${HOME}"
DESKTOP_ID_DIR="$DINOTTY_HOME/.dinotty"
DESKTOP_ID_FILE="$DESKTOP_ID_DIR/relay-desktop-id"
mkdir -p "$DESKTOP_ID_DIR"

if [ -f "$DESKTOP_ID_FILE" ]; then
    DESKTOP_ID="$(cat "$DESKTOP_ID_FILE")"
else
    if command -v uuidgen >/dev/null 2>&1; then
        DESKTOP_ID="$(uuidgen)"
    elif [ -r /proc/sys/kernel/random/uuid ]; then
        DESKTOP_ID="$(cat /proc/sys/kernel/random/uuid)"
    else
        DESKTOP_ID="$(od -x /dev/urandom | head -1 | awk '{print $2$3"-"$4"-"$5"-"$6}')"
    fi
    echo "$DESKTOP_ID" > "$DESKTOP_ID_FILE"
    echo ">> Generated new desktop_id: $DESKTOP_ID (saved to $DESKTOP_ID_FILE)"
fi

# ---- 2. Dinotty server token (so the relay can authenticate as the
#       desktop to itself; this is the long random string from
#       `dinotty-server`'s startup log, or the contents of
#       ~/.config/dinotty/token). ----
DINOTTY_TOKEN_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/dinotty/token"
if [ -z "${DINOTTY_TOKEN:-}" ] && [ -f "$DINOTTY_TOKEN_FILE" ]; then
    DINOTTY_TOKEN="$(cat "$DINOTTY_TOKEN_FILE")"
fi
if [ -z "$DINOTTY_TOKEN" ]; then
    read -r -p "Dinotty server token (paste from server log; blank = trust relay IP whitelist): " DINOTTY_TOKEN
    # Trim whitespace
    DINOTTY_TOKEN="$(printf '%s' "$DINOTTY_TOKEN" | tr -d '[:space:]')"
fi

# ---- 3. Register ----
echo ">> Registering desktop with relay at $RELAY_URL ..."
RELAY_BASE="${RELAY_URL#*//}"   # strip scheme
RELAY_BASE="${RELAY_BASE%/}"     # strip trailing slash
RELAY_SCHEME="${RELAY_URL%%://*}"
RELAY_HOST="${RELAY_BASE%%/*}"
RELAY_HTTP_SCHEME="$([ "$RELAY_SCHEME" = "wss" ] && echo https || echo http)"

REGISTER_URL="${RELAY_HTTP_SCHEME}://${RELAY_HOST}/relay/desktop/register"
RESP="$(curl -fsS -X POST \
    -H "Authorization: Bearer ${PASSWORD}" \
    -H "Content-Type: application/json" \
    -d "{\"desktop_id\":\"${DESKTOP_ID}\",\"upstream_token\":\"${DINOTTY_TOKEN}\"}" \
    "$REGISTER_URL" 2>&1)" || {
        echo "!! Registration failed: $RESP" >&2
        exit 1
    }
echo "   server replied: $RESP"

# ---- 4. systemd user unit ----
SERVICE_NAME="dinotty-relay-client"
SERVICE_DIR="$HOME/.config/systemd/user"
SERVICE_FILE="$SERVICE_DIR/${SERVICE_NAME}.service"
mkdir -p "$SERVICE_DIR"

cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=Dinotty relay client (outbound to ${RELAY_HOST})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${DINOTTY_HOME}/.cargo/bin/dinotty-server --relay-outbound ${RELAY_URL} ${PASSWORD} --relay-desktop-id ${DESKTOP_ID} \${DINOTTY_TOKEN}
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
EOF

# Check if dinotty-server is on PATH; warn if not
if ! command -v dinotty-server >/dev/null 2>&1 && [ ! -x "${DINOTTY_HOME}/.cargo/bin/dinotty-server" ]; then
    echo "!! dinotty-server not on PATH. Edit the ExecStart line above to point"
    echo "   to your binary before starting the service."
fi

systemctl --user daemon-reload 2>/dev/null || true
systemctl --user enable --now "$SERVICE_NAME" 2>/dev/null || {
    echo "!! systemctl --user failed. The unit file is at $SERVICE_FILE."
    echo "   You can run it manually:"
    echo "     ${DINOTTY_HOME}/.cargo/bin/dinotty-server --relay-outbound ${RELAY_URL} ${PASSWORD} --relay-desktop-id ${DESKTOP_ID} <token>"
}

# ---- 5. QR for the phone ----
DEEPLINK="${RELAY_HTTP_SCHEME}://${RELAY_HOST}/?desktop_id=${DESKTOP_ID}"
QR_TEXT="${DEEPLINK}#${PASSWORD}"

echo
echo "============================================================"
echo "  Scan this on the phone (or open the URL below):"
echo

QR_PATH="/tmp/dinotty-relay-qr.png"
if command -v qrencode >/dev/null 2>&1; then
    qrencode -o "$QR_PATH" "$QR_TEXT" && echo "  QR saved to: $QR_PATH"
    echo "  (transfer that file to the phone and open it)"
else
    echo "  (install 'qrencode' to get a PNG; the URL is below)"
fi
echo
echo "  URL:  $DEEPLINK"
echo "  Password (in URL fragment, hidden in QR):  $PASSWORD"
echo "============================================================"
