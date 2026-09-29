#!/usr/bin/env bash
set -euo pipefail

API_REF="${1:?CI API image digest required}"
WEB_REF="${2:?CI web image digest required}"
[[ "$API_REF" =~ ^ghcr\.io/vantanminh/cloud/api@sha256:[a-f0-9]{64}$ ]]
[[ "$WEB_REF" =~ ^ghcr\.io/vantanminh/cloud/web@sha256:[a-f0-9]{64}$ ]]
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NS=knotree-cloud
RELEASE=knotree-cloud

export PATH="/usr/local/bin:/usr/bin:$PATH"
export KUBECONFIG="${KUBECONFIG:-/etc/rancher/k3s/k3s.yaml}"
cd "$ROOT"

CI_VALUES="${3:?GitHub-rendered runtime Helm values required}"
test -f "$CI_VALUES"
# Configuration and pull/runtime Secrets are applied by the CI remote entrypoint.
# No .env, server-generated passwords, copied credentials or legacy ingress deletion.
helm upgrade --install "$RELEASE" "$ROOT/deploy/helm/knotree-api" \
  --namespace "$NS" \
  --values "$ROOT/deploy/helm/knotree-api/values-edge.yaml" \
  --values "$CI_VALUES" \
  --set image.repository="${API_REF%@*}" \
  --set image.digest="${API_REF#*@}" \
  --set image.pullPolicy=IfNotPresent \
  --set web.image.repository="${WEB_REF%@*}" \
  --set web.image.digest="${WEB_REF#*@}" \
  --set web.image.pullPolicy=IfNotPresent \
  --set migrations.enabled=true \
  --wait \
  --timeout 8m

k3s kubectl -n "$NS" rollout status deploy/knotree-cloud-knotree-api --timeout=240s
k3s kubectl -n "$NS" rollout status deploy/knotree-cloud-knotree-api-web --timeout=180s

API_SVC="$(k3s kubectl -n "$NS" get svc -l app.kubernetes.io/name=knotree-api -o jsonpath='{.items[0].metadata.name}')"
WEB_SVC="${API_SVC}-web"
OUT="${KNOTREE_PROBE_DIR:-/tmp/knotree-probes}"
mkdir -p "$OUT"
k3s kubectl -n "$NS" delete pod probe-healthz probe-web --ignore-not-found
k3s kubectl -n "$NS" run probe-healthz --restart=Never --image=curlimages/curl:8.11.1 -- curl -fsS "http://${API_SVC}:8080/healthz"
k3s kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-healthz --timeout=60s
k3s kubectl -n "$NS" logs probe-healthz | tee "$OUT/healthz.json"
k3s kubectl -n "$NS" run probe-web --restart=Never --image=curlimages/curl:8.11.1 -- curl -fsS "http://${WEB_SVC}:8080/"
k3s kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-web --timeout=60s
k3s kubectl -n "$NS" logs probe-web | tee "$OUT/dashboard.html"
k3s kubectl -n "$NS" delete pod probe-edge --ignore-not-found
if k3s kubectl -n nginx-edge get svc nginx-edge >/dev/null 2>&1; then
  k3s kubectl -n "$NS" run probe-edge --restart=Never --image=curlimages/curl:8.11.1 -- \
    curl -fsS -H 'Host: cloud.knotree.com' \
    "http://nginx-edge.nginx-edge.svc.cluster.local/"
  k3s kubectl -n "$NS" wait --for=jsonpath='{.status.phase}'=Succeeded pod/probe-edge --timeout=60s
  k3s kubectl -n "$NS" logs probe-edge | tee "$OUT/edge-dashboard.html"
else
  echo "nginx-edge service not found; skipped edge dashboard probe" >&2
fi
k3s kubectl -n "$NS" get deploy,sts,svc,ingressroute,pods -o wide
k3s kubectl -n "$NS" get deploy -o jsonpath='{range .items[*]}{.metadata.name}{" "}{.spec.template.spec.containers[*].image}{"\n"}{end}'
echo "deployed ${API_REF} and ${WEB_REF}"
