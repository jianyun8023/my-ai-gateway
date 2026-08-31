#!/usr/bin/env sh
set -eu

: "${DATABASE_URL:?DATABASE_URL is required}"
: "${GATEWAY_ADMIN_KEY:?GATEWAY_ADMIN_KEY is required}"

output=${1:-control-plane.json}
umask 077
gateway_bin=${GATEWAY_BIN:-my-ai-gateway}

if command -v "$gateway_bin" >/dev/null 2>&1; then
  "$gateway_bin" ops control-plane-export --output "$output"
else
  admin_url=${ADMIN_URL:-http://127.0.0.1:8787}
  curl --fail --silent --show-error \
    -H "Authorization: Bearer $GATEWAY_ADMIN_KEY" \
    "$admin_url/admin/control-plane/export" >"$output"
fi
chmod 600 "$output"
