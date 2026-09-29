#!/usr/bin/env bash
set -Eeuo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
payload="${1:?CI runtime payload required}"
shift
[[ $# == 2 && "$1" =~ ^ghcr\.io/vantanminh/cloud/api@sha256:[a-f0-9]{64}$ && "$2" =~ ^ghcr\.io/vantanminh/cloud/web@sha256:[a-f0-9]{64}$ ]] || {
  echo 'Deploy failed: invalid Cloud image references' >&2; exit 1;
}
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
encryption_status="$(k3s secrets-encrypt status)" || {
  echo 'Deploy failed: could not check k3s Secret encryption status.' >&2
  exit 1
}
if ! grep -qx 'Encryption Status: Enabled' <<< "$encryption_status" || \
   ! grep -qx 'Current Rotation Stage: reencrypt_finished' <<< "$encryption_status"; then
  echo 'Deploy failed: enable and finish k3s Secret encryption before applying runtime Secrets.' >&2
  exit 1
fi
python3 "$script_dir/runtime.py" apply "$payload"
python3 "$script_dir/runtime.py" values "$payload" "$script_dir/values.json"
bash "$script_dir/../k3s-pull.sh" "$1" "$2" "$script_dir/values.json"
