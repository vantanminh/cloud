#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
script_dir="$(cd "$(dirname "$0")" && pwd)"
task_dir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/github-deploy.XXXXXX")"
trap 'rm -rf "$task_dir"' EXIT

# Runs on the GitHub runner. kubectl uses the existing production kubeconfig
# secret; no SSH hop and no node-admin kubeconfig.
python3 "$script_dir/runtime.py" prepare "$task_dir"
install -m 600 /dev/null "$task_dir/kubeconfig"
printf '%s\n' "$KUBE_CONFIG" > "$task_dir/kubeconfig"
export KUBECONFIG="$task_dir/kubeconfig"
server="$(kubectl config view --minify -o jsonpath='{.clusters[0].cluster.server}')"
[[ "$server" == "https://15.235.210.66:6443" ]] || {
  echo 'Deploy failed: kubeconfig must target the production k3s API' >&2
  exit 1
}
kubectl --request-timeout=20s get --raw=/readyz >/dev/null

k3s() {
  if [[ "${1:-}" == "kubectl" ]]; then
    shift
    command kubectl "$@"
  else
    echo 'Deploy failed: node-local k3s commands are not used from GitHub runners' >&2
    exit 1
  fi
}
export -f k3s

bash "$script_dir/remote.sh" "$task_dir/runtime.json" "$@"
