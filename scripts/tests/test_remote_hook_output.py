"""Bounded final-answer previews through the production remote sender."""
import base64
from contextlib import contextmanager
import io
import json
import os
import unittest
from unittest.mock import patch

if __package__:
    from .test_remote_hooks import module, TOKEN
else:
    # CI discovery loads tests as top-level modules; keep package runs working too.
    from test_remote_hooks import module, TOKEN


@unittest.skipUnless(os.name == "posix", "POSIX remote hook sender")
class RemoteHookOutputTests(unittest.TestCase):
    def setUp(self):
        self.bridge = module("remote_bridge")
        self.state = io.BytesIO()
        self.frames = []

    @contextmanager
    def stream(self, *_args):
        yield self.state

    def send(self, source, payload, native=False):
        with patch.object(self.bridge, "stream_state", self.stream), patch.object(
            self.bridge, "write_terminal", lambda osc, *_args, **_kwargs: self.frames.append(osc)
        ):
            self.bridge.send(source, native, "--hooks=full", payload, TOKEN, owner="42:1")
        raw = base64.b64decode(self.frames[-1].split(b";", 3)[3][:-1])
        self.assertLessEqual(len(raw), self.bridge.MAX_ENVELOPE)
        return json.loads(raw.split(b"\n", 1)[1])

    def test_oversized_provider_results_keep_an_explicit_preview(self):
        cases = [
            ("claude", {"hook_event_name": "Stop"}, "last_assistant_message", False),
            ("codex", {"hook_event_name": "Stop"}, "last_assistant_message", True),
            ("codex", {"type": "agent-turn-complete"}, "last-assistant-message", False),
            ("opencode", {"kind": "done"}, "message", False),
            ("pi", {"kind": "done"}, "message", False),
            ("claude", {"hook_event_name": "StopFailure"}, "error", False),
        ]
        for source, fields, key, native in cases:
            with self.subTest(source=source, key=key):
                result = self.send(source, {**fields, key: "中" * 30_000}, native)
                output = result["pebrel_output"]
                self.assertEqual(output["preview"], "中" * 4_000)
                self.assertEqual(output["original_chars"], 30_000)
                self.assertTrue(output["truncated"])
                self.assertEqual(output["reason"], "envelope_limit")

    def test_large_unrelated_fields_do_not_mark_a_complete_answer_as_truncated(self):
        result = self.send("claude", {
            "hook_event_name": "Stop", "last_assistant_message": "complete",
            "unused_tool_output": "X" * 70_000,
        })
        self.assertEqual(result["pebrel_output"]["preview"], "complete")
        self.assertFalse(result["pebrel_output"]["truncated"])

    def test_no_room_for_preview_keeps_the_explicit_missing_data(self):
        result = self.send("claude", {
            "hook_event_name": "Stop", "session_id": "S" * 65_000,
            "last_assistant_message": "中" * 30_000,
        })
        self.assertIsNone(result["pebrel_output"]["preview"])
        self.assertTrue(result["pebrel_output"]["truncated"])
        self.assertEqual(result["pebrel_output"]["original_chars"], 30_000)

    def test_next_turn_does_not_reuse_preview_or_provider_supplied_metadata(self):
        payload = {"hook_event_name": "Stop", "last_assistant_message": "中" * 30_000}
        first = self.send("claude", payload)
        payload["last_assistant_message"] = "NEW_RESULT"
        payload["pebrel_output"] = first["pebrel_output"]
        second = self.send("claude", payload)
        self.assertNotIn("pebrel_output", second)
        self.assertEqual(second["last_assistant_message"], "NEW_RESULT")
        self.assertEqual(second["bridge_sequence"], first["bridge_sequence"] + 1)


if __name__ == "__main__":
    unittest.main()
