#!/usr/bin/env sh
set -eu

: "${DATABASE_URL:?DATABASE_URL is required}"
gateway_bin=${GATEWAY_BIN:-my-ai-gateway}
exec "$gateway_bin" ops retention-cleanup "$@"
