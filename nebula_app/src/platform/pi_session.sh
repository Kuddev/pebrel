# Read native metadata inside the owning WSL distribution/user. No chat content
# or provider request is sent. The caller bounds execution time and output size.
set -eu
expected="$1"
exact="$2"
emit_header() {
    header=$(head -c 16384 "$1" | head -n 1)
    case "$header" in
        *"$expected"*) printf '%s\t%s\n' "$1" "$header" ;;
    esac
}
if [ -n "$exact" ] && [ -f "$exact" ]; then
    printf '%s\t' "$exact"
    head -c 16384 "$exact" | head -n 1
    exit 0
fi
expand_directory() {
    case "$1" in
        '~') printf '%s\n' "$HOME" ;;
        '~/'*) printf '%s/%s\n' "$HOME" "${1#??}" ;;
        /*) printf '%s\n' "$1" ;;
        '') exit 1 ;;
        *) printf '%s/%s\n' "$PWD" "$1" ;;
    esac
}
config=$(expand_directory "${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}")
if [ -n "${PI_CODING_AGENT_SESSION_DIR:-}" ]; then
    root=$(expand_directory "$PI_CODING_AGENT_SESSION_DIR")
else
    custom=''
    # Read settings only when needed. Standalone Pi does not imply that Node is
    # installed; use a guest parser if present, otherwise report unavailability.
    if [ -f "$PWD/.pi/settings.json" ] || [ -f "$config/settings.json" ]; then
        if command -v python3 >/dev/null 2>&1; then
            custom=$(python3 -c '
import json, pathlib, sys
for name in sys.argv[1:]:
    path = pathlib.Path(name)
    if not path.exists():
        continue
    with path.open("rb") as f:
        raw = f.read(65537)
    if len(raw) > 65536:
        sys.exit(1)
    data = json.loads(raw)
    value = data.get("sessionDir")
    if value is not None:
        if not isinstance(value, str) or not value or any(ord(c) < 32 for c in value):
            sys.exit(1)
        print(value)
        break
' "$PWD/.pi/settings.json" "$config/settings.json")
        elif command -v node >/dev/null 2>&1; then
            custom=$(node -e '
const fs = require("node:fs");
for (const path of process.argv.slice(1)) {
    if (!fs.existsSync(path)) continue;
    const fd = fs.openSync(path, "r");
    const bytes = Buffer.alloc(65537);
    let count;
    try { count = fs.readSync(fd, bytes, 0, bytes.length, 0); }
    finally { fs.closeSync(fd); }
    if (count > 65536) process.exit(1);
    const value = JSON.parse(bytes.subarray(0, count).toString("utf8")).sessionDir;
    if (value == null) continue;
    if (typeof value !== "string" || !value || /[\x00-\x1f\x7f]/.test(value)) process.exit(1);
    console.log(value);
    break;
}
' "$PWD/.pi/settings.json" "$config/settings.json")
        else
            exit 1
        fi
    fi
    root=$(expand_directory "${custom:-$config/sessions}")
fi
[ -d "$root" ] || exit 1
find "$root" -maxdepth 2 -type f -name '*.jsonl' -print |
while IFS= read -r file; do
    emit_header "$file"
done
