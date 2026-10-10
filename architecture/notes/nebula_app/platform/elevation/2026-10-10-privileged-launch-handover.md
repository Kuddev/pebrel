# Elevated launches share only an elevated window owner

## Status

Implemented for review. Exact-head native CI and interactive UAC acceptance are
recorded separately; a transport test is not full application acceptance.

## Context

Repeated administrator launches ignored windowing_behavior. Ordinary resident
handover excludes elevated processes and explicit -e commands, while the UAC
launcher intentionally passes the selected program as an explicit argument vector.
Removing either exclusion alone would lose privilege separation or launch identity.

## Evidence

- main.rs::try_hand_over_to_resident excludes both cases.
- platform/elevation.rs::arguments preserves the selected program and arguments.
- runtime_api/server.rs keeps privileged endpoint discovery off disk.
- Windows Terminal's
  [WindowEmperor](https://github.com/microsoft/terminal/blob/dc4ce1c096c07c23e1271f62b781a29b0746887d/src/cascadia/WindowsTerminal/WindowEmperor.cpp#L149-L197)
  elects one process with a named mutex and forwards startup through a hidden
  window and WM_COPYDATA. Its elevation/user namespace and common window-policy
  dispatcher keep process ownership separate from window selection.
- [WM_COPYDATA](https://learn.microsoft.com/en-us/windows/win32/dataxchg/wm-copydata)
  makes the received buffer valid only during message processing.
  [SendMessageTimeoutW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)
  documents bounded waits and the need to clear last-error before interpreting failure.
- The previous draft's single-instance pipe first returned ERROR_PIPE_BUSY on
  owner contention, then encountered stale read state on a reused connection.
  The pinned Mio 1.2.3 still has the post-EOF scheduling described in
  [Mio #1983](https://github.com/tokio-rs/mio/issues/1983). The user requested the
  Windows Terminal pattern rather than extending the pipe lifecycle.

## Decision

Use a Windows-only, startup-only hidden window and named mutex. Acquire ownership
before tray, hooks and Runtime startup. Keep the mutex handle until the receiver
window, callback state and message loop have all stopped. The namespace includes
the executable, settings directory, explicit configuration file, user SID and
Windows session. The mutex has a protected administrator/SYSTEM DACL and high
integrity label. An elevated-only public entry preserves the ordinary startup path.

The launching process obtains the target PID from the HWND and checks elevation,
user SID and session before granting foreground permission and sending. The
receiver retains Windows' default UIPI message filtering; it does not enable
lower-integrity WM_COPYDATA delivery. The sender PID in the message is used only
to detect launcher exit, not as authenticated sender identity. This is not the
previous pipe's mutual kernel-peer verification claim.

Messages contain owned launch data, a sender-lifetime reference and a bounded
deadline, never a Runtime bearer token. Validate size before decoding and copy
the payload into an owned request before it leaves the window callback. A system
tick deadline accounts for transmission time; the UI checks its remaining
monotonic deadline when claiming an event.

Retain execution acknowledgement instead of reporting success just for queuing.
The sender waits for the bounded reply without an earlier hung-window abort.
Launcher exit, owner shutdown or expiry cancels a still-pending event. Claimed
events run once. Do not retry or create a fallback process after sending, because
a missing acknowledgement does not prove that the operation was not executed.

One owned message-loop thread sleeps in GetMessageW when idle. It does not depend
on tray enablement or any particular terminal window. Shutdown cancels queued work
and wakes the thread without joining it on the UI thread. Thread-ID publication
and retirement share the stop lock, so shutdown cannot target a recycled ID.

The receiver reuses the existing window registry's new-window/current-desktop/MRU
policy, startup conversion, tab creation and focus. Exact program, arguments and
directory remain intact. The shared conversion and its existing tests stay together
under windowing/startup.rs, rather than being copied into a second implementation.

## Rejected alternatives

- Continuing to recycle Tokio pipe instances: reproduces the native stale-read failure.
- Adding rotating pipe instances: unnecessary transport lifecycle for this startup-only
  operation when the referenced Windows Terminal pattern already fits.
- Reusing runtime.port or an inherited endpoint: crosses ordinary/elevated ownership.
- Publishing a privileged TCP token in ordinary settings: exposes a control capability.
- Replacing the selected program with a default shell: loses explicit launch identity.
- Depending on the tray window: elevated windows intentionally do not become tray residents.
- Copying Terminal's complete application framework: XAML, persistence, notifications
  and UI redesign are unrelated to this correction.

## Consequences

Ordinary startup, explicit ordinary commands, legacy-shell startup, session isolation
and private Runtime discovery are unchanged. There is no new dependency, persisted
setting, public Runtime method or visible UI control. Older processes without the
new endpoint are not retrofitted in place.

## Validation

The existing regressions retain literal argv, bounds, invalid requests, cancellation,
duplicate delivery, identity policy, original tab preservation and window selection.
The native message test uses two actual child processes and checks cancellation and
window cleanup on shutdown, including when the test runner is not elevated.
The public bootstrap test separately rejects an ordinary process and checks real
owner reuse when elevated. UI fixtures retain the existing serialization group.
Interactive UAC and actual virtual-desktop switching remain distinct acceptance steps.

## Supersedes

The independent-process behavior for subsequent elevated GPUI launches in ADR-0010.
Its ordinary-session and privileged-Runtime isolation contracts remain in force.

## Revisit when

The supported Windows message model or the required process/session isolation
contract changes. Re-evaluate the transport without adding a second window policy.