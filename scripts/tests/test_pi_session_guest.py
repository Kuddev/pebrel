"""Execute the actual guest metadata script against isolated native-format files."""
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'nebula_app/src/platform/pi_session.sh'
SESSION = '11111111-2222-4333-8444-555555555555'


@unittest.skipUnless(shutil.which('sh'), 'requires a POSIX guest shell')
class GuestSessionMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        # The guest script consumes shell paths, including when the host is
        # Windows. Resolve the shell's physical root once so macOS /var aliases
        # and Git Bash mount paths also match the script's own PWD.
        self.guest_root = PurePosixPath(subprocess.check_output(
            ['sh', '-c', 'pwd -P'], cwd=self.root, text=True, timeout=4).strip())
        self.cwd = self.root / 'work with spaces'
        self.cwd.mkdir()
        self.config = self.root / 'agent'
        self.config.mkdir()
        self.env = {**os.environ, 'PI_CODING_AGENT_DIR': self.guest_path(self.config)}
        self.env.pop('PI_CODING_AGENT_SESSION_DIR', None)

    def guest_path(self, path):
        return str(self.guest_root.joinpath(*Path(path).relative_to(self.root).parts))

    def native_file(self, root, name='arbitrary name.jsonl', session=SESSION):
        root.mkdir(parents=True, exist_ok=True)
        path = root / name
        path.write_text(json.dumps({'type': 'session', 'id': session}) + '\n'
                        + json.dumps({'type': 'message', 'content': 'PRIVATE-FIXTURE-BODY'}) + '\n', encoding='utf-8')
        return path

    def run_script(self, exact=''):
        exact = self.guest_path(exact) if exact else ''
        result = subprocess.run(['sh', '-s', '--', SESSION, exact], cwd=self.cwd,
                                input=SCRIPT.read_text(encoding='utf-8'), env=self.env,
                                capture_output=True, text=True, timeout=4)
        self.assertNotIn('PRIVATE-FIXTURE-BODY', result.stdout + result.stderr)
        return result

    def test_exact_file_does_not_require_default_directory_or_json_parser(self):
        path = self.native_file(self.root / 'outside')
        self.env['PI_CODING_AGENT_DIR'] = self.guest_path(self.root / 'missing-config')
        result = self.run_script(path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split('\t')[0], self.guest_path(path))

    def test_relative_project_setting_overrides_global_setting(self):
        (self.config / 'settings.json').write_text('{"sessionDir":"missing"}', encoding='utf-8')
        project = self.cwd / '.pi'
        project.mkdir()
        (project / 'settings.json').write_text('{"sessionDir":"project sessions"}', encoding='utf-8')
        path = self.native_file(self.cwd / 'project sessions')
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split('\t')[0], self.guest_path(path))

    def test_environment_override_and_missing_exact_path_search_native_headers(self):
        directory = self.root / 'overridden sessions'
        self.env['PI_CODING_AGENT_SESSION_DIR'] = self.guest_path(directory)
        path = self.native_file(directory)
        self.native_file(directory, 'unrelated.jsonl', 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee')
        result = self.run_script(self.root / 'old-location.jsonl')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertEqual(result.stdout.split('\t')[0], self.guest_path(path))

    def test_duplicate_native_ids_are_both_returned_for_ambiguity_detection(self):
        self.native_file(self.config / 'sessions', 'first.jsonl')
        self.native_file(self.config / 'sessions' / 'project', 'second.jsonl')
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 2)

    def test_malformed_settings_do_not_silently_select_default_history(self):
        (self.config / 'settings.json').write_text('{broken', encoding='utf-8')
        self.native_file(self.config / 'sessions')
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')


if __name__ == '__main__':
    unittest.main()
