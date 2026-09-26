# Color capabilities belong to the local child environment

## Status

Implemented; pending review.

## Context

Windows users report that Codex lacks its sparse composer stars and user-message
backgrounds in Pebrel while they appear in the reference terminal. The same distinction
must work across themes without identifying message roles from screen text.

## Evidence

GPUI `local_options` refreshes a complete Windows environment before `spawn`
calls `tty::setup_env()`. The environment cache preserves its earlier snapshot;
changing the parent environment afterward cannot insert `COLORTERM` into the
already complete child environment. An Explorer launch need not have terminal
capability variables inherited from another terminal.

Codex's `rust-v0.154.0` `tui/src/terminal_palette.rs` returns the default color
for ANSI-16/unknown color levels. `style.rs` uses that result for user-message
backgrounds. `bottom_pane/chat_composer/sparkle.rs` requires truecolor and queried
foreground/background colors. This explains a shared failure path; settings,
model choice and Codex version still control whether its animation runs.

## Decision

After refreshing a local Windows environment, fill missing `TERM` with
`xterm-256color` and missing `COLORTERM` with `truecolor`. Preserve explicit
case-insensitive overrides and `NO_COLOR`/`FORCE_COLOR`. Forward `COLORTERM` through the existing WSL environment allowlist, preserving
explicit forwarding flags. Keep the actual Pebrel terminal identity. Existing OSC color-query and rendering paths remain authoritative.

## Rejected alternatives

- Pretending to be the reference terminal through `WT_SESSION` misstates identity.
- Recoloring arbitrary terminal rows as user messages would invent application
  semantics and damage ordinary command output.
- Hardcoding a Nord-only fix leaves other themes and applications affected.

## Consequences

New local Windows panes advertise implemented capabilities independently of their
launching parent. Already running shells retain their inherited environment.
Providers can still intentionally suppress colors or animations. The two-second
Hook lifetime fix is unrelated to this environment declaration.

## Validation

Eight platform-environment tests pass, including an actual child launched with
an explicitly complete environment. VT replay preserves message surfaces and
Braille stars for all fifteen built-in themes, plus all 225 theme-switch pairs.
A native Debian control returned no `COLORTERM` without forwarding and
`truecolor` with it. These are capability/rendering tests, not a full live Codex
animation acceptance.

## Supersedes

None.

## Revisit when

Terminal capability defaults move into a shared per-child environment builder,
or the Windows environment refresh no longer creates a complete snapshot.
