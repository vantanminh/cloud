import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class RemoteEncryptionGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.tools = Path(self.temp.name)
        self.marker = self.tools / "python-was-called"
        k3s = self.tools / "k3s"
        k3s.write_text(
            "#!/bin/sh\nprintf '%s\\n' \"$K3S_TEST_STATUS\"\n"
            "exit \"${K3S_TEST_EXIT:-0}\"\n"
        )
        k3s.chmod(0o755)
        python = self.tools / "python3"
        python.write_text(f"#!/bin/sh\ntouch '{self.marker}'\nexit 77\n")
        python.chmod(0o755)

    def tearDown(self):
        self.temp.cleanup()

    def run_remote(self, status, exit_code="0"):
        env = dict(os.environ)
        env.update({
            "PATH": f"{self.tools}:{env['PATH']}",
            "K3S_TEST_STATUS": status,
            "K3S_TEST_EXIT": exit_code,
        })
        return subprocess.run(
            ["bash", str(Path(__file__).with_name("remote.sh")), "/tmp/payload.json",
             "ghcr.io/vantanminh/cloud/api@sha256:" + "a" * 64,
             "ghcr.io/vantanminh/cloud/web@sha256:" + "b" * 64],
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_disabled_encryption_stops_before_runtime_apply(self):
        result = self.run_remote(
            "Encryption Status: Disabled\nCurrent Rotation Stage: reencrypt_finished"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("enable and finish k3s Secret encryption", result.stderr)
        self.assertFalse(self.marker.exists())

    def test_incomplete_rotation_stops_before_runtime_apply(self):
        result = self.run_remote(
            "Encryption Status: Enabled\nCurrent Rotation Stage: reencrypt_active"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_status_command_error_stops_before_runtime_apply(self):
        result = self.run_remote("", exit_code="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not check k3s Secret encryption", result.stderr)
        self.assertFalse(self.marker.exists())


if __name__ == "__main__":
    unittest.main()
