# Pi paging when application mouse reporting is unavailable

## Status

Proposed for review with issue #781.

## Context

The reporter can page through full-screen Pi with PageUp/PageDown over SSH,
but cannot scroll with the wheel. A full-screen terminal has no local history
for Pebrel's scrollbar to control. Its application owns the transcript.

## Evidence

The shared wheel handler sends cursor arrows when alternate scrolling is
enabled without mouse reporting. Pi's full-screen transcript uses PageUp and
PageDown; arrows belong to its input editor. See Pi's
[key bindings](https://github.com/badlogic/pi-mono/blob/8ffea8a1bfdc32523ab056549b26141018c1e700/packages/tui/src/keybindings.ts)
and [viewport input](https://github.com/badlogic/pi-mono/blob/8ffea8a1bfdc32523ab056549b26141018c1e700/packages/tui/src/tui-alt-screen.ts).

This identifies an ineffective fallback in Pebrel. No terminal-mode trace was
provided for the reported macOS-to-Windows connection, so it does not establish
why mouse reporting was unavailable there.

## Decision

The GPUI wheel adapter pages a current Pi workload when the alternate screen
and alternate scrolling are active and no mouse reporting mode is active.
Only unmodified wheel events qualify. Send one page per event after the existing
fractional-delta accumulator produces a whole line; sending one page per line
would turn an ordinary three-line wheel event into three pages.

Use the shared key encoder so negotiated keyboard modes retain their semantics.
Use current program identity, including explicit program status, rather than
the saved conversation identity. Existing application mouse reports take priority.
Reuse the current-program classifier used by Send-to-Chat without constructing
the full Runtime Agent response in the wheel path.

## Rejected alternatives

- Replacing mouse reports with paging would discard Pi's negotiated mouse input.
- Paging every full-screen application would change editors and pagers that use
  cursor arrows for scrolling.
- Driving a local scrollbar cannot expose an application's internal history.
- Using saved session identity could page another foreground program after Pi exits.

## Consequences

Fallback scrolling is page-granular. Normal mouse reporting keeps its existing
line-level behavior. Explicitly disabling alternate scrolling also disables
this fallback. There are no new settings, persisted formats, or UI messages.

## Validation

The pointer tests cover real terminal-element wheel dispatch, both directions,
one-page batching, fractional accumulation, Kitty press/release encoding, SGR
precedence, stale identity, and alternate-screen exit. Pure routing tests cover
mode and modifier exclusions. The dispatch test observes one transport message
for a three-line event; work does not scale into three page commands. Identity
normalization and key encoding may allocate; no zero-allocation or latency
guarantee is claimed. Native macOS-to-Windows SSH and live Pi execution still
require platform acceptance.

## Supersedes

None.

## Revisit when

Pi changes its paging keys, the input protocol exposes a negotiated scrolling
capability, or a trace of #781 identifies a different failing mouse-report path.
