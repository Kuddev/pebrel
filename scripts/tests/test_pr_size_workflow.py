from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNNER = r'''
const fs = require('node:fs');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
const writes = [], failures = [], summaries = [];
const listFiles = () => {}, listComments = () => {};
const github = {
  rest: {
    pulls: {listFiles},
    issues: {
      listComments,
      addLabels: async (value) => writes.push(['add', value]),
      removeLabel: async (value) => writes.push(['remove', value]),
      createComment: async (value) => writes.push(['comment', value]),
    },
  },
  paginate: async (method) => method === listFiles ? input.files : [],
};
const summary = {addRaw(value) {summaries.push(value); return this;}, async write() {}};
const core = {info() {}, setFailed(value) {failures.push(value);}, summary};
const context = {
  repo: {owner: 'base', repo: 'project'},
  payload: {pull_request: {
    number: 1, labels: input.labels,
    head: {repo: input.deleted ? null : {full_name: input.fork ? 'fork/project' : 'base/project'}},
  }},
};
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
new AsyncFunction('github', 'context', 'core', input.script)(github, context, core)
  .then(() => process.stdout.write(JSON.stringify({writes, failures, summaries})))
  .catch((error) => {console.error(error); process.exit(1);});
'''


class PrSizeWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.node = shutil.which('node')
        if cls.node is None:
            raise RuntimeError('Node.js is required for the PR size workflow contract')
        workflow = (ROOT / '.github/workflows/pr-size.yml').read_text(encoding='utf-8')
        # Execute the actual inline action, without duplicating its policy.
        cls.script = textwrap.dedent(workflow.split('          script: |\n', 1)[1])

    def run_size(self, lines, *, fork=True, exempt=False, deleted=False):
        data = {
            'script': self.script, 'fork': fork, 'deleted': deleted,
            'labels': [{'name': 'size-exempt'}] if exempt else [],
            'files': [
                {'filename': 'src/example.rs', 'additions': lines, 'deletions': 0},
                {'filename': 'Cargo.lock', 'additions': 9000, 'deletions': 500},
                {'filename': 'docs/guide.md', 'additions': 9000, 'deletions': 500},
            ],
        }
        result = subprocess.run(
            [self.node, '-e', RUNNER], input=json.dumps(data), text=True,
            encoding='utf-8', capture_output=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_small_fork_passes_without_write_permissions(self):
        result = self.run_size(127)
        self.assertEqual(result['failures'], [])
        self.assertEqual(result['writes'], [])
        self.assertIn('127 counted lines', result['summaries'][0])

    def test_large_fork_is_rejected_before_optional_writes(self):
        result = self.run_size(1501)
        self.assertEqual(len(result['failures']), 1)
        self.assertIn('1501', result['failures'][0])
        self.assertEqual(result['writes'], [])

    def test_exact_limit_and_existing_maintainer_exemption(self):
        self.assertEqual(self.run_size(1500)['failures'], [])
        self.assertEqual(self.run_size(1501, exempt=True)['failures'], [])

    def test_same_repository_keeps_labels_and_oversize_guidance(self):
        small = self.run_size(127, fork=False)
        self.assertEqual(small['failures'], [])
        self.assertEqual(small['writes'][0][1]['labels'], ['size/M'])
        large = self.run_size(1501, fork=False)
        self.assertEqual(len(large['failures']), 1)
        self.assertEqual(large['writes'][0][1]['labels'], ['size/XL'])
        self.assertEqual(large['writes'][1][0], 'comment')

    def test_deleted_fork_can_still_be_counted_without_writes(self):
        result = self.run_size(127, deleted=True)
        self.assertEqual(result['failures'], [])
        self.assertEqual(result['writes'], [])


if __name__ == '__main__':
    unittest.main()
