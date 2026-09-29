import base64
import copy
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch

import runtime


class DeploymentContractTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).parent
        self.contract = json.loads((root / "contract.json").read_text())
        self.config = json.loads((root / "config.example.json").read_text())
        self.secrets = {key: "test-only-value" for key in self.contract["secrets"]}
        for key, length in self.contract.get("min_length", {}).items():
            self.secrets[key] = "x" * length
        if "POSTGRES_PASSWORD" in self.secrets:
            user = self.config[self.contract["database_identity"]["user"]]
            database = self.config[self.contract["database_identity"]["database"]]
            self.secrets["DATABASE_URL"] = f"postgres://{user}:test-only-value@postgres/{database}"
        if "DATABASE_CREDENTIALS_ENCRYPTION_KEY" in self.secrets:
            self.secrets["DATABASE_CREDENTIALS_ENCRYPTION_KEY"] = base64.urlsafe_b64encode(b"x" * 32).decode()

    def test_complete_contract(self):
        runtime.validate(self.contract, self.config, self.secrets)

    def test_prepare_missing_github_settings_fails_before_writing_payload(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(runtime, "kube") as kube, patch("sys.argv", ["runtime.py", "prepare", "/tmp/must-not-be-created"]):
            with self.assertRaisesRegex(runtime.Invalid, "Missing GitHub Actions"):
                runtime.main()
            kube.assert_not_called()

    def test_prepare_requires_dedicated_external_secret(self):
        if not self.contract.get("external_secrets"):
            return
        supplied = {k: v for k, v in self.secrets.items() if k not in self.contract["external_secrets"]}
        env = {"SSH_HOST": "example.com", "SSH_USER": "deploy", "SSH_PRIVATE_KEY": "test", "SSH_KNOWN_HOSTS": "test", "K8S_CONFIG_JSON": json.dumps(self.config), "K8S_SECRETS_JSON": json.dumps(supplied)}
        with patch.dict(os.environ, env, clear=True), patch("sys.argv", ["runtime.py", "prepare", "/tmp/must-not-be-created"]):
            with self.assertRaisesRegex(runtime.Invalid, "KNOTREE_REGISTRY_WEBHOOK_SECRET"):
                runtime.main()

    def test_each_missing_env_fails_without_cluster_access(self):
        for key in self.contract["config"]:
            config = dict(self.config)
            del config[key]
            with self.subTest(key=key), self.assertRaises(runtime.Invalid), patch.object(runtime, "kube") as kube:
                runtime.validate(self.contract, config, self.secrets)
            kube.assert_not_called()
        for key in self.contract["secrets"]:
            secrets = dict(self.secrets)
            del secrets[key]
            with self.subTest(key=key), self.assertRaises(runtime.Invalid), patch.object(runtime, "kube") as kube:
                runtime.validate(self.contract, self.config, secrets)
            kube.assert_not_called()

    def test_empty_secret_is_rejected_without_exposing_value(self):
        secrets = dict(self.secrets)
        key = self.contract["secrets"][0]
        secrets[key] = ""
        with self.assertRaisesRegex(runtime.Invalid, key):
            runtime.validate(self.contract, self.config, secrets)

    def test_json_duplicate_and_unknown_keys_fail(self):
        with self.assertRaises(runtime.Invalid):
            runtime.decode('{"DATABASE_URL":"a","DATABASE_URL":"b"}', "secrets")
        with self.assertRaises(runtime.Invalid):
            runtime.validate(self.contract, {**self.config, "TYPO": "bad"}, self.secrets)

    def test_database_password_mismatch_is_rejected(self):
        secrets = {**self.secrets, "DATABASE_URL": "postgres://test:wrong@postgres/test"}
        with self.assertRaises(runtime.Invalid):
            runtime.validate(self.contract, self.config, secrets)

    def test_database_identity_mismatch_is_rejected(self):
        secrets = {**self.secrets, "DATABASE_URL": "postgres://other:test-only-value@postgres/other"}
        with self.assertRaisesRegex(runtime.Invalid, "POSTGRES_USER and POSTGRES_DB"):
            runtime.validate(self.contract, self.config, secrets)

    def test_existing_key_mismatch_prevents_all_mutations(self):
        objects = runtime.manifests(self.contract, self.config, self.secrets)
        name = next(iter(self.contract["preserve"]))
        key = self.contract["preserve"][name][0]
        secret = copy.deepcopy(next(o for o in objects if o["metadata"]["name"] == name))
        secret["data"][key] = base64.b64encode(b"different-existing-value").decode()
        def response(args, *unused, **kwargs):
            self.assertEqual(args[0], "get")
            return json.dumps(secret) if args[2] == name else None
        with patch.object(runtime, "kube", side_effect=response), self.assertRaises(runtime.Invalid):
            runtime.apply(self.contract, self.config, self.secrets)

    def test_statefulset_identity_mismatch_prevents_all_mutations(self):
        calls = []
        def response(args, data=None, **kwargs):
            calls.append(args)
            if args[:2] == ["get", "namespace"]:
                return "namespace knotree-cloud\n"
            if args[:2] == ["get", "statefulset"]:
                return json.dumps({"spec": {"template": {"spec": {"containers": [{"name": "postgres", "env": [
                    {"name": "POSTGRES_USER", "value": "wrong"},
                    {"name": "POSTGRES_DB", "value": self.config["POSTGRES_DB"]},
                ]}]}}}})
            return None
        with patch.object(runtime, "kube", side_effect=response), self.assertRaisesRegex(runtime.Invalid, "identity change"):
            runtime.apply(self.contract, self.config, self.secrets)
        self.assertFalse(any(call[0] in {"apply", "patch"} for call in calls))

    def test_same_keys_can_apply_without_deleting_stateful_resources(self):
        calls = []
        def response(args, data=None, **kwargs):
            calls.append(args)
            return None
        with patch.object(runtime, "kube", side_effect=response):
            runtime.apply(self.contract, self.config, self.secrets)
        self.assertTrue(any(c[0] == "apply" for c in calls))
        self.assertFalse(any(c[0] == "delete" for c in calls))
        self.assertTrue(all("postgres" not in c or c[0] == "get" for c in calls))


if __name__ == "__main__":
    unittest.main()
