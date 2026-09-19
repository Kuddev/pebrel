# Relay QR invitations

## Status

Accepted for the Android connection preview.

## Context

The self-hosted relay already creates a bounded JSON invitation with a WSS endpoint,
separate mobile credential and display name. Requiring users to move that secret as
text is the highest-friction step in the existing computer connection flow. The
mobile app still needs a paste fallback for devices without a usable camera.

## Evidence

- `mobile/relay/init.mjs` is the authority that creates role-separated credentials.
- `RelayProfile.parse` rejects non-TLS endpoints, URL credentials, unexpected paths,
  oversized input and malformed device or token fields.
- The Android connection stores accepted profiles through the existing Keystore-
  protected relay store and does not place credentials in URLs or logs.

## Decision

Encode the existing invitation JSON unchanged into a permission-restricted SVG QR
file. Android scans it locally with the open-source ZXing stack and passes the raw
payload through the same `RelayProfile.parse` boundary as pasted text. Keep the
paste path visible. QR import adds no listener, cloud service, token format or
second pairing state machine.

## Rejected alternatives

- Google Play Services code scanning: smaller app code, but it would make pairing
  depend on Play Services availability.
- A new `pebrel://` invitation URL: it would duplicate an already validated payload
  and create another compatibility contract without a current routing need.
- Printing an ANSI QR to stdout: terminal logs and recordings could retain the
  mobile credential even though the existing initializer deliberately avoids it.

## Consequences

The preview APK gains a camera permission and Apache-licensed QR dependencies. The
relay kit gains an MIT-licensed QR encoder. Generated phone SVG and TXT files are
equally secret, use owner-only permissions where the platform supports them, and
must never be committed. Devices without a camera retain manual import.

## Validation

Relay tests verify role separation, non-disclosure in stdout, SVG creation, absence
of the literal token in SVG markup and owner-only output modes. Android CI compiles,
lints and packages the real scanner integration; physical camera behavior remains
a device acceptance item.

## Supersedes

None.

## Revisit when

Desktop-owned pairing or an end-to-end encrypted transport replaces the current
self-hosted relay invitation, or maintained Android platform APIs provide a local
scanner with equivalent non-Play-Services coverage.
