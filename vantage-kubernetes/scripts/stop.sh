#!/usr/bin/env bash
# Tear down the integration-test minikube cluster.
set -euo pipefail

PROFILE="${MINIKUBE_PROFILE:-vantage}"

if ! command -v minikube >/dev/null 2>&1; then
  echo "minikube is not installed; nothing to stop." >&2
  exit 0
fi

echo "Deleting minikube (profile: $PROFILE)..."
minikube delete --profile "$PROFILE"
echo "✅ cluster removed."
