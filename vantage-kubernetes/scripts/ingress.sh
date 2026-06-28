#!/usr/bin/env bash
# Deploy (or update) the sample workloads via Helm. Idempotent — re-run it
# after editing scripts/chart/values.yaml to roll out changes. This is the
# Helm analogue of vantage-surrealdb/scripts/ingress.sh seeding .surql files.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
NAMESPACE="${FIXTURE_NAMESPACE:-demo}"
RELEASE="vantage-fixtures"

if ! command -v helm >/dev/null 2>&1; then
  echo "Error: helm is not installed. See https://helm.sh/docs/intro/install/" >&2
  exit 1
fi

echo "Deploying fixtures (release: $RELEASE, namespace: $NAMESPACE)..."
helm upgrade --install "$RELEASE" "$SCRIPT_DIR/chart" \
  --namespace "$NAMESPACE" \
  --create-namespace \
  --wait \
  --timeout 120s

echo "Waiting for the web deployment to be available..."
kubectl rollout status deployment/web --namespace "$NAMESPACE" --timeout=120s

echo "✅ fixtures deployed. Try: cargo run --example k8s-cli -- core.pods namespace=$NAMESPACE"
