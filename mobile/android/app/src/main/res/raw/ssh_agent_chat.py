"""Find the Agent behind this SSH connection's terminal and project its native transcript.

Runs on the server through a separate exec channel, never through the interactive shell.
Input: argv[1] is base64 JSON {"mode": "detect"|"read", "revision": str|null, "before": int|null}.
Output: one line "PEBREL_SSH_CHAT=<json>". Python 3.6 or later, standard library only.
"""
import base64
import glob
import json
import os
import re
import subprocess
import sys
import time

HOME = os.path.expanduser("~")
PAGE_BYTES = 1024 * 1024
# The app reads at most 64 KiB from one exec channel, JSON escaping included.
MESSAGE_BYTES = 8 * 1024
OUTPUT_BYTES = 36 * 1024
MESSAGE_COUNT = 120
KINDS = ("claude", "codex", "pi", "antigravity")


def run(args):
    try:
        return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                              universal_newlines=True, timeout=5).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def processes():
    rows = {}
    text = run(["ps", "-A", "-o", "pid=,ppid=,pgid=,tpgid=,tty=,etimes=,args="])
    timed = bool(text.strip())
    if not timed:
        text = run(["ps", "-A", "-o", "pid=,ppid=,pgid=,tpgid=,tty=,args="])
    for line in text.splitlines():
        fields = line.split(None, 6 if timed else 5)
        if len(fields) < (7 if timed else 6):
            continue
        try:
            pid, ppid, pgid, tpgid = (int(value) for value in fields[:4])
            age = int(fields[5]) if timed else None
        except ValueError:
            continue
        rows[pid] = {"pid": pid, "ppid": ppid, "pgid": pgid, "tpgid": tpgid, "tty": fields[4],
                     "age": age, "args": fields[-1]}
    return rows


def own_terminal(rows):
    """The PTY whose sshd also owns this exec channel. Several candidates are ambiguous."""
    pid, server = os.getpid(), None
    for _ in range(32):
        row = rows.get(pid)
        if row is None:
            break
        if os.path.basename(row["args"].split()[0]).startswith("sshd"):
            server = pid
            break
        pid = row["ppid"]
    if server is None:
        return None

    def under(pid):
        for _ in range(32):
            row = rows.get(pid)
            if row is None or pid <= 1:
                return False
            if row["ppid"] == server:
                return True
            pid = row["ppid"]
        return False

    terminals = {row["tty"] for row in rows.values()
                 if row["tty"] not in ("?", "??", "-") and under(row["pid"])}
    return terminals.pop() if len(terminals) == 1 else None


def agent_kind(args):
    for word in args.split()[:3]:
        name = os.path.basename(word)
        if name == "claude" or word.endswith("claude-code/cli.js"):
            return "claude"
        if name == "codex" or name.startswith("codex-") or word.endswith("codex.js"):
            return "codex"
        if name == "pi" or "pi-coding-agent" in word:
            return "pi"
        if name in ("agy", "antigravity", "antigravity-cli"):
            return "antigravity"
    return None


def find_agent(rows, terminal):
    found = [row for row in rows.values() if row["tty"] == terminal and agent_kind(row["args"])]
    foreground = [row for row in found if row["pgid"] == row["tpgid"]]
    found = foreground or found
    if not found:
        return None
    pids = {row["pid"] for row in found}
    # npm launchers start the native binary as a child; prefer the deepest of one kind.
    found.sort(key=lambda row: row["ppid"] in pids, reverse=True)
    kind = agent_kind(found[0]["args"])
    return kind, [row for row in found if agent_kind(row["args"]) == kind]


def option(args, *names):
    words = args.split()
    for index, word in enumerate(words[:-1]):
        if word in names:
            return words[index + 1]
        for name in names:
            if name.startswith("--") and word.startswith(name + "="):
                return word.split("=", 1)[1]
    return None


def newest(paths, since):
    stamped = []
    for path in paths:
        try:
            stamped.append((os.stat(path).st_mtime, path))
        except OSError:
            pass
    stamped.sort(reverse=True)
    for mtime, path in stamped:
        if since is None or mtime >= since - 2:
            return path
    return None


def codex_cwd(path):
    try:
        with open(path, "rb") as file:
            record = json.loads(file.readline())
        return record.get("payload", {}).get("cwd")
    except (OSError, ValueError, AttributeError):
        return None


def transcript(kind, agents):
    agent = agents[0]
    cwd = None
    try:
        cwd = os.readlink("/proc/%d/cwd" % agent["pid"])
    except OSError:
        pass
    since = time.time() - agent["age"] if agent["age"] is not None else None
    resumed = any(word in agent["args"].split() for word in ("-c", "--continue", "-r", "--resume", "resume"))
    # An Agent that keeps its own transcript open names it exactly.
    patterns = {"claude": "/.claude/projects/", "codex": "/rollout-", "pi": "/.pi/agent/sessions/",
                "antigravity": "/.system_generated/logs/transcript"}
    for row in agents:
        try:
            for fd in os.listdir("/proc/%d/fd" % row["pid"]):
                target = os.readlink("/proc/%d/fd/%s" % (row["pid"], fd))
                if patterns[kind] in target and target.endswith(".jsonl"):
                    return target, cwd
        except OSError:
            pass
    if kind == "claude":
        session = option(agent["args"], "--resume", "-r", "--session-id")
        folders = [os.path.join(HOME, ".claude/projects", re.sub(r"[^A-Za-z0-9]", "-", cwd))] if cwd else []
        if session and re.match(r"^[0-9a-fA-F-]{8,64}$", session):
            for folder in folders + glob.glob(os.path.join(HOME, ".claude/projects/*")):
                path = os.path.join(folder, session + ".jsonl")
                if os.path.isfile(path):
                    return path, cwd
        paths = [path for folder in folders for path in glob.glob(os.path.join(folder, "*.jsonl"))]
    elif kind == "codex":
        days = sorted(glob.glob(os.path.join(HOME, ".codex/sessions/*/*/*")))[-3:]
        paths = [path for day in days for path in glob.glob(os.path.join(day, "rollout-*.jsonl"))
                 if cwd is None or codex_cwd(path) in (cwd, None)]
    elif kind == "pi":
        explicit = option(agent["args"], "--session")
        if explicit and explicit.endswith(".jsonl") and os.path.isfile(explicit):
            return explicit, cwd
        paths = []
        if cwd:
            folder = "--" + re.sub(r"[/\\:]", "-", cwd.lstrip("/\\")) + "--"
            paths = glob.glob(os.path.join(HOME, ".pi/agent/sessions", folder, "*.jsonl"))
    else:
        brain = os.path.join(HOME, ".gemini/antigravity-cli/brain")
        session = option(agent["args"], "--conversation")
        folders = [os.path.join(brain, session)] if session and "/" not in session else glob.glob(os.path.join(brain, "*"))
        paths = []
        for folder in folders:
            logs = os.path.join(folder, ".system_generated/logs")
            for name in ("transcript_full.jsonl", "transcript.jsonl"):
                if os.path.isfile(os.path.join(logs, name)):
                    paths.append(os.path.join(logs, name))
                    break
        if session:
            return (paths[0] if paths else None), cwd
    return newest(paths, None if resumed else since), cwd


def text_of(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "\n".join(part.get("text", "") for part in value if isinstance(part, dict)
                         and part.get("type") in ("text", "Text", "input_text", "output_text")
                         and isinstance(part.get("text"), str))
    return ""


class Projection:
    def __init__(self):
        self.messages = []
        self.calls = {}
        self.pending = []

    def say(self, identity, role, text, event=False):
        if not isinstance(text, str) or not text.strip():
            return
        # Codex records each message twice; pair the two sources but keep real repeats.
        for previous in reversed(self.messages):
            if previous["role"] == role and previous["text"] == text and previous["_event"] != event \
                    and not previous["_paired"]:
                previous["_paired"] = True
                return
        self.messages.append({"id": identity, "role": role, "text": text, "name": None, "detail": None,
                              "complete": True, "truncated": False, "error": False,
                              "_event": event, "_paired": False})

    def call(self, identity, call, name, payload):
        self.calls[call] = len(self.messages)
        self.pending.append(call)
        self.messages.append({"id": identity, "role": "tool", "text": payload, "name": name, "detail": None,
                              "complete": False, "truncated": False, "error": False,
                              "_event": False, "_paired": False})

    def result(self, identity, call, output, error=False):
        if call is None and self.pending:
            call = self.pending[0]
        if call in self.pending:
            self.pending.remove(call)
        index = self.calls.get(call)
        if index is not None:
            message = self.messages[index]
            message.update(detail=output, complete=True, error=bool(error))
        else:
            self.messages.append({"id": identity, "role": "tool", "text": "", "name": None, "detail": output,
                                  "complete": True, "truncated": False, "error": bool(error),
                                  "_event": False, "_paired": False})


def project_codex(record, identity, out):
    payload = record.get("payload") or {}
    kind, item = record.get("type"), payload.get("type")
    if kind == "response_item" and item == "message":
        if payload.get("role") == "assistant" and payload.get("channel") != "analysis":
            out.say(identity, "assistant", text_of(payload.get("content")))
    elif kind == "event_msg" and item == "item_completed":
        entry = payload.get("item") or {}
        role = {"UserMessage": "user", "AgentMessage": "assistant"}.get(entry.get("type"))
        if role and entry.get("phase") != "analysis":
            out.say(identity, role, text_of(entry.get("content")), True)
    elif kind == "event_msg" and item in ("user_message", "agent_message"):
        out.say(identity, "user" if item == "user_message" else "assistant", payload.get("message") or "", True)
    elif kind == "response_item" and item in ("function_call", "custom_tool_call"):
        out.call(identity, payload.get("call_id") or identity, payload.get("name") or "tool",
                 payload.get("arguments") or payload.get("input") or "")
    elif kind == "response_item" and item in ("function_call_output", "custom_tool_call_output"):
        output = payload.get("output")
        if isinstance(output, dict):
            output = output.get("content", "")
        out.result(identity, payload.get("call_id"), text_of(output))


def project_claude(record, identity, out):
    role = {"user": "user", "assistant": "assistant"}.get(record.get("type"))
    if role is None or record.get("isSidechain") is True:
        return
    content = (record.get("message") or {}).get("content")
    if isinstance(content, str):
        out.say(identity, role, content)
        return
    for index, part in enumerate(content or []):
        if not isinstance(part, dict):
            continue
        block = "%s:%d" % (identity, index)
        if part.get("type") == "text":
            out.say(block, role, part.get("text") or "")
        elif part.get("type") == "tool_use":
            out.call(block, part.get("id") or block, part.get("name") or "tool",
                     json.dumps(part.get("input"), ensure_ascii=False, indent=2))
        elif part.get("type") == "tool_result":
            out.result(block, part.get("tool_use_id"), text_of(part.get("content")), part.get("is_error"))


def project_pi(record, identity, out):
    if record.get("type") != "message":
        return
    message = record.get("message") or {}
    role = message.get("role")
    if role == "user":
        out.say(identity, "user", text_of(message.get("content")))
    elif role == "assistant":
        for index, part in enumerate(message.get("content") or []):
            block = "%s:%d" % (identity, index)
            if isinstance(part, dict) and part.get("type") == "text":
                out.say(block, "assistant", part.get("text") or "")
            elif isinstance(part, dict) and part.get("type") == "toolCall":
                out.call(block, part.get("id") or block, part.get("name") or "tool",
                         json.dumps(part.get("arguments"), ensure_ascii=False, indent=2))
    elif role == "toolResult":
        out.result(identity, message.get("toolCallId"), text_of(message.get("content")), message.get("isError"))
    elif role == "bashExecution":
        out.call(identity, identity, "bash", json.dumps({"command": message.get("command", "")}))
        out.result(identity, identity, message.get("output") or "", message.get("exitCode") not in (0, None))


HOUSEKEEPING = {"CONVERSATION_HISTORY", "CHECKPOINT", "SYSTEM_MESSAGE", "GENERIC"}


def project_antigravity(record, identity, out):
    kind, text = record.get("type"), record.get("content")
    if kind == "USER_INPUT":
        match = re.search(r"<USER_REQUEST>(.*?)</USER_REQUEST>", text or "", re.S)
        out.say(identity, "user", (match.group(1) if match else text or "").strip())
    elif kind == "PLANNER_RESPONSE":
        out.say(identity, "assistant", text or "")
        for index, call in enumerate(record.get("tool_calls") or []):
            if isinstance(call, dict):
                block = "%s:%d" % (identity, index)
                arguments = call.get("args", call.get("arguments"))
                out.call(block, block, call.get("name") or "tool",
                         arguments if isinstance(arguments, str) else json.dumps(arguments, ensure_ascii=False, indent=2))
    elif kind and kind not in HOUSEKEEPING and record.get("source") != "USER_EXPLICIT" and out.pending:
        if record.get("status") != "RUNNING":
            out.result(identity, None, text or "", record.get("status") == "ERROR")


PROJECT = {"codex": project_codex, "claude": project_claude, "pi": project_pi,
           "antigravity": project_antigravity}


def trim(message, key):
    value = message[key]
    if isinstance(value, str) and len(value.encode("utf-8")) > MESSAGE_BYTES:
        message[key] = value.encode("utf-8")[:MESSAGE_BYTES].decode("utf-8", "ignore")
        message["truncated"] = True


def read_page(kind, path, revision, before):
    with open(path, "rb") as file:
        info = os.fstat(file.fileno())
        stamp = "%d:%d:%d:%d" % (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
        if before is None and revision == stamp:
            return {"unchanged": True, "revision": stamp}
        end = info.st_size if before is None else before
        if not isinstance(end, int) or end < 0 or end > info.st_size:
            return {"error": "conversation_changed"}
        start = max(0, end - PAGE_BYTES)
        file.seek(max(0, start - 1))
        data = file.read(end - max(0, start - 1))
    offset = max(0, start - 1)
    if start > 0:
        # Begin at the first complete record of the page.
        cut = data.find(b"\n")
        if cut < 0:
            return {"unchanged": False, "revision": stamp, "messages": [], "before": start, "truncated": True}
        offset += cut + 1
        data = data[cut + 1:]
    out, truncated = Projection(), False
    for line in data.splitlines(True):
        identity = str(offset)
        offset += len(line)
        if not line.endswith(b"\n"):
            break  # the Agent is still appending this record
        try:
            record = json.loads(line.decode("utf-8"))
        except ValueError:
            truncated = True
            continue
        if isinstance(record, dict):
            PROJECT[kind](record, identity, out)
    messages, kept = [], 0
    for message in reversed(out.messages):
        trim(message, "text")
        if message["detail"] is not None:
            trim(message, "detail")
        size = len(message["text"].encode("utf-8")) + len((message["detail"] or "").encode("utf-8"))
        if kept + size > OUTPUT_BYTES or len(messages) >= MESSAGE_COUNT:
            truncated = True
            break
        kept += size
        messages.append({key: value for key, value in message.items() if not key.startswith("_")})
    messages.reverse()
    first = int(messages[0]["id"].split(":")[0]) if messages else None
    cursor = first if first and len(messages) < len(out.messages) else (start if start > 0 else None)
    busy = bool(out.messages) and (out.messages[-1]["role"] == "user" or not out.messages[-1]["complete"])
    return {"unchanged": False, "revision": stamp, "messages": messages, "before": cursor,
            "truncated": truncated, "busy": busy}


def main(request):
    rows = processes()
    # The app names the pane PTY when a multiplexer client on this connection shows another terminal.
    requested = request.get("tty")
    if isinstance(requested, str) and re.match(r"^/dev/(pts/[0-9]+|tty[A-Za-z0-9]+)$", requested):
        target = requested[len("/dev/"):]
    else:
        target = own_terminal(rows)
    if target is None:
        return {"error": "terminal_unknown"}
    found = find_agent(rows, target)
    if found is None:
        return {"agent": None}
    kind, agents = found
    result = {"agent": kind}
    if request.get("mode") != "read":
        return result
    path, cwd = transcript(kind, agents)
    result.update(cwd=cwd or "", session=path or "")
    if path is None:
        result.update(unchanged=False, revision="", messages=[], before=None, truncated=False, busy=False)
        return result
    try:
        result.update(read_page(kind, path, request.get("revision"), request.get("before")))
    except OSError:
        result["error"] = "conversation_unavailable"
    return result


if __name__ == "__main__":
    try:
        reply = main(json.loads(base64.b64decode(sys.argv[1]).decode("utf-8")))
    except Exception:  # never echo a traceback that could carry transcript text
        reply = {"error": "conversation_unavailable"}
    reply["version"] = 1
    sys.stdout.write("PEBREL_SSH_CHAT=" + json.dumps(reply, ensure_ascii=False, separators=(",", ":")) + "\n")
