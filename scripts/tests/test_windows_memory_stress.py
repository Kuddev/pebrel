import unittest
from unittest.mock import Mock
import subprocess

from scripts.windows_memory_stress import MIB, close_owned_process, summarize


def sample(phase, commit, resident=40):
    return {"phase": phase, "private_commit_bytes": commit * MIB,
            "private_working_set_bytes": resident * MIB, "working_set_bytes": (resident + 50) * MIB}


def complete_samples():
    return [sample("idle", 100), sample("settled-0", 130), sample("settled-1", 131),
            sample("tabs-open-0", 170), sample("tabs-closed-0", 132),
            sample("tabs-open-1", 171), sample("tabs-closed-1", 133)]


class WindowsMemoryStressTests(unittest.TestCase):
    def checkpoints(self):
        return [{"phase": row["phase"], "handles": 700, "threads": 120}
                for row in complete_samples() if not row["phase"].startswith("tabs-open")]

    def test_warm_cache_and_reclaimed_tabs_fit_both_budgets(self):
        result = summarize(complete_samples(), 2, 180, 4)
        self.assertTrue(result["budget_enforced"])
        self.assertEqual(result["violations"], [])
        self.assertEqual(result["tab_growth_after_warmup_bytes"], MIB)

    def test_falling_working_set_does_not_hide_private_commit_growth(self):
        samples = complete_samples()
        samples[-1] = sample("tabs-closed-1", 155, resident=10)
        result = summarize(samples, 2, 180, 4)
        self.assertEqual(len(result["violations"]), 1)
        self.assertIn("retained", result["violations"][0])

    def test_transient_peak_is_checked_even_when_all_tabs_release(self):
        samples = complete_samples() + [sample("tabs-open-1", 300)]
        result = summarize(samples, 2, 180, 4)
        self.assertEqual(result["sampled_peak_private_commit_bytes"], 300 * MIB)
        self.assertEqual(len(result["violations"]), 1)
        self.assertIn("peak", result["violations"][0])

    def test_late_release_does_not_hide_earlier_retained_growth(self):
        samples = complete_samples()
        samples[-1] = sample("tabs-closed-1", 155)
        samples += [sample("settled-2", 130), sample("tabs-closed-2", 132)]
        result = summarize(samples, 3, 180, 4)
        self.assertGreater(result["tab_growth_after_warmup_bytes"], 4 * MIB)
        self.assertEqual(len(result["violations"]), 1)

    def test_incomplete_run_cannot_report_a_budget_pass(self):
        with self.assertRaisesRegex(ValueError, "tabs-closed-1"):
            summarize(complete_samples()[:-1], 2, 180, 4)
        with self.assertRaises(ValueError):
            summarize([], 2, 180, 4)

    def test_observation_without_limits_does_not_claim_a_budget(self):
        result = summarize(complete_samples(), 2, None, None)
        self.assertFalse(result["budget_enforced"])
        self.assertIsNone(result["peak_budget_mib"])
        self.assertIsNone(result["growth_budget_mib"])

    def test_resource_leak_fails_even_when_memory_is_below_budget(self):
        checkpoints = self.checkpoints()
        checkpoints[-1].update(handles=736, threads=128)
        result = summarize(complete_samples(), 2, 180, 4, checkpoints=checkpoints,
                           handle_growth_limit=8, thread_growth_limit=4)
        self.assertEqual(result["resource_growth_after_warmup"], {"handles": 36, "threads": 8})
        self.assertEqual(len(result["violations"]), 2)

    def test_missing_resource_checkpoints_cannot_pass_resource_budgets(self):
        with self.assertRaisesRegex(ValueError, "checkpoints"):
            summarize(complete_samples(), 2, 180, 4, handle_growth_limit=8)
        with self.assertRaisesRegex(ValueError, "tabs-closed-1"):
            summarize(complete_samples(), 2, 180, 4, checkpoints=self.checkpoints()[:-1],
                      thread_growth_limit=4)

    def test_invalid_budgets_cannot_disable_the_gate(self):
        for limit in (float("nan"), float("inf"), -1):
            with self.subTest(limit=limit), self.assertRaises(ValueError):
                summarize(complete_samples(), 2, limit, 4)

    def test_normal_window_close_does_not_force_terminate(self):
        process = Mock(pid=123)
        process.poll.return_value = None
        process.wait.side_effect = lambda **_: setattr(process.poll, "return_value", 0)
        native = Mock()
        result = close_owned_process(process, native)
        self.assertTrue(result["normal_exit"])
        self.assertFalse(result["forced_termination"])
        process.terminate.assert_not_called()

    def test_sampler_initialization_failure_still_reclaims_its_process(self):
        process = Mock(pid=123)
        process.poll.return_value = None
        process.wait.side_effect = lambda **_: setattr(process.poll, "return_value", 1)
        result = close_owned_process(process, None)
        self.assertFalse(result["normal_exit"])
        self.assertTrue(result["forced_termination"])
        process.terminate.assert_called_once()

    def test_a_crash_cannot_count_as_normal_cleanup(self):
        process = Mock(pid=123)
        process.poll.return_value = -1073741819
        result = close_owned_process(process, Mock())
        self.assertFalse(result["normal_exit"])
        self.assertFalse(result["forced_termination"])
        process.terminate.assert_not_called()

    def test_hung_close_is_a_failure_even_if_forced_cleanup_succeeds(self):
        process = Mock(pid=123)
        process.poll.return_value = None

        def wait(**_):
            if not process.terminate.called:
                raise subprocess.TimeoutExpired("owned test process", 15)
            process.poll.return_value = 1

        process.wait.side_effect = wait
        result = close_owned_process(process, Mock())
        self.assertFalse(result["normal_exit"])
        self.assertTrue(result["forced_termination"])
        self.assertTrue(result["errors"])


if __name__ == "__main__":
    unittest.main()
