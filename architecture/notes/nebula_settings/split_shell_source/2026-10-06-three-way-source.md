# One explicit source for interactive split launches

## Status

Proposed for maintainer review. Replaces the optional boolean preference in PR #484.

## Context

The focused-shell fix removed the ability to deliberately split a configured local
shell beside a WSL or SSH pane. MomentDerek proposed one mutually exclusive source
option, and the owner requested an explicit default and compatibility decision.

## Evidence

- [MomentDerek's three-source proposal](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003205050).
- [Owner's default-shell and migration review](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003895446).
- `nebula_settings/src/split_shell_source.rs` owns the legal values and default.
- `workspace/splitting.rs` already captures the source pane and split direction;
  `tab_duplication::PaneOrigin` already separates host, WSL and SSH locations.

## Decision

- Persist only `split_shell_source=focused|default|ask`. Missing, empty or invalid
  values select `default`; the new settings row is a dropdown, not a toggle.
- `default` resolves the currently configured default Shell/Profile and inherits
  only the pane-origin adapter's host-visible directory. It does not copy the
  focused WSL distribution/user, inject its guest cwd, or pass SSH remote cwd to
  a local process. A usable host cwd takes precedence over profile startup cwd;
  without one, the selected default profile retains its own startup location.
- `focused` keeps the focused pane's Shell/Profile/SSH launch identity and existing
  guest/user/cwd rules. Do not replace main's distribution snapshots or full-layout
  duplication implementation.
- `ask` reuses the existing launcher, captured pane id/direction, cancellation,
  selected-target directory rules and visible closed-source failure.
- Remove the boolean field, parser, runtime cache, UI toggle, reset entry and
  obsolete messages. Do not read, migrate, or fall back to `split_shell_picker`.
  Existing text for that key is inert unknown configuration, not an alias. Its
  literal appears only in compatibility regression inputs and historical notes.
- Saving and restoring the new row use the shared persistence authority. Changes
  apply to subsequent interactive shortcuts, palette and context-menu requests.
  A failed save restores the visible selected value and reports an error.
- Runtime API splitting remains immediate focused inheritance as in the PR's
  existing noninteractive contract; it never opens or waits for the launcher.

## Rejected alternatives

- Boolean plus an implicit default: cannot represent all three exclusive choices.
- Map the old boolean to enum values: explicitly rejected for this revision;
  stale trial settings must not choose a new source implicitly.
- Default to focused: changes the unspecified-preference choice; this revision
  makes default-shell behavior the explicit factory fallback instead.
- Reuse new-tab guest inheritance for default splits: can silently transfer the
  source guest identity/directory into the configured default target.
- Modify layout duplication or guest conversion helpers to add this UI choice:
  those accepted contracts are reused, not replaced.

## Consequences

No new dependencies, threads or session-schema change. English and Simplified
Chinese use typed message ids with the existing fallback for other catalogs.
Existing nextest fixture-group isolation is retained: enum choices still share
real persisted settings across fixture processes. No retry or assertion relaxation.

## Validation

Regression source covers default/parse/round-trip/reset, ignored old-key inputs,
real dropdown selection and live settings, both shortcuts, target/host-only default
inheritance, cancellation and closed-source handling. The initial local delivery
compiled the Windows product without running behavior tests, as requested;
manual UI acceptance remains separate evidence.

The subsequently authorized fork CI on `3ceb349a` passed architecture, lint, size
and Linux native validation. Both Windows and both macOS native jobs failed only
`default_source_uses_the_configured_shell_for_both_split_shortcuts`. Its invalid
in-memory shell preference was not a controlled configured target: reloads could
restore an unspecified default. Windows froze a concrete integrated PowerShell
identity but the test compared it to the `Default` request sentinel; macOS started
a real PTY whose reader violated the deterministic GPUI scheduler's thread owner.
The other split/dropdown cases passed on all five hosts. This is not evidence that
the failed contract or full regression succeeded.

Use a real saved, synthetic profile with a discoverable but non-executable image,
restore both settings files under the existing serial fixture contract, and assert
the exact resolved profile identity rather than a fallback sentinel. Refocus the
original distinct shell before each shortcut so the second cannot pass merely by
inheriting the first default pane. No production thread/launch rules, assertion
relaxation, runner retries or platform skips are introduced. Preserve first-failure
logs; this correction still requires native verification.

## Supersedes

The preference/default decisions in
[the original split proposal](../../nebula_app/gpui_shell/workspace/2026-10-04-split-shell-selection.md).
Captured-request lifetime, guest inheritance and fixture-isolation rationale remain.

## Revisit when

Maintainers request a different factory default or explicit migration policy, or
interactive and automation split-source semantics are deliberately unified.
