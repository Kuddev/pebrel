# Inline pairing camera ownership

## Status
Implemented; physical-device QR decoding and computer approval require separate acceptance.

## Context
The connection page offers scanning, an eight-digit code and pasted invitations
in one place. Launching a separate full-screen scanner hides that context.
Embedding the preview transfers permission and camera lifetime ownership from
the scanner Activity to the pairing view.

## Evidence
- [ZXing 4.3.0 embedding guidance](https://github.com/journeyapps/zxing-android-embedded/blob/v4.3.0/EMBEDDING.md)
  assigns permission and camera ownership to the embedding caller. It describes
  preview overflow in dialogs with SurfaceView and the TextureView solution.
- [`InlinePairingScanner.kt`](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/ui/InlinePairingScanner.kt)
  owns the Android view and lifecycle observer.
- [`RelayForm.kt`](../../../../mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/ui/RelayForm.kt)
  composes the scanner only for the active scanning method and uses the existing
  invitation parser before entering the connection flow.

## Decision
Reuse the existing ZXing dependency with a TextureView-backed BarcodeView.
Only a resumed scanner with camera permission starts preview and single-result
decoding. Switching methods, leaving the page or pausing the Activity stops
decoding and releases the camera. Disposed or stopped views discard queued
results; the current callback handles an accepted result once.

Permission denial and camera failure remain local states with retry or app
settings actions. Code entry and paste do not depend on camera permission.
Scanner hints sit below the fixed preview so larger system text does not cover
the viewfinder. Pairing parsing, discovery, verification and approval retain
their existing owners and protocols.

## Rejected alternatives
- A separate capture Activity would retain the full-screen interruption.
- A new camera dependency would duplicate a capability already provided by
  ZXing without removing the caller's lifecycle responsibility.
- Keeping the preview alive under inactive methods would consume the camera
  after the user had moved to an input method that does not need it.
- Blocking `pauseAndWait()` on every transition would stall the UI thread.
  The existing camera worker serializes close/open operations; this view does
  not immediately hand the camera to an unrelated capture implementation.

## Consequences
Camera cleanup is now part of the pairing view's contract, including tab changes
and background transitions. No new persisted settings, dependencies or fallback
pairing protocols are introduced. TextureView's compositing cost is accepted
for a bounded preview that must respect the surrounding Compose controls.

## Validation
The existing `HomeSessionsTest` covers method exclusivity, a bounded scanner
region, invitation validation, back navigation and one accessible numeric
input at enlarged text size. Android API 28 emulator checks cover actual touch
switching and camera service ownership across method changes, backgrounding and dismissal.
These checks do not establish physical-camera optical decoding or a completed
connection to a real computer.

## Supersedes
None.

## Revisit when
The app replaces ZXing, adds concurrent camera consumers or changes pairing-page
navigation in a way that outlives its current Activity/view lifecycle.
