"""The helper-control screen finds cgroup control writes and delegation."""
from pathlib import Path
import tempfile
import unittest

import check_helper_control as screen


class HelperControlTests(unittest.TestCase):
    def scan(self, relative, text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / relative
            path.parent.mkdir(parents=True)
            path.write_text(text)
            return screen.findings(root)

    def test_clean_source_passes(self):
        self.assertEqual(self.scan("crates/x/src/lib.rs", "pidfd_send_signal(fd, SIGSTOP)\n"), [])

    def test_cgroup_writes_and_delegation_fail(self):
        for relative, text in [
            ("crates/x/src/lib.rs", 'fs::write(dir.join("cgroup.procs"), pid)\n'),
            ("crates/x/src/lib.rs", 'let freeze = "cgroup.freeze";\n'),
            ("deploy/unit.service", "Delegate=yes\n"),
            ("consensus/owner-guard/x.go", 'os.WriteFile("/sys/fs/cgroup/x", b, 0)\n'),
        ]:
            self.assertTrue(self.scan(relative, text), relative)

    def test_tests_are_not_scanned(self):
        self.assertEqual(self.scan("crates/x/tests/cgroup.rs", "cgroup.procs\n"), [])


if __name__ == "__main__":
    unittest.main()
