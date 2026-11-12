# Native title attention in the Agent lifecycle

## Status

Accepted for the Windows configuration candidate.

## Context

A Codex tab displayed `[ ! ] Action Required` beside a running indicator.
The title adapter retained the text but discarded its state meaning. Full Hook
coverage also excluded screen observations, so a missed attention event left the
last working state visible indefinitely.

## Evidence

Codex's `chatwidget/status_surfaces.rs` at upstream revision
`67f0994b532b8740f14b5b738af7b7b2a915b1d6` emits the reserved segments
`[ ! ] Action Required` and `[ . ] Action Required` for the same attention
state. The second segment is the alternate animation frame, not resumed work.
The title is emitted through OSC 0. The provided screenshot contains the first
segment and Pebrel's running indicator.

## Decision

`ai_agents::native_title` recognizes these complete segments only after the
foreground process has independently been identified as Codex. The shared
`AgentActivity` reducer owns the latest title observation. It can project
attention or working, with a distinct `title` source exposed by the Runtime API.
It cannot establish session identity, authorize a command, construct a provider
request, announce completion, or send a notification.

The observation retains the underlying lifecycle state. A new Hook or explicit
submission clears it; a removed title marker reveals the underlying state
without manufacturing a result. Shell and remote-context boundaries reset both.
Each remote pane sample is consumed once, and samples older than a Hook/submission are
rejected. Current native title evidence excludes screen reconciliation.
Explicit OSC 7501 program records retain their existing priority.

Tab labels remove the reserved attention segment and native Braille animation,
leaving the client's remaining task/title fields for display.

## Rejected alternatives

- Matching `permission` or `Action Required` anywhere in screen/task prose:
  ordinary text could become a false pending request.
- Treating the alternate `[ . ]` frame as working: the indicator would flicker
  between incompatible states while waiting for the same response.
- Sending a synthetic attention Hook: a title supplies no request identity,
  choices, or permission scope.
- Copying status rules into tab rendering: Runtime API and confirmation state
  would disagree with the visible indicator.

## Consequences

The mapping depends on the client's reserved title format and on title output
being enabled. It supplements existing Hooks and screen evidence. It does not
interpret arbitrary titles from other programs or infer success from silence.

## Validation

Reviewed the supplied screenshot against the client's title writer and both
animation frames. Format and architecture checks accompany the change. Product
compilation is included in the candidate build; no regression suite is run at
the user's request. Source review is not native UI interaction verification.

## Supersedes

None.

## Revisit when

The client changes its reserved segments or reliably supplies explicit program
state across all supported local and remote transport paths.
