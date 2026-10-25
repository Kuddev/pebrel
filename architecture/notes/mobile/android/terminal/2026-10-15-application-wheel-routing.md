# Mobile history and application wheel ownership

## Status

Proposed for 2.2.1; Android unit tests and both JNI ABI builds passed. Device
and direct SSH application acceptance remain separate validation.

## Context

Phone history must remain independent of the desktop viewport. Fullscreen
applications own their visible content and may instead require wheel input.
The direct SSH terminal only moved its local viewport, while desktop mirror
reflow suppressed wheel forwarding. Primary-screen mouse reporting also
prevented independent history paging.

## Evidence

- [History capture](../../../../../nebula_app/src/runtime_api/terminal_read.rs)
  can read retained rows without mutating the terminal display offset.
- [Android snapshot gestures](../../../../../mobile/android) in `TerminalSnapshotView.kt`
  retain a source-cell projection for phone reflow.
- [Direct session input](../../../../../mobile/android) in `TerminalSession.kt`
  already serializes bounded encoded input with ordinary text and keys.

## Decision

Primary-screen gestures read phone-local history even if mouse reporting is
enabled. Alternate-screen gestures first move content already on the phone,
then deliver wheel events to the program, or negotiated arrow keys when
alternate scrolling is enabled. Program names do not participate.

Direct SSH uses the pinned native mouse/key encoders through the existing
ordered input queue. Mirror gestures map phone-reflow coordinates back to
the original desktop cells. The mobile bridge sets `application_only` on
scroll requests; dispatch rechecks the current mode so a queued fullscreen
gesture cannot scroll PC history after the application exits.

## Rejected alternatives

- Agent-name whitelists miss SSH nesting and other TUI programs.
- Treating primary-screen mouse tracking as application ownership blocks
  independent history reading.
- Sending arrows unconditionally changes shell commands and ignores mode
  negotiation.
- Forwarding ordinary history scrolling moves the desktop viewport.

## Consequences

Normal history remains independent. Wheel input can change the shared
fullscreen program state seen on both devices. The standard Runtime scroll
command keeps its existing default behavior; the mobile bridge imposes the
narrower application-only ownership rule. Reflow is a phone projection and
never changes the program's terminal dimensions.

## Validation

Existing Android snapshot tests cover reflowed application scrolling and
read-only history paging. JNI instrumentation fixtures cover primary history,
SGR mouse encoding, application cursor mode and disabled alternate scrolling.
The JNI and real SSH fixtures compile but have not run on a device in this
validation. Desktop tests cover a queued wheel arriving after alternate-screen exit.
These fixtures do not establish compatibility with every AI/TUI version.

## Supersedes

The mouse-mode classification in the 2026-10-07 independent-history decision;
its read-only paging and viewport ownership remain in effect.

## Revisit when

A program supplies a distinct, explicit protocol for independent application
history, or an observed mode sequence requires a documented routing change.
