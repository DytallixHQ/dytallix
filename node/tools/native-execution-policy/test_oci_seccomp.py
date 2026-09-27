import unittest

from oci_seccomp import deny_list, oci_profile
from render import NO_SOCKET_DENY, SYSTEM_CALL_DENY


class OciSeccompTests(unittest.TestCase):
    def test_deny_list_follows_the_rendered_expression(self):
        names = deny_list(SYSTEM_CALL_DENY)
        for name in ["mount", "umount2", "pivot_root", "memfd_create", "ptrace", "process_vm_writev",
                     "recvmsg", "recvmmsg", "pidfd_getfd", "io_uring_setup", "io_uring_enter",
                     "io_uring_register"]:
            self.assertIn(name, names)
        self.assertNotIn("socket", names)
        self.assertNotIn("pidfd_open", names)
        self.assertNotIn("pidfd_send_signal", names)

    def test_socketless_units_also_deny_sockets(self):
        names = oci_profile(sockets=False)["syscalls"][0]["names"]
        for name in NO_SOCKET_DENY.split():
            self.assertIn(name, names)

    def test_profile_allows_by_default_and_denies_with_eperm(self):
        profile = oci_profile()
        self.assertEqual(profile["defaultAction"], "SCMP_ACT_ALLOW")
        self.assertEqual(profile["syscalls"][0]["action"], "SCMP_ACT_ERRNO")
        self.assertEqual(profile["syscalls"][0]["errnoRet"], 1)

    def test_unknown_groups_and_allow_expressions_are_rejected(self):
        with self.assertRaises(ValueError):
            deny_list("~@unknown")
        with self.assertRaises(ValueError):
            deny_list("mount")

    def test_memory_deny_write_execute_is_opt_in(self):
        self.assertEqual(len(oci_profile()["syscalls"]), 1)
        rules = {r["names"][0]: r for r in oci_profile(mdwe=True)["syscalls"][1:]}
        self.assertEqual(rules["mmap"]["args"][0]["value"], 0x6)
        self.assertEqual(rules["mprotect"]["args"][0]["value"], 0x4)


if __name__ == "__main__":
    unittest.main()
