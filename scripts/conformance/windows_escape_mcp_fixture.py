import json
import sys

for raw in sys.stdin:
    message = json.loads(raw)
    if "id" not in message:
        continue
    method = message.get("method")
    if method == "initialize":
        result = {
            "protocolVersion": message["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "pebrel-escape-fixture", "version": "1"},
        }
    elif method == "tools/list":
        result = {"tools": []}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
