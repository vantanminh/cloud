#!/usr/bin/env bash
set -Eeuo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
payload="${1:?CI runtime payload required}"
shift
[[ $# == 2 && "$1" =~ ^ghcr\.io/vantanminh/cloud/api@sha256:[a-f0-9]{64}$ && "$2" =~ ^ghcr\.io/vantanminh/cloud/web@sha256:[a-f0-9]{64}$ ]] || {
  echo 'Deploy failed: invalid Cloud image references' >&2; exit 1;
}
python3 "$script_dir/runtime.py" apply "$payload"
python3 "$script_dir/runtime.py" values "$payload" "$script_dir/values.json"
bash "$script_dir/../k3s-pull.sh" "$1" "$2" "$script_dir/values.json"
