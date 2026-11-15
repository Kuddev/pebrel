# Elevation isolation follows the desktop shell, not the process token alone

## Status

Implemented for review. Fixes session restore being silently disabled for the
entire machine on an Administrator desktop session (Windows-only).

## Context

`requires_isolation()` gated every session-persistence write (and single-instance
handover, the runtime control plane, and residency) on the process token being
elevated. The design target was the occasional UAC `runas` window on an ordinary
account: its privileged tabs must not overwrite the ordinary workspace.

The built-in Administrator account ships with UAC Admin Approval Mode disabled
(`FilterAdministratorToken` unset), so explorer.exe and every user process run
at High integrity. On such a desktop every pebrel instance is "elevated", the
isolation trips permanently, and the 1 Hz autosave, window-close and quit
snapshots all become silent no-ops. Session restore then never works on that
machine — while macOS/Linux (`is_elevated()` is a constant `false`) restore
fine, which is what made the defect look platform-specific rather than
policy-specific.

## Evidence

- Machine report (built-in Administrator, High Mandatory Level S-1-16-12288,
  `FilterAdministratorToken` unset): a pebrel instance ran for 40 minutes with
  live tabs while `%APPDATA%\Pebrel\session.json` mtime stayed at the previous
  night's teardown — `SessionPersistence::save_with` returned early on
  `isolated` the whole time.
- `session_persistence.rs` `save_with` opens with `if self.isolated { return Ok(()) }`;
  `SessionPersistence::default` seeds `isolated` from `requires_isolation()`.
- `elevation.rs` previously defined isolation as `is_elevated().unwrap_or(true)`,
  with no reference to the desktop session the window actually belongs to.
- The 2026-10-10 handover note already separates "elevated window owner" from
  "ordinary resident" by comparing two live processes' tokens; the same
  comparison pattern applies here.

## Decision

Isolate only on a privilege mismatch with the desktop shell: the process token
is elevated AND the shell window's process (explorer.exe via `GetShellWindow`)
is not. Both-sides-elevated (Administrator desktop) saves and restores normally;
UAC `runas` on an ordinary account (process High, shell Medium) keeps the
original protection.

If the shell window cannot be queried (SSH/service session), treat the desktop
as non-elevated, which reduces to the previous process-only semantics. The
integrity comparison reads `TokenIntegrityLabel` RID from the shell token and
compares against `SECURITY_MANDATORY_HIGH_RID`; results are cached in a
`OnceLock` because tokens do not change during a process lifetime and the
predicate sits on every persistence path.

A failed **process** token query stays fail-closed and does not enter the
mismatch composition at all: `is_elevated()` returning `Err` isolates
unconditionally, regardless of what the desktop shell query observed. Folding
the error into `unwrap_or(true)` and composing anyway (the first draft) would
let an observed-elevated desktop clear an *unknown* process — the failure can
just as well happen on a genuinely elevated `runas` process, and an unknown
process must not reach the persistence plane (maintainer review, 2026-10-28).
The error path is locked separately from the successful-query truth table in
`a_failed_process_token_query_isolates_regardless_of_the_desktop`.

## Rejected alternatives

- Separate `session.elevated.json` per elevation state: doubles the session
  file per user and leaves an Administrator desktop user on a side track
  forever; the mismatch predicate fixes the actual confusion once.
- Keep isolation but strip privileged launch identities from snapshots:
  Administrator users would still lose their elevated profile tabs (they would
  restore as default shells); the leak being defended against is not the
  layout but cross-privilege overwrite, which the mismatch predicate already
  pins down.

## Consequences

- On Administrator desktop sessions, all `requires_isolation()` gates flip to
  enabled at once: session persistence, resident handover, the runtime
  control-plane port file, and hide-to-tray residency. That is the intent —
  they were all casualties of the same over-broad predicate — but reviewers
  should treat them as one behavioral change, not four.
- An elevated pebrel on an ordinary desktop is still isolated exactly as
  before; the 2026-10-10 handover contract is untouched.
- Instances built before this change (including any resident one) keep the old
  predicate until restarted; their final teardown still writes nothing.

## Validation

- Truth-table unit test `only_a_privilege_mismatch_with_the_desktop_isolates`
  in `platform/elevation/tests.rs`: (elevated, Some(elevated)) → restores,
  (elevated, Some(ordinary)) → isolated, ordinary → never isolated,
  `None` → falls back to the process-only rule.
- Error-path unit test `a_failed_process_token_query_isolates_regardless_
  of_the_desktop`: a failed process token query isolates for every desktop
  observation (elevated / ordinary / absent).
- Live run on the reporting machine (Administrator desktop, High/High): a
  freshly built instance wrote `session.json` within one autosave tick of
  opening a tab, and its clean teardown wrote `clean_exit: true` — both were
  no-ops before the change. The user's resident instance was left untouched.
- Full `session*` test set (201 tests) and `check_architecture.py --base HEAD`
  pass.

## Supersedes

None. The 2026-10-10 privileged-launch-handover note is unaffected; this note
narrows the predicate its caller-compatibility relies on.

## Revisit when

- A pebrel window is ever attached to a desktop whose shell process differs
  from the session's logon shell (custom shell replacements): `GetShellWindow`
  may return null there; the fallback keeps the old semantics, which is safe
  but conservative.
- Multi-session scenarios (Fast User Switching) need per-session rather than
  per-process caching.
