"""Host selection must never run a tool for the wrong native architecture."""
import unittest

from build_ghostty import zig_host


class GhosttyHostTest(unittest.TestCase):
    def test_linux_and_windows_select_their_own_pinned_archives(self):
        self.assertEqual(zig_host("Linux", "x86_64"), ("zig", "zig.tar.xz"))
        self.assertEqual(zig_host("Windows", "AMD64"), ("zig_windows", "zig-windows.zip"))
        for alias in ("aarch64", "arm64"):
            self.assertEqual(zig_host("Linux", alias), ("zig_linux_arm64", "zig-linux-arm64.tar.xz"))

    def test_unsupported_hosts_fail_before_downloading_a_wrong_tool(self):
        for system, machine in (("Darwin", "arm64"), ("Windows", "ARM64"), ("Linux", "riscv64")):
            with self.subTest(system=system, machine=machine), self.assertRaises(SystemExit):
                zig_host(system, machine)


if __name__ == "__main__":
    unittest.main()
