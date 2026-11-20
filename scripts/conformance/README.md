# Nebula Runtime Conformance

This standard-library-only suite checks user-visible behavior through Nebula's
public Runtime API. It launches one isolated application process with a private
`NEBULA_CONFIG_DIR`, talks to its loopback TCP JSONL endpoint, and only stops
the process it launched.

The cases run in this order:

1. `boot`: startup, Runtime API discovery, default shell, working directory,
   and pane process-tree availability.
2. `echo`: cross-shell command input and terminal-tail reading.
3. `resize`: split-ratio mutation and propagation to both PTY column counts.
4. `split`: nested horizontal/vertical layouts, focus, and cleanup.
5. `scrollback`: a 600-line burst with at least 500 retained history rows.
6. `session`: live autosave, forced process termination, cold restore, a
   custom tab label, and live PTYs in every restored tab.
7. `ssh_loop`: currently skipped because Runtime API v1 has no `ssh.open`
   method. The skip is explicit in the report.
8. `paste`: multiline bracketed-paste transport when the target advertises it,
   or an explicit safety refusal when it does not; neither path may submit.
9. `cjk_roundtrip`: UTF-8/CJK input through Runtime API, PTY, and terminal grid.
10. `close`: last-window shutdown within five seconds. It runs last because it
    intentionally terminates the application.

The suite does not claim coverage of native IME composition or the manual
clipboard confirmation dialog. Those paths require GUI automation and cannot
be reached through Runtime API v1. The CJK and paste cases cover only the
runtime-input paths stated above.

## Run

Run a freshly built binary or packaged artifact. ZIP archives, macOS `.app`
bundles, AppImages, unpacked package directories, and direct executable paths
are accepted.

```powershell
python scripts/conformance/run.py `
  --app target/debug/pebrel.exe `
  --platform windows-x86_64 `
  --output dist/conformance/windows-report.json
```

```sh
python3 scripts/conformance/run.py \
  --app target/release/pebrel \
  --platform linux-x86_64 \
  --output dist/conformance/linux-report.json
```

`report.json` is the default output. Nebula logs are retained next to it in a
`<report-stem>-artifacts` directory. The isolated config and work directories
are temporary. A failed or timed-out run is cleaned up by killing only the
process owned by that run.

The runner validates `golden/common.json` on every run. If a reviewed platform
golden exists, it is compared automatically. The first successful run on a new
platform reports that the golden is absent without treating that absence as a
failure. After reviewing the report, create or replace the platform baseline:

```sh
python3 scripts/conformance/run.py \
  --app path/to/fresh/nebula \
  --platform linux-x86_64 \
  --output dist/conformance/linux-report.json \
  --update-golden
```

`--update-golden` refuses to write a baseline when a case or common invariant
fails. `--no-platform-golden` disables only the platform-specific comparison;
common invariants still apply.

Compare two or more reports after each has passed its common checks:

```sh
python3 scripts/conformance/run.py --compare \
  dist/conformance/windows-report.json \
  dist/conformance/linux-report.json \
  dist/conformance/macos-report.json
```

Only fields documented in `golden/whitelist.json` may vary. Timing and terminal
geometry are removed from platform goldens; default shell/path shape and the
optional SSH result are additionally ignored only for cross-platform report
comparison.

## Protocol Boundary

All operating systems publish the same discovery record in
`<NEBULA_CONFIG_DIR>/runtime.port`. It contains a loopback TCP port and token.
Each connection sends one UTF-8 JSON request line using protocol
`nebula.runtime` version 1 and reads one response line. This suite deliberately
does not use Windows named pipes or Unix-domain sockets because Nebula's actual
Runtime API transport is loopback TCP on every platform.

## Native AI conversation recovery

On a Linux desktop the offline test opens two native clients in one tab and working directory. It checks the saved IDs against each provider's own transcript. After each of three process restarts, a new user message must reach the same native conversation with that pane's earlier message in its model request. The other pane's marker must be absent. Install the CLI under test on PATH. The runner creates its own provider home, enables that provider's hook and serves model responses on loopback.

```sh
python3 scripts/conformance/agent_recovery/run.py --app /path/to/pebrel \
  --agent claude --agent pi --agent omp --agent grok --agent codebuddy
```

The CodeBuddy case launches `codebuddy` in the upper pane and `cbc` in the lower pane. For headless Linux, run the command under `dbus-run-session -- xvfb-run -a`.

Clients that require a signed-in service can run the conversation test on a dedicated Linux desktop. Sign in to the provider with a test account in a separate test home. The selected provider may bill the test turns. This test starts two fresh chats in a disposable project, asks each to retain a different random word, and checks the native ID reported by Pebrel and a fresh response containing the right word after every restart. The second question omits the word. Check PATH when using `agent`, which is also an entry point of Grok Build.

```sh
python3 scripts/conformance/agent_recovery/interactive.py --app /path/to/pebrel \
  --source cursor --test-home /path/to/cursor-test-profile
python3 scripts/conformance/agent_recovery/interactive.py --app /path/to/pebrel \
  --source antigravity --test-home /path/to/antigravity-test-profile
```

The same command accepts any source in [the live test registry](agent_recovery/entries.toml). Missing identity, an unanswered challenge, and a missing executable fail that case. The exhaustive Rust session test separately checks file persistence and supported resume-command selection for every registered source.
