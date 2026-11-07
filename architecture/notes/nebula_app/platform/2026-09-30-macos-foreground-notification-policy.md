# macOS foreground notification presentation

## Status

Implemented; native acceptance required.

## Context

Sending a foreground pane notice to the native backend does not itself permit macOS to display it while Pebrel is active.

## Evidence

Native acceptance on `1b9e697` reached system dispatch without registration, delivery or activation errors, but Notification Center did not expose a banner. The pinned `mac-notification-sys` 0.6.15 `NotificationCenterDelegate` implements delivery and activation callbacks but omits `userNotificationCenter:shouldPresentNotification:`. The default foreground presentation policy suppresses the notice.

## Decision

During the existing once-only bundle registration, add the foreground presentation callback to the backend's registered delegate class. It returns the target's Objective-C BOOL true. Delivery, activation, threading and delegate ownership remain in the existing backend; the pane capability controls when it is called.

## Rejected alternatives

- A second native dispatcher would duplicate delivery and activation ownership.
- Replacing the delegate would discard the backend's delivery and interaction callbacks.
- Suppressing focused pane notices would defeat this feature's foreground requirement.

## Consequences

Foreground notices can be presented using the existing system channel. macOS notification permissions and user preferences still apply. Linux and Windows behavior is unchanged. The callback ABI and encoding use the existing objc2 bindings for both macOS architectures.

## Validation

Existing delivery-channel regressions cover focused and unfocused notices and the in-app switch. Native acceptance exercises a real foreground OSC 9 notice, Notification Center and the production dispatcher. The final native results must be checked before readiness.

## Supersedes

None.

## Revisit when

The pinned notification backend changes its delegate class or adopts a native foreground callback.

Superseded by [GPUI macOS notification ownership](notifications/2026-10-01-gpui-macos-notification-ownership.md), pending native acceptance.
