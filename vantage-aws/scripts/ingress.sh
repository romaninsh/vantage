#!/bin/bash
# Walk db/*.sh and run each one. Each db script is self-contained: it
# knows how to drop, create, and populate one table. Mirrors the pattern
# in vantage-surrealdb/scripts/ingress.sh.
set -e

# Optional .env for overriding endpoint / credentials. Mirrors surrealdb.
if [[ -f .env ]]; then
    source .env
fi

cd "$(dirname "$0")"

for db_script in db/*.sh; do
  name=$(basename "$db_script" .sh)
  echo "Processing $name..."
  bash "$db_script"
  echo "  done."
done
