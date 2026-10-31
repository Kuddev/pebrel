# Fast-path SSH shell dispatch decoupled from provider bookkeeping and python

## Status

Implemented on 2026-10-22 (phase 1: dispatch + bookkeeping decoupling; phase
2: SFTP-installed shell assets). This records the connection-critical
bootstrap repair after a silent-fallback incident; it does not claim coverage
of every server-side shell policy.

## Context

Every new SSH pane used to depend on the full integration bootstrap before its
shell channel existed: a `python3 -` snapshot, `plan`/`apply` bookkeeping
(including provider hook merges into `~/.claude/settings.json` and
`~/.codex/config.toml`), then an exec of `remote_shell.py`. When `plan` refused
to proceed — most notably the "edited … preserved" protection after the user's
remote provider config was replaced with a foreign copy (hooks pointing at
another machine's paths, observed on 2026-10-22) — the upstream `prepare`
returned no bootstrap and the pane silently fell back to the sshd login shell
for its lifetime. A bare login shell reports no OSC 7, no `NEBULA|` title and
no `pebrel_git_root`, so the sidebar SSH grouping froze at the injected initial
cwd while a nested fish (which emits its own OSC 7) appeared to work. Both
same-day incidents — the morning pane and every afternoon pane on that host —
had this single root cause; an earlier timeout theory was disproved by direct
measurement (the snapshot round trip is ~100 ms there; codex probes are native
binaries).

## Evidence

[The upstream open path](../../../../nebula_app/src/ssh_session/lifecycle.rs)
keeps `open_shell_channel` byte-identical as the delegated safety net; the
single dispatch point swaps in the branch module. The dispatch script, the
per-connection-pool root cache and the decoupled install policy live in
[fast_path](../../../../nebula_app/src/ssh_session/lifecycle/fast_path.rs),
whose tests also mirror the environment contract of
[remote_shell.py](../../../../nebula_app/res/hooks/remote_shell.py) byte for
byte. Loopback protocol tests drive cache hits, first-exec rejection, the
provider-bookkeeping refusal with and without an installed rcfile, delegated
fallback and per-connection verification in
[lifecycle/tests.rs](../../../../nebula_app/src/ssh_session/lifecycle/tests.rs).
The POSIX-absolute-path gate in `Snapshot::plan`
([remote.rs](../../../../nebula_app/src/ai_hook/remote.rs)) is what keeps
Windows remotes out of the integrated path entirely, before and after this
change.

## Decision

Opening a shell channel no longer depends on provider hook bookkeeping — and,
since phase 2, not on python either.

1. A per-pool-key cache of the confirmed integration root lets a repeat pane
   exec `exec sh -c '<dispatch>'` directly. The dispatch script selects on
   `$SHELL` — bash gets `--rcfile <root>/bashrc -i`, zsh gets the `ZDOTDIR`
   fold, anything else degrades to a plain login shell — and exports the same
   environment contract `remote_shell.py` established. Cache entries are
   written only after a confirmed install, never speculatively: on a host
   without installed files the script's rcfile check would degrade to a bare
   login shell, which is worse than installing.
2. On a cache miss the SFTP layer installs first, without python: an SFTP
   subsystem channel opened on the pooled session discovers the home via
   `canonicalize(".")`; a bashrc whose content matches the embedded assets
   short-circuits with zero writes, a missing or stale one triggers a POSIX
   gate (`exec sh -c true`, so non-POSIX remotes never receive files) and an
   atomic temp-write/rename of the shell assets plus a manifest merge that
   updates only the assets digests. Python-less hosts (clean macOS, minimal
   containers) therefore get full shell integration; provider hooks stay a
   python concern (background verification).
3. When SFTP is unavailable, the decoupled python installer remains the
   fallback (snapshot → plan → apply). A `plan`/`apply` refusal — the
   user-edited-config protection — downgrades only the hook installation: as
   long as an rcfile asset is present, the shell still dispatches and the
   cwd/title/grouping channels stay alive. A host that disabled the
   integration (`--remove`) keeps the opt-out and never dispatches.
4. Any remaining failure delegates to the untouched upstream
   `open_shell_channel` (its own prepare, python bootstrap and request_shell
   fallback). Every degradation is logged as a warning with its cause; the
   historical debug-only records never reached a release log because the
   bin-crate target is outside the logger's allowlist, so the fast path logs
   under the allowed `nebula` target.
5. A background verification re-runs the python install with a generous budget
   once per **new** TCP connection (or after a delegated fallback). It repairs
   provider merges, upgrades assets, and corrects the cached root when the
   host sets `XDG_DATA_HOME` (which the SFTP layer cannot see); panes on
   reused connections send zero probe traffic.

The dispatch script rides argv (`sh -c`), never stdin: the shell channel
already has a PTY, and stdin-fed bytes would be echoed into the pane. Nested
quoting uses the same POSIX single-quote folding as the remote git commands.
The SFTP layer waits for the subsystem reply before the SFTP handshake: a
rejected subsystem fails immediately instead of leaking SSH_FXP_INIT bytes
into the channel stream. The `pebrel-hook` launcher written by SFTP uses PATH
`python3`; on python-capable hosts the background verify rewrites it with the
absolute interpreter path, and on python-less hosts hooks cannot work anyway
(the bridge is python) while shell integration does.

`remote_shell.py` stays installed but is no longer exec'd: it remains the
executable mirror the fast-path tests compare against, and removing it from
the asset manifest would churn ownership bookkeeping for no gain.

## Rejected alternatives

- Raising the install budget or retrying the probe: the measured snapshot is
  ~100 ms; the failures were deterministic bookkeeping refusals, so neither
  helps and both delay fallback on genuinely broken hosts.
- Retrying the upstream `prepare` alone keeps python on the critical path and
  still strands the pane when `plan` refuses for legitimate reasons.
- A `/proc` cwd backfill (the `ssh_sftp::remote_cwd` probe) would revive
  grouping without integration, but costs a remote exec per prompt and leaves
  hooks, branch chips and titles dead; fixing the channel itself is strictly
  better.
- Overriding the "edited … preserved" protection to force provider merges would
  clobber exactly the user edits the marker exists to protect; the shell
  channel must live without it instead.
- A self-locating script (deriving the root from `$XDG_DATA_HOME`) was rejected
  in favor of the explicit cached root: it would let speculative execs race
  hosts that never installed the files.
- Persisting the root cache across restarts was deferred: the first connection
  after restart already installs inline, and persistence adds invalidation
  questions (`--remove`, home changes) for one saved probe.

## Consequences

The first pane on a fresh connection still pays one inline install; every later
pane on that host opens without python. Provider hook installation and the
shell channel fail independently: a user who syncs a foreign settings.json
loses hook events (visible in the warning log) but keeps cwd following,
grouping, branch chips and titles. Remote hosts keep the integration only
while pebrel can install it; users who removed it keep the plain login shell
as before. The upstream python dispatch and the new sh dispatch coexist until
an upstream merge lets the delegated path retire; until then their contracts
are held equal by the mirror test.

## Validation

Unit tests execute the dispatch script under a real `/bin/sh` with stub shells
for all three branches (including set-but-empty `ZDOTDIR`) and compare the
exported environment against `remote_shell.py`. Loopback tests assert: cache
hit ⇒ `bootstrap` is the first wire event; first-exec rejection ⇒ install then
dispatch succeeds; an edited-config refusal with an installed rcfile ⇒ the
shell still dispatches without apply; the same refusal without an rcfile ⇒
delegated upstream fallback with the documented event sequence; reused
connections spawn no verification. The upstream integration and hung-channel
regressions still pass unchanged.

## Supersedes

None.

## Revisit when

- The upstream `open_shell_channel` path is retired (drop the python dispatch
  and `remote_shell.py` exec entirely), and with it the python installer for
  shell assets (provider hooks would remain python's last duty).
- The root cache needs to survive restarts (persist per destination and
  invalidate on `--remove`).
- A real manifest read-modify-write race between the SFTP layer and the
  background python apply is ever observed (introduce ownership locking).
- Hosts setting `XDG_DATA_HOME` turn out to be common (move home/XDG discovery
  off SFTP realpath, e.g. via a one-shot `exec sh` env probe).
- A server-side policy rejects `sh -c` wrapping specifically (the delegated
  path then carries those hosts; revisit before removing it).
