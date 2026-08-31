#!/usr/bin/env sh
set -eu

: "${RESTORE_DATABASE_URL:?RESTORE_DATABASE_URL is required}"
dump=${1:?usage: postgres-restore.sh <dump> --confirm}
if [ "${2:-}" != "--confirm" ]; then
  echo "refusing destructive restore; pass --confirm explicitly" >&2
  exit 2
fi

pg_restore --no-owner --exit-on-error --dbname "$RESTORE_DATABASE_URL" "$dump"
