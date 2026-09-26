# Acrylic during native window transitions

## Status

Implemented as a compatibility fallback on the GPUI Windows controller path.
Local native acceptance uses Windows 11 build 26200.9457 and Windows App Runtime
1.8 version 8000.946.1701.0; other compositor/driver combinations need acceptance.

## Context

Desktop Acrylic fixes the Accent edge artifact and preserves unfocused blur, but
its old blurred background can stretch or crop during maximize and restore to a
floating window. Ordinary window movement does not reproduce this incident.
The previous Accent path becomes temporarily clear during those animations.
Taskbar minimize/restore keeps native behavior: clearing before shrink introduces
a more noticeable flash than its existing, brief blurred snapshot.

## Evidence

- Colored horizontal bands and repeated text behind the same GPUI window expose
  the stale blur geometry. A flat background does not establish correctness.
- An Accent/controller comparison keeps the GPUI renderer, theme and opacity
  constant. The controller's old sample remains visible during the transition;
  Accent exposes the clear background and resumes blur afterward. No application
  code explicitly pauses Accent during these transitions.
- Native message traces put WINDOWPOSCHANGED and SIZE only a few milliseconds
  after the command, while the visible transition continues. Neither message is
  a completion notification for the compositor animation.
- The root already fills the desktop target. The public controller APIs provide
  no force-recompute or per-HWND animation-completion operation. This does not
  establish a documented internal DWM defect.
- [DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)
  documents TRANSITIONS_FORCEDISABLED for setting only. A local native probe
  confirms readback returns E_INVALIDARG, so it cannot identify animation state.

## Decision

- [transitions.rs](../../../../nebula_app/src/platform/acrylic/transitions.rs)
  belongs to the per-window Backdrop and installs a subclass on its owning UI
  thread. Pause only the system-backdrop brush before a meaningful system command
  or a maximize state change. The latter also covers ShowWindow calls. Minimize
  and return from the taskbar are tracked but never initiate a material pause.
  Hidden-window startup, ordinary movement and interactive geometry changes
  retain their material.
- Resume after a nonblocking 300 ms settling interval, measured to cover the local
  native animation. This is a bounded fallback, not an exact completion signal.
  Repeated transitions extend the deadline; a queued old timer cannot shorten it.
  If a window is minimized during a pending maximum-size transition, the timer
  still restores its saved brush without delaying or replaying the minimize.
- Preserve a newer SDK brush instead of overwriting a policy/accessibility update
  with the saved brush. Respect the system's SPI_GETANIMATION setting. Do not
  change animation settings, focus policy, GPUI content, opacity or startup flow.
- Remove the timer/subclass before closing the controller/target. The subclass
  holds its own Rc until removal or HWND destruction; callbacks also hold a local
  Rc for nested teardown. No RefCell borrow spans a COM call or DefSubclassProc.
- Timer allocation failure restores usable material and logs the failure; it
  must not leave a visible window permanently clear. Failed subclass removal
  retains an inert callback allocation until HWND destruction.
- There is no new worker or per-frame update. Additional native work occurs on
  state changes and their settling timer, not continuously during rendering.

## Rejected alternatives

- SYSTEMBACKDROP AUTO and a transparent Accent alongside the controller did not
  remove the stretched sample in the native comparison.
- Disabling GPUI DirectComposition still reproduced the sample artifact and
  removed visible GPUI content in the probe; it is not a production solution.
- Disabling Windows animations or adding edge masks changes the requested visual
  behavior. A rendering-backend replacement exceeds this focused repair.
- Restoring at WINDOWPOSCHANGED exposes stale material while DWM is still animating.
- Polling a write-only DWM attribute cannot detect disabled transitions.
- For taskbar minimization, RequestCommitAsync, DwmFlush and earlier CBT notification
  still retained the old snapshot in local probes. A 32 ms asynchronous preparation
  did clear it, but added a visible clear frame before shrinking. That tradeoff is
  rejected: there is no CBT hook, minimize veto/replay or synchronous DWM wait in
  production. Preserve the less distracting native minimize animation.

## Consequences

The fallback deliberately exposes a clear background during maximize/windowed
restore transitions, then
restores normal blur. It does not make Acrylic resample throughout the animation.
An unusually long system animation may outlast the measured settling interval.
The system animation setting is supported; per-HWND external suppression has no
supported readback and is not inferred. Runtime changes to accessibility policy
still require their own native visual acceptance.
If the composition target itself fails during restoration, the error is logged;
the guard does not retry COM calls indefinitely or overwrite an unknown SDK state.

## Validation

- Native tests exercise ordinary geometry, system commands, ShowWindowAsync,
  queued stale timers, successive transitions, unchanged taskbar material, a newer SDK
  brush, owner teardown and HWND destruction before the Rust owner.
- Colored-background recordings compare all four transition directions with the
  old Accent and unguarded controller. Static material/focus acceptance remains
  documented in the original controller note.
- Test processes have isolated configuration and are closed by their own HWND;
  existing user sessions are not used as test targets.

## Supersedes

None. Extends [controller ownership](2026-09-22-acrylic-controller.md) with the
animation compatibility boundary.

## Revisit when

The Windows controller exposes supported transition synchronization, GPUI shares
ownership of the backdrop target, or a tested direct solution preserves material
through the animation without changing the existing rendering behavior.
