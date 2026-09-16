#!/usr/bin/env bash
set -euo pipefail

SHA="${1:?image tag / git sha}"
API_IMAGE="${2:-ghcr.io/vantanminh/cloud/api}"
WEB_IMAGE="${3:-ghcr.io/vantanminh/cloud/web}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NS=knotree-cloud
RELEASE=knotree-cloud

export PATH="/usr/local/bin:/usr/bin:$PATH"
cd "$ROOT"

kubectl create namespace "$NS" --dry-run=client -o yaml | kubectl apply -f -
kubectl -n "$NS" delete limitrange --all --ignore-not-found

if kubectl -n knotree-system get secret ghcr-cred >/dev/null 2>&1; then
  kubectl get secret ghcr-cred -n knotree-system -o json \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); d["metadata"]={"name":"ghcr-cred","namespace":"knotree-cloud"}; d.pop("resourceVersion",None); d.pop("uid",None); d.pop("creationTimestamp",None); print(json.dumps(d))' \
    | kubectl apply -f -
fi
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

helm upgrade --install "$RELEASE" "$ROOT/deploy/helm/knotree-api" \
  --namespace "$NS" \
  --set image.repository="${API_IMAGE}" \
  --set image.tag="${SHA}" \
  --set image.pullPolicy=Always \
  --set web.image.repository="${WEB_IMAGE}" \
  --set web.image.tag="${SHA}" \
  --set web.image.pullPolicy=Always \
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
kubectl -n "$NS" get deploy,sts,svc,ingressroute,pods -o wide
kubectl -n "$NS" get deploy -o jsonpath='{range .items[*]}{.metadata.name}{" "}{.spec.template.spec.containers[*].image}{"\n"}{end}'
echo "deployed ${SHA}"
