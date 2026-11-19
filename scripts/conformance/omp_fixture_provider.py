"""Serve a deterministic assistant turn to the isolated OMP recovery fixture."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
from threading import Thread


REPLY = "PEBREL-LOCAL-RESPONSE"


class Completion(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        self.rfile.read(int(self.headers["Content-Length"]))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for delta, finish in [({"role": "assistant", "content": REPLY}, None), ({}, "stop")]:
            event = {"id": "chatcmpl-fixture", "object": "chat.completion.chunk",
                     "model": "fixture", "created": 1795074000,
                     "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")

    def log_message(self, *_):
        pass


def start_provider(agent_dir: Path):
    server = ThreadingHTTPServer(("127.0.0.1", 0), Completion)
    provider = {"baseUrl": f"http://127.0.0.1:{server.server_port}/v1",
                "api": "openai-completions", "apiKey": "fixture-only",
                "models": [{"id": "fixture", "name": "Fixture"}]}
    (agent_dir / "models.yml").write_text(json.dumps({"providers": {"pebrelfixture": provider}}),
                                          encoding="utf-8")
    Thread(target=server.serve_forever, daemon=True).start()
    return server
