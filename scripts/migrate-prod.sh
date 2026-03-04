#!/usr/bin/env bash
# migrate-prod.sh — Apply all pending migrations to the production database.
#
# Usage:
#   ./scripts/migrate-prod.sh
#   DATABASE_URL=postgres://user:pass@host:5432/db ./scripts/migrate-prod.sh
#
# The DATABASE_URL environment variable takes precedence over settings.yaml.
# If not set, it falls back to the value in settings.yaml.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Resolve DATABASE_URL: env var wins, otherwise parse from settings.yaml
if [ -z "${DATABASE_URL:-}" ]; then
  SETTINGS="$PROJECT_DIR/settings.yaml"
  if [ ! -f "$SETTINGS" ]; then
    echo "ERROR: DATABASE_URL not set and settings.yaml not found at $SETTINGS" >&2
    exit 1
  fi
  DATABASE_URL=$(grep '^database_url:' "$SETTINGS" | sed 's/^database_url:[[:space:]]*//')
  if [ -z "$DATABASE_URL" ]; then
    echo "ERROR: Could not parse database_url from $SETTINGS" >&2
    exit 1
  fi
fi

# Safety check: refuse to run against any URL containing "test"
if echo "$DATABASE_URL" | grep -qi "test"; then
  echo "ERROR: DATABASE_URL appears to point at a test database. Aborting." >&2
  echo "  URL: $DATABASE_URL" >&2
  exit 1
fi

echo "Applying migrations to: $DATABASE_URL"
sqlx migrate run --source "$PROJECT_DIR/migrations" --database-url "$DATABASE_URL"
echo "Migrations applied successfully."
