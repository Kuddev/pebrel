"""Loopback model for native CLI recovery conformance."""
from contextlib import contextmanager, suppress
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import re
from threading import Thread
from urllib.parse import urlsplit


class FixtureModel(BaseHTTPRequestHandler):
    def send_body(self, body, content_type):
        data = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        with suppress(BrokenPipeError, ConnectionResetError):
            self.wfile.write(data)

    def do_GET(self):
        if urlsplit(self.path).path.endswith("/models"):
            self.send_body(json.dumps({"object": "list", "data": [{"object": "model", "id": "fixture"}]}),
                           "application/json")
        else:
            self.send_error(404)

    def do_POST(self):
        path = urlsplit(self.path).path
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if path.endswith("/messages/count_tokens"):
            self.send_body('{"input_tokens":10}', "application/json")
            return
        if not path.endswith(("/messages", "/chat/completions")):
            self.send_error(404)
            return
        # Record only fixture markers from user turns. Diagnostics, summaries
        # and assistant messages cannot attest which conversation was resumed.
        markers = {
            marker for message in request.get("messages", []) if message.get("role") == "user"
            for marker in re.findall(r"pebrel-qa-[0-9a-f]{16}", json.dumps(message.get("content")))
        }
        self.server.turns.append(markers)
        reply = "Native recovery fixture reply."
        if path.endswith("/messages"):
            message = {"id": "msg_qa", "type": "message", "role": "assistant",
                       "model": "fixture", "content": [], "usage": {"input_tokens": 10, "output_tokens": 1}}
            events = [
                ("message_start", {"message": message}),
                ("content_block_start", {"index": 0, "content_block": {"type": "text", "text": ""}}),
                ("content_block_delta", {"index": 0, "delta": {"type": "text_delta", "text": reply}}),
                ("content_block_stop", {"index": 0}),
                ("message_delta", {"delta": {"stop_reason": "end_turn", "stop_sequence": None},
                                   "usage": {"output_tokens": 5}}),
                ("message_stop", {}),
            ]
            wire = "".join(f"event: {kind}\ndata: {json.dumps({'type': kind, **body})}\n\n"
                           for kind, body in events)
        else:
            chunks = [{"id": "chatcmpl-qa", "object": "chat.completion.chunk", "model": "fixture",
                       "created": 1, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
                      for delta, finish in [({"role": "assistant", "content": reply}, None), ({}, "stop")]]
            wire = "".join(f"data: {json.dumps(chunk)}\n\n" for chunk in chunks) + "data: [DONE]\n\n"
        self.send_body(wire, "text/event-stream")

    def log_message(self, *_args):
        pass


@contextmanager
def local_model():
    with ThreadingHTTPServer(("127.0.0.1", 0), FixtureModel) as server:
        server.turns = []
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server
        finally:
            server.shutdown()
            thread.join()
