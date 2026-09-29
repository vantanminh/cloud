#!/usr/bin/env python3
"""GitHub-owned runtime configuration. Never log configuration or secret values."""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote, urlsplit


class Invalid(Exception):
    pass


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise Invalid("Duplicate configuration key: " + key)
        result[key] = value
    return result


def decode(value, name):
    try:
        result = json.loads(value, object_pairs_hook=unique)
    except (ValueError, TypeError):
        raise Invalid(name + " must contain valid JSON") from None
    if not isinstance(result, dict):
        raise Invalid(name + " must be a JSON object")
    return result


def validate(contract, config, secrets):
    for name, values, required, optional in [
        ("K8S_CONFIG_JSON", config, contract["config"], contract.get("optional_config", [])),
        ("K8S_SECRETS_JSON", secrets, contract["secrets"], contract.get("optional_secrets", [])),
    ]:
        missing = sorted(set(required) - values.keys())
        if missing:
            raise Invalid(name + " missing: " + ", ".join(missing))
        unknown = sorted(values.keys() - set(required) - set(optional))
        if unknown:
            raise Invalid(name + " unknown keys: " + ", ".join(unknown))
        for key, value in values.items():
            if not isinstance(value, str) or (not value.strip() and key not in contract.get("allow_empty", [])):
                raise Invalid(name + " empty/invalid: " + key)
            if name == "K8S_CONFIG_JSON" and any(ord(c) < 32 for c in value):
                raise Invalid("Configuration contains control characters: " + key)
    for key, length in contract.get("min_length", {}).items():
        if len(secrets.get(key, "")) < length:
            raise Invalid("Secret too short: " + key)
    if config.keys() & secrets.keys():
        raise Invalid("Config and secret keys overlap")
    for key, expected in contract.get("fixed", {}).items():
        if config.get(key) != expected:
            raise Invalid("Invalid production setting: " + key)
    for key in contract.get("https", []):
        url = urlsplit(config[key])
        if url.scheme != "https" or not url.hostname or url.username or url.password or url.query or url.fragment:
            raise Invalid("Expected HTTPS URL: " + key)
    if secrets.get("POSTGRES_PASSWORD"):
        url = urlsplit(secrets["DATABASE_URL"])
        if url.scheme not in ("postgres", "postgresql") or unquote(url.password or "") != secrets["POSTGRES_PASSWORD"]:
            raise Invalid("DATABASE_URL must use POSTGRES_PASSWORD")
    for group in contract.get("secret_groups", []):
        if any(secrets.get(k) for k in group) and not all(secrets.get(k) for k in group):
            raise Invalid("Incomplete secret group: " + ", ".join(group))
    for group in contract.get("cross_groups", []):
        combined = {**config, **secrets}
        if any(combined.get(k) for k in group) and not all(combined.get(k) for k in group):
            raise Invalid("Incomplete configuration/secret group: " + ", ".join(group))
    if config.get("STORAGE_BACKEND") == "r2" and not all(secrets.get(k) for k in ["R2_ENDPOINT", "R2_BUCKET", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY"]):
        raise Invalid("R2 storage requires all four R2 secrets")
    if config.get("PULL_MODE") == "edge" and not all(secrets.get(k) for k in ["EDGE_DOWNLOAD_URL", "EDGE_DOWNLOAD_SECRET"]):
        raise Invalid("Redirect pull mode requires both edge download secrets")
    if "DATABASE_CREDENTIALS_ENCRYPTION_KEY" in secrets:
        try:
            key = secrets["DATABASE_CREDENTIALS_ENCRYPTION_KEY"]
            raw = base64.b64decode(key + "=" * (-len(key) % 4), altchars=b"-_", validate=True)
        except ValueError:
            raise Invalid("Invalid DATABASE_CREDENTIALS_ENCRYPTION_KEY") from None
        if len(raw) != 32:
            raise Invalid("DATABASE_CREDENTIALS_ENCRYPTION_KEY must encode 32 bytes")
    return config, secrets


def resource(kind, name, namespace, values, secret_type="Opaque"):
    obj = {"apiVersion": "v1", "kind": kind, "metadata": {"name": name, "namespace": namespace}}
    if kind == "Secret":
        obj.update(type=secret_type, data={k: base64.b64encode(v.encode()).decode() for k, v in values.items()})
    else:
        obj["data"] = values
    return obj


def manifests(contract, config, secrets):
    ns = contract["namespace"]
    runtime = {k: v for k, v in secrets.items() if k not in contract.get("auxiliary_secrets", [])}
    objects = [resource("ConfigMap", contract["configmap"], ns, config), resource("Secret", contract["runtime_secret"], ns, runtime)]
    docker = json.dumps({"auths": {"ghcr.io": {"auth": base64.b64encode((secrets["GHCR_USERNAME"] + ":" + secrets["GHCR_TOKEN"]).encode()).decode()}}})
    objects.append(resource("Secret", contract["pull_secret"], ns, {".dockerconfigjson": docker}, "kubernetes.io/dockerconfigjson"))
    for item in contract.get("extra_secrets", []):
        objects.append(resource("Secret", item["name"], ns, {dest: secrets[source] for dest, source in item["keys"].items()}))
    return objects


def kube(args, data=None, absent=False):
    result = subprocess.run(["k3s", "kubectl", *args], input=data, text=True, capture_output=True)
    if result.returncode:
        # kubectl errors may embed submitted secret data. Never forward stderr.
        raise Invalid("Kubernetes operation failed (details withheld to protect secrets)")
    if absent and not result.stdout.strip():
        return None
    return result.stdout


def apply(contract, config, secrets):
    objects = manifests(contract, config, secrets)
    ns = contract["namespace"]
    # Validate all immutable credentials BEFORE any mutation. Changing a Secret
    # does not rotate a live PostgreSQL password or re-encrypt stored data.
    for name, keys in contract["preserve"].items():
        current = kube(["get", "secret", name, "-n", ns, "--ignore-not-found", "-o", "json"], absent=True)
        if current:
            existing = json.loads(current).get("data", {})
            desired = next(o["data"] for o in objects if o["kind"] == "Secret" and o["metadata"]["name"] == name)
            for key in keys:
                if key in existing and existing[key] != desired[key]:
                    raise Invalid("Refusing implicit credential rotation: " + name + "/" + key)
    namespace = {"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": ns}}
    kube(["apply", "-f", "-"], json.dumps(namespace))
    # No last-applied annotation containing secret material. Force ownership
    # only for these explicitly CI-owned config/secret objects.
    for obj in objects:
        kube(["apply", "--server-side", "--force-conflicts", "--field-manager=github-runtime", "-f", "-"], json.dumps(obj))
        # Replace only data, preserving metadata/resource identity. This removes
        # stale keys owned by earlier manual/Helm managers without deleting Secrets.
        kube(["patch", obj["kind"].lower(), obj["metadata"]["name"], "-n", ns,
              "--type=json", "--patch-file=/dev/stdin"],
             json.dumps([{"op": "replace", "path": "/data", "value": obj["data"]}]))


def main():
    contract = json.loads(Path(__file__).with_name("contract.json").read_text())
    command = sys.argv[1]
    if command == "prepare":
        missing = [k for k in ["SSH_HOST", "SSH_USER", "SSH_PRIVATE_KEY", "SSH_KNOWN_HOSTS", "K8S_CONFIG_JSON", "K8S_SECRETS_JSON", *contract.get("external_secrets", [])] if not os.environ.get(k, "").strip()]
        if missing:
            raise Invalid("Missing GitHub Actions Secrets/Variables: " + ", ".join(missing))
        for key in ["SSH_HOST", "SSH_USER"]:
            if not re.fullmatch(r"[A-Za-z0-9_.:-]+", os.environ[key]) or os.environ[key].startswith("-"):
                raise Invalid("Invalid SSH setting: " + key)
        supplied = decode(os.environ["K8S_SECRETS_JSON"], "K8S_SECRETS_JSON")
        for key in contract.get("external_secrets", []):
            if key in supplied:
                raise Invalid("Use the dedicated GitHub Actions Secret for " + key)
            supplied[key] = os.environ[key]
        config, secrets = validate(contract, decode(os.environ["K8S_CONFIG_JSON"], "K8S_CONFIG_JSON"), supplied)
        target = Path(sys.argv[2])
        target.mkdir(parents=True, exist_ok=True, mode=0o700)
        path = target / "runtime.json"
        path.write_text(json.dumps({"config": config, "secrets": secrets}))
        path.chmod(0o600)
    else:
        packet = decode(Path(sys.argv[2]).read_text(), "runtime payload")
        config, secrets = validate(contract, packet["config"], packet["secrets"])
        if command == "apply":
            apply(contract, config, secrets)
        elif command == "checksum":
            print(hashlib.sha256(json.dumps(packet, sort_keys=True).encode()).hexdigest())
        elif command == "values":
            env = {dest: config[src] for src, dest in contract["helm_env"].items()}
            values = {"env": env, "runtime": {"enabled": True, "configMap": contract["configmap"]}, "podAnnotations": {"github-runtime/checksum": hashlib.sha256(json.dumps(packet, sort_keys=True).encode()).hexdigest()}, "controlPlaneDatabase": {"user": config["POSTGRES_USER"], "database": config["POSTGRES_DB"]}, "ingress": {"host": urlsplit(config["PUBLIC_UI_URL"]).hostname, "apiHost": urlsplit(config["PUBLIC_API_URL"]).hostname}, "web": {"ingress": {"host": urlsplit(config["PUBLIC_UI_URL"]).hostname}}}
            Path(sys.argv[3]).write_text(json.dumps(values))
        else:
            raise Invalid("Unknown runtime command")


if __name__ == "__main__":
    try:
        main()
    except (Invalid, KeyError, OSError) as error:
        print("Deploy preflight failed: " + str(error), file=sys.stderr)
        sys.exit(1)
