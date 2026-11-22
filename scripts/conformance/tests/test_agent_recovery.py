import json
from pathlib import Path
import tempfile
import unittest

from scripts.conformance.agent_recovery.providers import PROFILES, provider_confirmed


class NativeTranscriptTest(unittest.TestCase):
    def test_native_transcript_must_match_the_pane_and_new_user_message(self):
        marker = "pebrel-qa-a91d9f0cba239e10"
        wanted = "0f82b8ec-61ec-458a-a2df-01c32502540a"
        other = "4423c29f-bdfa-4611-91b2-866287bfb5c8"
        part = [{"type": "text", "text": marker}]
        for provider in PROFILES:
            with self.subTest(provider=provider), tempfile.TemporaryDirectory() as directory:
                home = Path(directory)
                profile = home / PROFILES[provider]
                root = profile / ("projects" if provider in ("claude", "codebuddy") else "sessions")
                folder = root / wanted
                folder.mkdir(parents=True)
                if provider in ("pi", "omp"):
                    rows = [{"type": "session", "id": wanted},
                            {"type": "message", "message": {"role": "user", "content": part}}]
                    if provider == "omp":
                        rows.insert(0, {"type": "title", "v": 1})
                elif provider == "claude":
                    rows = [{"type": "user", "sessionId": wanted, "message": {"content": part}}]
                elif provider == "codebuddy":
                    rows = [{"type": "message", "role": "user", "content": part, "sessionId": wanted}]
                else:
                    rows = [{"type": "user", "content": [{"text": f"<user_query>\n{marker}\n</user_query>"}]}]
                path = folder / "chat_history.jsonl"
                content = "".join(json.dumps(row) + "\n" for row in rows)
                path.write_text(content, encoding="utf-8")
                self.assertFalse(provider_confirmed(home, provider, wanted, marker))
                reply = "Native recovery fixture reply."
                if provider in ("pi", "omp", "claude"):
                    assistant = {"type": "assistant" if provider == "claude" else "message",
                                 "sessionId": wanted, "message": {"role": "assistant", "content": reply}}
                else:
                    assistant = {"type": "assistant", "role": "assistant", "content": reply, "sessionId": wanted}
                if provider == "codebuddy":
                    assistant.update(type="message", status="completed", message={"usage": {"output_tokens": 5}})
                content += json.dumps(assistant) + "\n"
                path.write_text(content, encoding="utf-8")
                self.assertTrue(provider_confirmed(home, provider, wanted, marker))
                self.assertFalse(provider_confirmed(home, provider, other, marker))
                self.assertFalse(provider_confirmed(home, provider, wanted, "different-message"))
                path.write_text(content.replace('"user"', '"assistant"'), encoding="utf-8")
                self.assertFalse(provider_confirmed(home, provider, wanted, marker))
