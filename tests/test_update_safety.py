"""Exercise the real updater function against local disposable git repositories."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def git(cwd, *args):
    return subprocess.run(["git", "-c", "core.hooksPath=/dev/null", *args], cwd=cwd,
                          check=True, text=True, capture_output=True).stdout.strip()


class UpdateSafety(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="omen-update-test-")
        self.root = Path(self.tmp.name)
        self.origin = self.root / "origin"
        git(self.root, "init", "--bare", "-b", "main", str(self.origin))
        self.work = self.root / "work"
        git(self.root, "clone", str(self.origin), str(self.work))
        self.configure(self.work)
        (self.work / "src/omen-space-daemon").mkdir(parents=True)
        (self.work / "src/omen-space-daemon/Cargo.toml").write_text("initial manifest\n")
        (self.work / "Cargo.toml").write_text("fixture\n")
        (self.work / "Cargo.lock").write_text("initial lock\n")
        (self.work / "setup.sh").write_text("#!/bin/bash\nprintf 'fixture reexec only\\n'\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "fixture base")
        git(self.work, "push", "origin", "main")
        self.function = re.search(r"(?ms)^prepare_update_repo\(\) \{.*?^\}",
                                  (ROOT / "setup.sh").read_text()).group(0)

    def tearDown(self):
        self.tmp.cleanup()

    def configure(self, repo):
        git(repo, "config", "user.name", "Fixture")
        git(repo, "config", "user.email", "fixture@example.invalid")

    def invoke(self):
        script = 'set -e\nSCRIPT_DIR="$PWD"\nOMEN_UPDATER_UI=1\nOMEN_POST_PULL=0\nemit_step() { :; }\n'
        return subprocess.run(["bash", "-c", script + self.function + "\nprepare_update_repo\n"],
                              cwd=self.work, text=True, capture_output=True,
                              env={**os.environ, "GIT_TERMINAL_PROMPT": "0"})

    def remote_commit(self):
        other = self.root / "other"
        git(self.root, "clone", str(self.origin), str(other))
        self.configure(other)
        (other / "remote-work").write_text("upstream\n")
        git(other, "add", ".")
        git(other, "commit", "-m", "remote change")
        git(other, "push", "origin", "main")

    def test_gui_update_preserves_dirty_manifest_and_lock(self):
        for name in ["Cargo.lock", "src/omen-space-daemon/Cargo.toml"]:
            (self.work / name).write_text("uncommitted local work\n")
        head = git(self.work, "rev-parse", "HEAD")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Local source changes", result.stderr)
        self.assertEqual(git(self.work, "rev-parse", "HEAD"), head)
        self.assertEqual((self.work / "Cargo.lock").read_text(), "uncommitted local work\n")
        self.assertEqual((self.work / "src/omen-space-daemon/Cargo.toml").read_text(), "uncommitted local work\n")

    def test_diverged_gui_update_keeps_local_commit(self):
        self.remote_commit()
        (self.work / "local-work").write_text("keep me\n")
        git(self.work, "add", ".")
        git(self.work, "commit", "-m", "local contribution")
        head = git(self.work, "rev-parse", "HEAD")
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("diverges", result.stderr)
        self.assertEqual(git(self.work, "rev-parse", "HEAD"), head)
        self.assertEqual((self.work / "local-work").read_text(), "keep me\n")

    def test_clean_fast_forward_still_reexecutes_updated_script(self):
        self.remote_commit()
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("fixture reexec only", result.stdout)
        self.assertTrue((self.work / "remote-work").exists())

    def test_driver_failure_stops_installation(self):
        driver = self.root / "driver"
        driver.mkdir()
        stub = driver / "setup.sh"
        stub.write_text("#!/bin/sh\nexit 7\n")
        stub.chmod(0o755)
        source = (ROOT / "setup.sh").read_text()
        line = next(line.strip() for line in source.splitlines() if line.strip().startswith("./setup.sh install"))
        result = subprocess.run(["bash", "-c", "set -e\n" + line + "\nprintf 'unexpected success'\n"],
                                cwd=driver, capture_output=True, text=True)
        self.assertEqual(result.returncode, 7)
        self.assertNotIn("unexpected success", result.stdout)


if __name__ == "__main__":
    unittest.main()
