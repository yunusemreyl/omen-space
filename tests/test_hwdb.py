import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("systemd-hwdb"), "systemd-hwdb is not installed")
class HwdbMapping(unittest.TestCase):
    def test_omen17_record_compiles_and_other_hp_mappings_are_preserved(self):
        source = (ROOT / "src/omen-space-daemon/src/platform.rs").read_text()
        block = source.split("// Always map macro keys", 1)[1].split('let content = lines.join', 1)[0]
        lines = ["evdev:atkbd:dmi:bvn*:bvr*:bd*:svnHP*:pn*:*"]
        for match in re.finditer(r'lines.push\(String::new\(\)\);|lines.push\("([^"\n]*)"\.to_string\(\)\)', block):
            lines.append(match.group(1) or "")
        with tempfile.TemporaryDirectory(prefix="omen-hwdb-test-") as tmp:
            path = Path(tmp) / "etc/udev/hwdb.d"
            path.mkdir(parents=True)
            (path / "99-omen.hwdb").write_text("\n".join(lines) + "\n")
            build = subprocess.run(["systemd-hwdb", "--root", tmp, "--strict", "update"],
                                   capture_output=True, text=True)
            self.assertEqual(build.returncode, 0, build.stderr)
            self.assertNotIn("ignoring", build.stderr)
            for model, expected in [("OMEN17-ck1xxx", "f13"), ("OMENMAX16-ah0xxx", "prog2")]:
                result = subprocess.run(["systemd-hwdb", "--root", tmp, "query",
                                         f"evdev:atkbd:dmi:bvnHP:bvrF.23:bd2026:svnHP:pn{model}:"],
                                        capture_output=True, text=True, check=True)
                self.assertIn(f"KEYBOARD_KEY_95={expected}", result.stdout)


if __name__ == "__main__":
    unittest.main()
