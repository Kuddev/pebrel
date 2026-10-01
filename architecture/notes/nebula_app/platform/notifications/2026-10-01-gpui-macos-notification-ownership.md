# GPUI macOS notification ownership

## Status

Candidate under native acceptance.

## Context

A foreground OSC 9 notice must appear through the actual macOS notification channel, with authorization and click handling. Successful legacy dispatch does not establish visible delivery.

## Evidence

Native acceptance of `8193eef` reached the production dispatcher without errors. Public synthetic screenshots in fork run 36806536236 show the Notifications settings list scrolled to its end without a Pebrel entry; the authorization status remained not determined. The earlier denied status came from an invalid helper authorization request and is not product evidence.

The pinned mac-notification-sys setApplication installs a process-wide NSBundle hook. GPUI mode does not initialize that legacy registration; all native entrypoints use the existing GPUI service and real bundle identity. Legacy mode retains its existing backend.

The exact locked GPUI revision `fc05d637cc7029d75de051fd7f52c1a0fb8fa6b4` already implements macOS `UNUserNotificationCenter` authorization, Banner/List foreground presentation, retained delegate ownership, main-thread response callbacks, actions and dismissal in `gpui_macos::system_notifications`.

## Decision

Use that existing GPUI platform service for macOS GPUI notifications. A bounded application adapter forwards existing notification payloads to the foreground executor without a worker or blocking send, and routes native responses to existing activation/choice closures. The notification capability still owns policy, text and throttling.

The adapter retains at most 64 delivered callback sets. Eviction dismisses the associated delivered/pending system notification. Response handling removes a callback set and releases its borrow before activation. The foreground executor owns task lifetime; it does not run after the App is dropped. A bundle guard avoids invoking Apple's service for unbundled development and test binaries.

## Rejected alternatives

- Patching the deprecated legacy backend's delegate alone did not establish actual authorization or visible delivery on the tested macOS version.
- A new UserNotifications implementation would duplicate a platform service already provided by the locked GPUI dependency.
- Test helpers must not replace production dispatch, silently grant permissions or turn accepted dispatch into a visible-success assertion.

## Consequences

No additional dependency or lockfile change. GPUI controls Apple's delegate, native authorization and presentation. The application controls bounded payload/callback retention and existing activation events. macOS legacy, Linux and Windows delivery stay on their existing paths.

The first native notice may initiate Apple's permission flow. User refusal and system presentation preferences remain authoritative; no permission bypass is implemented.

## Validation

Native acceptance uses the registered real candidate App, actual foreground OSC 9 production routing, synthetic permission/settings interactions and Notification Center accessibility plus PNG evidence. Current candidate results must be checked before readiness. Architecture and formatting checks cover the adapter; existing pane delivery tests retain policy coverage.

## Supersedes

[macOS foreground notification presentation](../2026-09-30-macos-foreground-notification-policy.md).

## Revisit when

GPUI's notification API or delegate ownership changes, or product notification retention policy changes.
