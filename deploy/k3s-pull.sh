#!/usr/bin/env bash
set -euo pipefail

SHA="${1:?image tag / git sha}"
API_IMAGE="${2:-ghcr.io/vantanminh/cloud/api}"
WEB_IMAGE="${3:-ghcr.io/vantanminh/cloud/web}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NS=knotree-cloud
RELEASE=knotree-cloud

export PATH="/usr/local/bin:/usr/bin:$PATH"
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "$ROOT"

kubectl create namespace "$NS" --dry-run=client -o yaml | kubectl apply -f -
# Copy a durable read-only pull credential. A workflow GITHUB_TOKEN expires
# after the run and cannot authenticate later rescheduling on this node.
PULL_SECRET_NAMESPACE="${PULL_SECRET_NAMESPACE:-knotree}"
PULL_SECRET_NAME="${PULL_SECRET_NAME:-registry-credentials}"
kubectl -n "$PULL_SECRET_NAMESPACE" get secret "$PULL_SECRET_NAME" -o json \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d.get("type")=="kubernetes.io/dockerconfigjson"; assert ".dockerconfigjson" in d.get("data",{}); d["metadata"]={"name":"ghcr-cred","namespace":"knotree-cloud"}; d.pop("immutable",None); print(json.dumps(d))' \
  | kubectl apply -f -
if kubectl -n knotree-system get secret knotree-landing-tls >/dev/null 2>&1; then
  kubectl get secret knotree-landing-tls -n knotree-system -o json \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); d["metadata"]={"name":"knotree-landing-tls","namespace":"knotree-cloud"}; d.pop("resourceVersion",None); d.pop("uid",None); d.pop("creationTimestamp",None); print(json.dumps(d))' \
    | kubectl apply -f -
fi
kubectl -n knotree-system delete ingress knotree-console knotree-console-api --ignore-not-found

if ! kubectl -n "$NS" get secret knotree-api-secrets >/dev/null 2>&1; then
  PG_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(24))')"
  ENC_KEY="$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip("="))')"
  DATABASE_URL="postgres://knotree:${PG_PASSWORD}@knotree-cloud-knotree-api-pg:5432/knotree_cloud"
  kubectl -n "$NS" create secret generic knotree-api-secrets \
    --from-literal=POSTGRES_PASSWORD="$PG_PASSWORD" \
    --from-literal=DATABASE_URL="$DATABASE_URL" \
    --from-literal=DATABASE_CREDENTIALS_ENCRYPTION_KEY="$ENC_KEY"
fi
if ! kubectl -n "$NS" get secret knotree-api-secrets \
  -o jsonpath='{.data.KONG_TRAFFIC_LOG_TOKEN}' | grep -q '[^[:space:]]'; then
  TRAFFIC_LOG_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
  TRAFFIC_LOG_TOKEN_B64="$(printf '%s' "$TRAFFIC_LOG_TOKEN" | base64 | tr -d '\n')"
  kubectl -n "$NS" patch secret knotree-api-secrets --type=merge \
    -p "{\"data\":{\"KONG_TRAFFIC_LOG_TOKEN\":\"$TRAFFIC_LOG_TOKEN_B64\"}}"
fi

helm upgrade --install "$RELEASE" "$ROOT/deploy/helm/knotree-api" \
  --namespace "$NS" \
  --values "$ROOT/deploy/helm/knotree-api/values-edge.yaml" \
  --set image.repository="${API_IMAGE}" \
  --set image.tag="${SHA}" \
  --set image.pullPolicy=IfNotPresent \
  --set web.image.repository="${WEB_IMAGE}" \
  --set web.image.tag="${SHA}" \
  --set web.image.pullPolicy=IfNotPresent \
  --set migrations.enabled=true \
  --set env.appServiceProvisioningEnabled=true \
  --set env.databaseProvisioningEnabled=true \
  --set env.databaseClusterProvider=kubernetes \
  --set env.databaseClusterNamespace="${NS}" \
  --wait \
  --timeout 8m

kubectl -n "$NS" rollout status deploy/knotree-cloud-knotree-api --timeout=240s
kubectl -n "$NS" rollout status deploy/knotree-cloud-knotree-api-web --timeout=180s

API_SVC="$(kubectl -n "$NS" get svc -l app.kubernetes.io/name=knotree-api -o jsonpath='{.items[0].metadata.name}')"
WEB_SVC="${API_SVC}-web"
OUT="${KNOTREE_PROBE_DIR:-/tmp/knotree-probes}"
mkdir -p "$OUT"
kubectl -n "$NS" delete pod probe-healthz probe-web --ignore-not-found
kubectl -n "$NS" run probe-healthz --restart=Never --image=curlimages/curl:8.11.1 -- curl -fsS "http://${API_SVC}:8080/healthz"
kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-healthz --timeout=60s
kubectl -n "$NS" logs probe-healthz | tee "$OUT/healthz.json"
kubectl -n "$NS" run probe-web --restart=Never --image=curlimages/curl:8.11.1 -- curl -fsS "http://${WEB_SVC}:8080/"
kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-web --timeout=60s
kubectl -n "$NS" logs probe-web | tee "$OUT/dashboard.html"
kubectl -n "$NS" delete pod probe-edge --ignore-not-found
if kubectl -n nginx-edge get svc nginx-edge >/dev/null 2>&1; then
  kubectl -n "$NS" run probe-edge --restart=Never --image=curlimages/curl:8.11.1 -- \
    curl -fsS -H 'Host: cloud.knotree.com' \
    "http://nginx-edge.nginx-edge.svc.cluster.local/"
  kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-edge --timeout=60s
  kubectl -n "$NS" logs probe-edge | tee "$OUT/edge-dashboard.html"
else
  echo "nginx-edge service not found; skipped edge dashboard probe" >&2
fi
kubectl -n "$NS" get deploy,sts,svc,ingressroute,pods -o wide
kubectl -n "$NS" get deploy -o jsonpath='{range .items[*]}{.metadata.name}{" "}{.spec.template.spec.containers[*].image}{"\n"}{end}'
echo "deployed ${SHA}"
