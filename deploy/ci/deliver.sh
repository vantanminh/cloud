#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
script_dir="$(cd "$(dirname "$0")" && pwd)"
task_dir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/github-deploy.XXXXXX")"
trap 'rm -rf "$task_dir"' EXIT

# Runs on the GitHub runner BEFORE SSH or any production mutation.
python3 "$script_dir/runtime.py" prepare "$task_dir"
install -m 600 /dev/null "$task_dir/id_ed25519"
printf '%s\n' "$SSH_PRIVATE_KEY" > "$task_dir/id_ed25519"
printf '%s\n' "$SSH_KNOWN_HOSTS" > "$task_dir/known_hosts"
cp -R "$script_dir/.." "$task_dir/deploy"

remote_args=""
for image in "$@"; do
  [[ "$image" =~ ^[a-z0-9./_-]+(:[a-f0-9]{40}|@sha256:[a-f0-9]{64})$ ]] || {
    echo 'Deploy failed: expected a CI image SHA/digest' >&2
    exit 1
  }
  remote_args+=" $image"
done
[[ -n "$remote_args" ]] || { echo 'Deploy failed: missing image' >&2; exit 1; }
# Sensitive JSON travels only over SSH stdin and exists briefly in a mode-700
# remote temp directory. Never source it, echo it, or upload it as an artifact.
tar -czf - -C "$task_dir" deploy runtime.json | ssh \
  -i "$task_dir/id_ed25519" -o UserKnownHostsFile="$task_dir/known_hosts" \
  -o StrictHostKeyChecking=yes -o IdentitiesOnly=yes \
  -o ServerAliveInterval=30 -o ServerAliveCountMax=10 \
  "${SSH_USER}@${SSH_HOST}" \
  "set -eu; task_dir=\$(mktemp -d /tmp/github-deploy.XXXXXX); chmod 700 \"\$task_dir\"; trap 'rm -rf \"\$task_dir\"' EXIT; tar -xzf - -C \"\$task_dir\"; bash \"\$task_dir/deploy/ci/remote.sh\" \"\$task_dir/runtime.json\"$remote_args"
