import os
import sys
import unittest

from scripts.conformance.windows_standard_user import WindowsTokens, run_python


@unittest.skipUnless(os.name == "nt", "requires native Windows process tokens")
class StandardUserLaunchTests(unittest.TestCase):
    def test_restricted_token_can_launch_even_when_parent_is_already_ordinary(self):
        api = WindowsTokens()
        token, restricted = api.current_token(0x008B), None
        try:
            restricted = api.restrict(token)
            self.assertFalse(api.elevated(restricted))
            self.assertEqual(api.run(restricted, [sys.executable, "-c", "raise SystemExit(19)"]), 19)
        finally:
            if restricted:
                api.kernel.CloseHandle(restricted)
            api.kernel.CloseHandle(token)

    def test_child_is_unelevated_and_preserves_arguments_and_failure_code(self):
        script = """
import sys
import subprocess
from scripts.conformance.windows_standard_user import WindowsTokens
api = WindowsTokens()
token = api.current_token()
try:
    assert not api.elevated(token), 'child retained administrator elevation'
finally:
    api.kernel.CloseHandle(token)
assert sys.argv[1:] == ['path with spaces\\\\', '中文', 'literal & symbol']
child = subprocess.run([sys.executable, '-c', 'print("pipe output"); raise SystemExit(73)'],
                       input='pipe input', capture_output=True, text=True)
assert child.stdout.strip() == 'pipe output'
raise SystemExit(child.returncode)
"""
        self.assertEqual(run_python(["-c", script, "path with spaces\\", "中文",
                                     "literal & symbol"]), 73)

    def test_successful_child_does_not_change_parent_elevation(self):
        api = WindowsTokens()
        token = api.current_token()
        try:
            before = api.elevated(token)
            self.assertEqual(run_python(["-c", "raise SystemExit(0)"]), 0)
            self.assertEqual(api.elevated(token), before)
        finally:
            api.kernel.CloseHandle(token)


if __name__ == "__main__":
    unittest.main()
