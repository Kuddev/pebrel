# Duplicate terminal tabs through the cold reconstruction path

## Status

Proposed for issue #372.

## Context

GPUI tab duplication read only the focused terminal and created one pane. A nested
split tab therefore lost both its layout and the other panes' launch identities.
Copying the live tab would instead share entity subscriptions and PTY ownership.

## Evidence

[`duplicate_tab`](../../../../../nebula_app/src/gpui_shell/workspace/tab_duplication.rs)
previously called single-terminal creation. The existing session bridge already
converts the shared split tree and restores each leaf with a fresh pane. SSH
restoration discarded the saved cwd in the generic launch conversion.

## Decision

Capture a transient `TabSession` with each pane's launch, current directory and
custom name, then reuse cold reconstruction at the configured new-tab position.
Preserve the source tree's DFS focus index and ratios using the existing permille
conversion. Supply SSH directories directly to SSH launch without host stat calls.
An imported profile's initial directory yields to its current pane directory.

## Rejected alternatives

- Clone the live tab or PTY: closing or typing in the duplicate could affect the source.
- Create one terminal: loses the requested layout and mixed local/SSH identities.
- Replay split commands: duplicates tree construction and cannot faithfully restore
  arbitrary nesting and ratios.

## Consequences

No dependency, persistence schema or threading model changes. Every copied leaf
gets a new pane id, entity, terminal and session. AI recovery identity, live output,
zoom, divider dragging and broadcast mode are not copied. Ratio precision follows
the existing session contract (nearest permille). WSL identity/parser changes remain
in PR #351, and guest zsh cwd reporting remains in PR #409.

## Validation

The GPUI regression exercises nested local-shell/profile/SSH panes, different
storage and tree orders, non-first focus, names/color, current cwd, fresh terminal
ownership and closing the copy. It uses failed local executables and loopback SSH;
it does not prove remote authentication or native Windows/WSL behavior. All
execution is delegated to GitHub Actions; results are recorded in the PR.

## Supersedes

None.

## Revisit when

The session reconstruction authority changes or layout precision becomes a
user-visible concern.
