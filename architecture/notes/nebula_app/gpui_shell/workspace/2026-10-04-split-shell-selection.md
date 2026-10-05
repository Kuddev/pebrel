# Split the captured pane using its launch identity or an explicit selection

## Status

Proposed focused-shell split behavior and optional interactive shell preference.
Adapted WSL identity rules from MomentDerek's upstream
[PR #351](https://github.com/Kuddev/pebrel/pull/351).

## Context

With PowerShell as the default and WSL as the focused pane, the old split path
inherited only a host cwd and supplied no shell, opening PowerShell instead.
Issue [#372](https://github.com/Kuddev/pebrel/issues/372) describes this need.
An optional picker also allows deliberately choosing another shell for the split.

Complete split-layout tab duplication is a separate behavior covered by upstream
[PR #455](https://github.com/Kuddev/pebrel/pull/455); it is not part of this PR.

## Evidence

The adapted #351 head is `c7089aa55ff263a0f3712e9d7dac54a5419a3c3e`.
Its `CopyKind::Split` preserves WSL but resets other shells to Default, so it alone
does not satisfy focused-shell inheritance for host Shell/Profile launches.
The pane already owns a frozen `session_launch` and execution context. Tab metadata
is insufficient for a tab containing several shells.

Shared launch-copy rules remain in `workspace/tab_duplication.rs`; split UI
orchestration and pending request lifetime belong to `workspace/splitting.rs`.
Existing duplication stays single-pane; sharing guest-identity rules does not
introduce the separate layout-reconstruction feature.

## Decision

- Inherit the focused pane's frozen launch (Shell, Profile or SSH), not tab metadata
  or a newly sampled default. Preserve command arguments and WSL distro/user.
  An inherited profile's current cwd takes precedence over its startup cwd.
- Persist `split_shell_picker` through the existing boolean preference authority.
  Missing, empty or invalid values are false. Reset removes the override. Cache it
  in GPUI Settings and apply changes immediately to subsequent interactive requests.
- Reuse the Shell/SSH launcher and its keyboard, search and click interactions.
  Capture pane id plus direction before opening it. Selection splits that surviving
  pane even if focus or tab order changes. Cancellation clears the request without
  a process; a closed source fails visibly without choosing another anchor.
- Explicit choices follow the chosen target's distribution/user and startup directory.
  A WSL guest cwd follows only into the same guest and user. SSH cwd follows only
  into the same host. Never validate a guest path as a host directory, including when
  its quoting cannot be encoded for WSL.
- Runtime API splits remain immediate and never wait for the picker. Keyboard,
  palette and terminal/tab context-menu split actions use the interactive entry.
  Administrator launch is omitted while choosing a split because its existing
  operation opens a separate elevated window, not a split.

## Rejected alternatives

- Integrate #351 unchanged: host shells would still switch to the default.
- Read tab launch metadata: mixed-shell tabs have different per-pane identities.
- Open a new tab after selection: loses the requested split geometry and source.
- Retarget to the new focus after source closure: can silently split an unrelated pane.
- Let the automation API wait for UI confirmation: prevents unattended callers
  from receiving the pane id under its existing synchronous result contract.
- Infer a shell from arbitrary foreground processes: this feature copies Pebrel's
  launch identity, not commands typed inside that shell.
- Include full-layout duplication in this submission: combines separate behavior
  and repeats the scope already under review in #455.

## Consequences

No new dependencies, threads or session schema. The additive preference uses
existing persistence/reset contracts. New messages are provided in English and
Simplified Chinese with the existing fallback for other catalogs. Existing WSL
snapshot consumers use the same spawn identity instead of sampling a later default.
Guest cwd availability still depends on existing directory reporting.

## Validation

Settings default/boolean/round-trip/reset tests, WSL copy/argument tests, GPUI split
shortcut/picker/cancellation/stale-anchor tests and a real settings switch click
cover the contracts. On the focused submission based on upstream `51514bd5`,
native Windows checks passed: settings 82, i18n 23 (1 ignored), splitting 3,
copy rules 5, settings UI 1, WSL rules 26, runtime exec 4, file-tree launch 1,
OSC links 1, completion context 13 and file budgets 2. Architecture unit tests
ran 54 cases (3 skipped); the baseline-relative architecture check passed.
Production GPUI compile checks, the actual `cargo build --locked -p nebula
--bin pebrel --features gpui-shell`, formatting and diff checks passed.
These initial selected checks did not run the full application suite or other
native platforms locally.

The operator reported successful real PowerShell/WSL splitting and shell selection
using the local Windows `target/debug/pebrel.exe` before scope separation. This is
operator-reported manual acceptance, separate from automated fixtures. The full
application suite, authenticated SSH, all theme/DPI states and every distribution
or user combination were not exercised by that report.

The first Windows picker fixture failed because appearance initialization calls
`apply_runtime_settings`, replacing its memory-only preference with the disk value.
The fixture now uses `persist_keys`, `SettingsBytesGuard` and the shared fixture
lock; teardown restores the user's original settings bytes. No production
platform-specific workaround was added. The closed-source case retains another
tab, proving selection cannot fall back to the new focus. Both cases passed again
in the focused submission, independently of the prior integrated run.

CI on `8d6443a` then exposed a different fixture-ownership gap: Windows x64 ran
2809 tests with one failing split-shortcut assertion (the picker opened despite
being disabled), while Linux ran 2646 with one settings persistence assertion
failing (memory was enabled but disk was disabled). The new fixtures held the
process-local mutex but were missing from nextest's existing `theme-studio` group.
Multiple fixture processes could overwrite or restore the same settings file.
With CI's pinned nextest 0.9.146, the saved pre-fix config and four test threads,
the first local run reproduced the identical shortcut assertion at line 151.
The runner's group report placed all four new rendered cases in `@global`.

Register both rendered test modules in the existing one-thread fixture group and
update its exact-list contract test. The registration test fails without the fix
and passes with it. Do not serialize the whole suite, add retries, relax assertions
or change production persistence. The local reproduction uses the existing
`PEBREL_CONFIG_DIR` override so no real user settings are touched.

After registration, all four rendered cases passed eight consecutive nextest
runs (32 passes). The exact fixture-group contract ran 18 tests successfully;
ordinary shell-rule tests stayed in `@global`. The complete Windows native entry
`python scripts/ci_native_tests.py --runner nextest` subsequently passed with
CI's pinned runner/profile and four local test threads: 2809 Rust tests passed,
35 skipped, one doctest passed, and Python suites ran 283/47 cases with 64/11
platform skips. Its separate production GPUI check and final architecture,
formatting and diff checks passed. CI's default parallelism and zero retries
remain unchanged; this is not a guarantee for every resource-load condition.

Retain first-failure evidence: the local Python prerequisite pass initially lacked
UTF-8/default C preprocessing; the first complete Rust attempt lacked the pinned
ConPTY pair and also hit the existing formula-frame assertion under concurrent
load. Both latter cases passed isolated probes. Preparing the standard pinned
runtime, using a fresh test settings directory and four local runner threads
allowed the complete entry to pass without modifying those unrelated tests.
Temporary tools/settings and raw evidence remain uncommitted under `tmp/`.

## Supersedes

None. Extends #351's host split policy while retaining its WSL identity rules.

## Revisit when

Split launches gain first-class elevation or startup-command policies, or launch
snapshot ownership and session reconstruction change.
