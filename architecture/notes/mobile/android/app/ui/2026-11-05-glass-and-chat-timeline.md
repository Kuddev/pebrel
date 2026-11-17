# Glass surfaces and the native conversation timeline

## Status

Accepted by the maintainer on 2026-11-05 as a preview direction. Android
acceptance is limited to an x86_64 emulator with a scripted desktop.

## Context

The conversation view rendered CommonMark through the reader WebView and floating
controls had no depth. The maintainer asked for frosted glass, a web-chat layout
with the user on the right, and no loss of scroll performance.

## Evidence

On the emulator, scroll frame time with blur on stayed about 2 ms above the
baseline build; with blur off the fallback kept text behind the input bar sharp.
Emulator numbers are only comparable with each other. `GlassTokensTest` checks
every registered theme on both the blurred and the solid path.

## Decision

Use Haze for backdrop blur and derive tint, hairline and shadow from each theme, so
no install theme is added. Battery saver, disabled animators, disabled window blur,
low-RAM devices and old API levels fall back to a translucent surface. Parse
CommonMark into Compose blocks; messages keep their transcript ids, so unchanged
rows reuse their parsed documents.

Sizes and alphas are our own: type sizes follow the Material scale, motion uses the
Compose spring constants, and tint alphas start from the contrast check. A survey of
another product informed the direction, but its measured constants are not reused,
because that product publishes no license for them.

## Rejected alternatives

- Keep the WebView timeline: selection and scroll anchoring stay out of reach.
- Reuse the surveyed constants as they were: this invites a copying claim for no
  functional gain.
- Add dedicated glass themes: the Rust table stays the only palette authority.

## Consequences

Haze becomes a runtime dependency recorded in the third-party notices. Blur cost has
not been measured on physical devices; do that before calling the performance
settled.

## Validation

`GlassTokensTest`, `ConversationTimelineTest` and the debug unit test suite pass; the
preview build succeeds.

## Supersedes

None.

## Revisit when

Physical-device measurements exceed the frame budget, or Haze drops an API level
this app supports.
