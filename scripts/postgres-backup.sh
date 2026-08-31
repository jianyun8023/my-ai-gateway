#!/usr/bin/env sh
set -eu

: "${DATABASE_URL:?DATABASE_URL is required}"
output=${1:-"backups/gateway-$(date -u +%Y%m%dT%H%M%SZ).dump"}
umask 077
mkdir -p "$(dirname "$output")"
pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" >"$output"
sha256sum "$output"
