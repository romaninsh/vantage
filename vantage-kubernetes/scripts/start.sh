#!/usr/bin/env bash
# Start a blank minikube cluster for vantage-kubernetes integration tests,
# enable metrics-server, and wait for the node to be Ready. Mirrors
# vantage-surrealdb/scripts/start.sh (Docker), but for Kubernetes.
set -euo pipefail

PROFILE="${MINIKUBE_PROFILE:-vantage}"

if ! command -v minikube >/dev/null 2>&1; then
  echo "Error: minikube is not installed. See https://minikube.sigs.k8s.io/docs/start/" >&2
  exit 1
fi

echo "Starting minikube (profile: $PROFILE)..."
minikube start --profile "$PROFILE"

echo "Enabling metrics-server addon..."
minikube addons enable metrics-server --profile "$PROFILE"

echo "Pointing kubectl at the '$PROFILE' context..."
kubectl config use-context "$PROFILE"

echo "Waiting for the node to be Ready..."
kubectl wait --for=condition=Ready node --all --timeout=120s

echo "✅ minikube is up. Seed fixtures with: ./scripts/ingress.sh"
