from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
import shutil
import subprocess
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNNER = r'''
const input = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const writes = [], logs = [];
process.env.COPILOT_REVIEW_TOKEN = input.missingToken ? '' : 'fixture-token';
process.env.CODERABBIT_FALLBACK_ENABLED = input.fallbackEnabled ? 'true' : 'false';
const list = () => {}, listReviews = () => {}, listForRef = () => {};
const github = {
  rest: {
    pulls: {
      list, listReviews,
      get: async () => ({data: input.current || input.pr}),
    },
    checks: {listForRef},
    issues: {addLabels: async value => {
      if (input.labelError) throw new Error('claim failed');
      writes.push(['claim', value]);
    }},
  },
  paginate: async (method, args) => {
    if (method === list) return [input.pr];
    if (method === listReviews) return input.reviews || [];
    if (method === listForRef) return input.checks;
    if (typeof method === 'string' && method.includes('/rules/branches/')) {
      if (input.policyError) throw new Error('policy unavailable');
      return input.rules;
    }
    throw new Error('unexpected API');
  },
};
const reviewer = {
  rest: {
    users: {getAuthenticated: async () => ({data: {login: input.tokenOwner || 'owner'}})},
    pulls: {requestReviewers: async value => {
      writes.push(['request', value]);
      if (input.requestError) throw new Error('request failed');
    }},
    issues: {createComment: async value => writes.push(['fallback', value])},
  },
  request: async () => {
    if (input.quotaError) throw new Error('quota API failed');
    return {data: {quota_snapshots: {premium_interactions: input.quota || {
      unlimited: false, quota_remaining: 100, overage_permitted: false,
      timestamp_utc: new Date().toISOString(),
    }}}};
  },
};
github.constructor = class {
  constructor({auth}) {
    if (auth !== 'fixture-token') throw new Error('Wrong credential');
    return reviewer;
  }
};
const context = {repo: {owner: 'owner', repo: 'project'}, eventName: input.event || 'workflow_run',
  payload: {inputs: {pull_number: input.number || '7'}}};
const core = {info: value => logs.push(value)};
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
new AsyncFunction('github', 'context', 'core', input.script)(github, context, core)
  .then(() => process.stdout.write(JSON.stringify({writes, logs})))
  .catch(error => process.stdout.write(JSON.stringify({writes, logs, error: error.message})));
'''


class CopilotReviewWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.node = shutil.which('node')
        if not cls.node:
            raise RuntimeError('Node.js is required for the Copilot workflow contract')
        cls.workflow = (ROOT / '.github/workflows/copilot-review.yml').read_text(encoding='utf-8')
        cls.script = textwrap.dedent(cls.workflow.split('          script: |\n', 1)[1])

    def fixture(self):
        return {
            'pr': {'number': 7, 'state': 'open', 'draft': False,
                   'user': {'login': 'contributor', 'type': 'User'},
                   'head': {'sha': 'current'}, 'base': {'ref': 'main'},
                   'labels': [], 'requested_reviewers': []},
            'rules': [{'type': 'required_status_checks', 'parameters': {
                'required_status_checks': [{'context': 'native', 'integration_id': 15368},
                                           {'context': 'architecture', 'integration_id': 15368}]}}],
            'checks': [{'id': i, 'name': name, 'head_sha': 'current', 'status': 'completed',
                        'conclusion': 'success', 'app': {'id': 15368}}
                       for i, name in enumerate(('native', 'architecture'), 1)],
        }

    def run_gate(self, data):
        result = subprocess.run([self.node, '-e', RUNNER],
                                input=json.dumps({'script': self.script, **data}),
                                text=True, encoding='utf-8', capture_output=True, check=True)
        return json.loads(result.stdout)

    def test_review_has_no_automatic_event_subscriptions(self):
        triggers = self.workflow.split('permissions:', 1)[0]
        self.assertIn('  workflow_dispatch:', triggers)
        for automatic in ('workflow_run:', 'pull_request_target:', 'pull_request:',
                          'issues:', 'issue_comment:', 'schedule:', 'push:'):
            self.assertNotIn(automatic, triggers)

    def test_external_ready_pr_claimed_then_requested_after_all_required_ci(self):
        result = self.run_gate(self.fixture())
        self.assertEqual([row[0] for row in result['writes']], ['claim', 'request'])
        self.assertEqual(result['writes'][1][1]['reviewers'], ['copilot-pull-request-reviewer[bot]'])

    def test_owner_draft_closed_and_bot_prs_never_request(self):
        for change in ({'user': {'login': 'OwNeR', 'type': 'User'}}, {'draft': True},
                       {'state': 'closed'}, {'user': {'login': 'dependabot', 'type': 'Bot'}}):
            with self.subTest(change=change):
                data = self.fixture()
                data['pr'].update(change)
                self.assertEqual(self.run_gate(data)['writes'], [])

    def test_failed_pending_skipped_neutral_missing_wrong_app_and_old_checks_defer(self):
        for change in ({'conclusion': 'failure'}, {'status': 'in_progress'},
                       {'conclusion': 'skipped'}, {'conclusion': 'neutral'},
                       {'name': 'unrelated'}, {'app': {'id': 123}}, {'head_sha': 'old'}):
            with self.subTest(change=change):
                data = self.fixture()
                data['checks'][0].update(change)
                self.assertEqual(self.run_gate(data)['writes'], [])

    def test_newer_pending_attempt_cannot_reuse_old_success(self):
        data = self.fixture()
        data['checks'].append({**data['checks'][0], 'id': 99, 'status': 'queued', 'conclusion': None})
        self.assertEqual(self.run_gate(data)['writes'], [])

    def test_previous_review_on_any_commit_prevents_automatic_rereview(self):
        data = self.fixture()
        data['reviews'] = [{'user': {'login': 'copilot-pull-request-reviewer[bot]'}, 'commit_id': 'old'}]
        self.assertEqual(self.run_gate(data)['writes'], [])

    def test_pending_request_or_persistent_claim_prevents_duplicates(self):
        for field, value in (
            ('labels', [{'name': 'copilot-review-requested'}]),
            ('requested_reviewers', [{'login': 'copilot-pull-request-reviewer[bot]'}]),
        ):
            data = self.fixture()
            data['pr'][field] = value
            self.assertEqual(self.run_gate(data)['writes'], [])

    def test_push_draft_close_base_change_or_claim_during_reads_defers(self):
        for change in ({'head': {'sha': 'new'}}, {'draft': True}, {'state': 'closed'},
                       {'base': {'ref': 'release'}}, {'labels': [{'name': 'copilot-review-requested'}]}):
            with self.subTest(change=change):
                data = self.fixture()
                data['current'] = {**data['pr'], **change}
                self.assertEqual(self.run_gate(data)['writes'], [])

    def test_missing_or_unreadable_policy_fails_closed(self):
        data = self.fixture()
        data['rules'] = []
        self.assertEqual(self.run_gate(data)['writes'], [])
        data['policyError'] = True
        self.assertEqual(self.run_gate(data)['error'], 'policy unavailable')

    def test_write_failure_does_not_retry_or_erase_claim(self):
        data = self.fixture()
        data['requestError'] = True
        result = self.run_gate(data)
        self.assertEqual(result['error'], 'request failed')
        self.assertEqual([row[0] for row in result['writes']], ['claim', 'request'])
        data['labelError'] = True
        self.assertEqual(self.run_gate(data)['writes'], [])

    def test_manual_backfill_uses_same_gate_and_validates_number(self):
        data = {**self.fixture(), 'event': 'workflow_dispatch'}
        self.assertEqual(len(self.run_gate(data)['writes']), 2)
        data['number'] = '7; arbitrary code'
        self.assertEqual(self.run_gate(data)['error'], 'Invalid PR number')

    def test_quota_exhaustion_uses_only_enabled_coderabbit(self):
        data = self.fixture()
        data['quota'] = {'unlimited': False, 'quota_remaining': 0, 'overage_permitted': False,
                         'timestamp_utc': datetime.now(timezone.utc).isoformat()}
        self.assertEqual(self.run_gate(data)['writes'], [])
        data['fallbackEnabled'] = True
        result = self.run_gate(data)
        self.assertEqual([row[0] for row in result['writes']], ['claim', 'fallback'])
        self.assertEqual(result['writes'][1][1]['body'], '@coderabbitai review')

    def test_quota_errors_stale_data_paid_overage_and_wrong_identity_fail_closed(self):
        for extra in ({'quotaError': True}, {'tokenOwner': 'other'},
                      {'quota': {'unlimited': False, 'quota_remaining': -1}},
                      {'quota': {'unlimited': False, 'quota_remaining': 10,
                                 'overage_permitted': True, 'timestamp_utc': '2020-01-01T00:00:00Z'}}):
            with self.subTest(extra=extra):
                data = {**self.fixture(), **extra, 'fallbackEnabled': True}
                result = self.run_gate(data)
                self.assertEqual(result['writes'], [])
                self.assertIn('error', result)

    def test_paid_overage_or_stale_allowance_never_routes_to_either_bot(self):
        for quota in (
            {'unlimited': False, 'quota_remaining': 50, 'overage_permitted': True,
             'timestamp_utc': datetime.now(timezone.utc).isoformat()},
            {'unlimited': False, 'quota_remaining': 0, 'overage_permitted': False,
             'timestamp_utc': '2020-01-01T00:00:00Z'},
        ):
            data = {**self.fixture(), 'quota': quota, 'fallbackEnabled': True}
            self.assertEqual(self.run_gate(data)['writes'], [])

    def test_missing_user_token_does_not_spend_or_claim(self):
        result = self.run_gate({**self.fixture(), 'missingToken': True})
        self.assertEqual(result['writes'], [])
        self.assertIn('Missing dedicated', result['error'])

    def test_user_token_has_no_ci_reading_methods(self):
        # The two clients deliberately expose disjoint APIs in the fixture.
        result = self.run_gate(self.fixture())
        self.assertEqual([row[0] for row in result['writes']], ['claim', 'request'])
        self.assertNotIn('github-token: ${{ secrets.', self.workflow)

    def test_available_copilot_does_not_trigger_enabled_fallback(self):
        result = self.run_gate({**self.fixture(), 'fallbackEnabled': True})
        self.assertEqual([row[0] for row in result['writes']], ['claim', 'request'])

    def test_coderabbit_config_disables_independent_automatic_reviews(self):
        config = (ROOT / '.coderabbit.yaml').read_text(encoding='utf-8')
        for entry in ('enabled: false', 'auto_incremental_review: false', 'drafts: false', '- Kuddev'):
            self.assertIn(entry, config)

    def test_coderabbit_does_not_duplicate_previous_ai_review(self):
        data = self.fixture()
        data['reviews'] = [{'user': {'login': 'coderabbitai[bot]'}}]
        self.assertEqual(self.run_gate(data)['writes'], [])

    def test_workflow_never_executes_fork_content_or_approves_merges(self):
        self.assertNotIn('actions/checkout', self.workflow)
        self.assertIn('secrets.COPILOT_REVIEW_TOKEN', self.workflow)
        self.assertNotIn('contents: write', self.workflow)
        self.assertNotIn('createReview', self.script)
        self.assertNotIn('synchronize', self.workflow)
        self.assertNotIn('types: [ready_for_review]', self.workflow)
        self.assertIn("github.event_name == 'workflow_dispatch'", self.workflow)
        self.assertIn('retries: 0', self.workflow)
        self.assertIn('cancel-in-progress: false', self.workflow)


if __name__ == '__main__':
    unittest.main()
