# Shell detection is a process-wide snapshot refreshed by pickers

## Status

Proposed for review with implementation.

## Context

`detect_shells()` enumerates the registry, walks every `PATH` directory through
`terminal_profiles::scan_directory` (canonicalize plus 18 `is_file` probes per
directory), and reads the WSL Lxss key. Each call costs ~17 ms on the development
machine. Cold startup called it from `main.rs` (directory validation),
`session_recovery` (cold-start cwd), `shell_launch::startup_shell` (warm-up) and
again from `TerminalView::new` for every restored pane and every new pane; the
returned shells were often used only to answer "is this WSL?". Startup A/B showed
40–64 ms of visible-time difference with `shell=pwsh`.

## Evidence

Measured 2026-11-13 on Windows 10: one `detect_shells()` call takes ~17.1 ms;
`shell=pwsh` cold startup issued it about five times through the paths above.
`TerminalView::new` resolved the settings shell before the launch match even
when the launch already carried a frozen shell (restore) or was SSH/serial, so
the result was discarded. The `startup_shell` callers in `main.rs`,
`gpui_shell/mod.rs`, `session_recovery.rs` and `configured_directory` only passed
the result to `launch_directory::is_wsl`.

## Decision

`shell_detect` owns one process-wide snapshot (`OnceLock<RwLock<Option<Vec<…>>>>`).
`detect_shells()` returns a clone of the snapshot and runs discovery only on the
first call; concurrent first callers serialize on the write lock. The snapshot is
replaced only by `refresh_detection()`, which eagerly re-runs discovery. The new
tab shell palette (`Workspace::open_shell_palette`) and the settings pane
(`SettingsPane::new`) call it when they open, so a shell installed while Pebrel
runs becomes visible without a restart. All other callers keep reusing the
snapshot, with one exception: an id lookup that misses the snapshot re-runs
discovery once before reporting an unknown id, so `--shell`, IPC requests and
the configured default still resolve a shell installed while Pebrel runs. That
scan happens only on the miss path.

Shell ids that only need the WSL decision no longer resolve a shell at all:
`shell_launch::requests_wsl`/`is_wsl_shell_id` decide from the `-e` program or
the `wsl` / `wsl:<distro>` id (plus the cheap `wsl_executable()` existence check),
and `TerminalView::new` resolves the settings shell lazily only when the frozen
launch has none (the `Default` case). `resolve_shell_id` keeps its contract:
an explicit unknown id still returns an error, and `resolve_id` still returns
`None`, so the cache cannot silently turn a bad id into the default shell.

## Rejected alternatives

- Cache only inside `Display::nebula_detected_shells`: the GPUI paths
  (`main`, restore, panes, palette) never read it, so startup kept paying the
  scan.
- Let detection results expire after a time-to-live: a timer either serves stale
  data or silently re-runs the scan on a UI path; an explicit picker refresh has
  a visible owner.
- Replace the snapshot lazily in `refresh_detection()` (clear only): the next
  reader would pay the scan while opening a picker; eager replacement keeps the
  cost at the explicit boundary and makes the refresh testable.
- Keep resolving the shell for the `is_wsl` checks: the answer follows from the
  requested id/program, so resolution was pure overhead.

## Consequences

A shell installed after startup stays invisible in the pickers' lists until
one of them opens, or until the process restarts; removing a shell likewise
stays listed until then. Resolution is fresher than the lists: an id lookup
that misses the snapshot re-runs discovery once, paying the same ~17 ms pass
the settings fallback paid before this change. Imported terminal profiles are
still read from disk on each picker build. For an imported profile id whose
program is `wsl.exe`, the startup-directory WSL/host decision now follows the
id prefix instead of resolving the profile; the profile itself still resolves
normally when the pane launches.
The snapshot costs one clone of a handful of strings per call. Detection
remains synchronous when a picker opens (~17 ms), the same cost those paths
paid before, but now once per open instead of once per process-wide lookup.

## Validation

- `shell_detect::detection_is_cached_until_an_explicit_refresh` counts discovery
  runs on the calling thread: one scan for repeated `detect_shells()`, one more
  after `refresh_detection()`.
- `shell_detect::resolve_id_keeps_unknown_ids_unresolved` holds the
  no-fall-back contract, including an unregistered `wsl:` id.
- `shell_launch::unknown_id_rescans_detection_before_failing` counts discovery
  on the calling thread: a snapshot miss re-runs discovery exactly once before
  the unknown-id error.
- Existing `shell_detect` and `shell_launch` tests (WSL option parsing, spawn
  composition, startup shell) pass unchanged.
- `cargo check -p pebrel --bin pebrel --features gpui-shell --tests`,
  `cargo fmt --all -- --check` and `scripts/check_architecture.py`.

## Supersedes

None.

## Revisit when

Detection becomes cheap enough to run per call, or a live OS notification for
shell installation makes an explicit refresh unnecessary.
