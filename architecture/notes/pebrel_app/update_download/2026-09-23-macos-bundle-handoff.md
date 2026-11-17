# Native macOS application replacement

## Status

Proposed for review; implemented with native macOS arm64 acceptance.

## Context

macOS could discover releases but had no in-app installer adapter. The requested
update flow must preserve the running application until the downloaded replacement
is verified and all windows have authorized shutdown. API rate-limit recovery and
installation are delivered together because discovery alone left manual upgrades.

## Evidence

The official v1.9.0 ARM64 DMG was downloaded and mounted read-only on 2026-09-23.
Its legacy `-preview.dmg` asset name contains the formal `Pebrel.app`, identifier
`io.github.kuddev.pebrel`, bundle version 1.9.0 and executable version 1.9.0;
`codesign --verify --deep --strict` passed. Current packaging also emits the
preferred `macos-arm64.dmg` / `macos-x64.dmg` names without the legacy suffix.

## Decision

`update_check::assets` owns exact native package selection. `update_download`
continues to own streaming, size limits, SHA-256 validation and cache publication;
a DMG additionally needs a UDIF trailer. This uses existing dependencies.

Reuse the existing schema-1 prepare/commit/result/workspace transaction. The
macOS coordinator remains in `update_download::handoff::macos`; the platform
adapter owns plist, signature, process identity, mounting and atomic filesystem
operations. No UI module owns installation persistence or invokes shell commands.

Copy the current executable into its private transaction directory and enter a
non-GUI helper mode before normal startup. Validate every plan's configuration,
installation, package, version and participant identity. A lifetime lock beside
the app survives replacement and is shared across configuration overrides. Hash
the bundle filename for an extension-free lock base; the existing lock API remains
the authority for its final filename. PID plus native creation time avoids PID
reuse. Refuse updates while another process executes the same installed binary.

Mount the verified DMG read-only in a private transaction directory. Copy its
single top-level app beside the installed app, then validate bundle ID, plist and
executable versions, native architecture and strict code signature. A Developer
ID signed installation also requires the same signing team. Existing ad-hoc
packages remain supported. Unmount before reporting readiness.

Only commit authorizes installation. The application saves its workspace and
exits through the existing shutdown flow. The helper waits for that exact process
to stop, rechecks that the old executable has not changed and that no other copy
started, then atomically swaps same-volume bundle directories with `RENAME_SWAP`.
The complete old app remains at the unique hidden staging path as a backup.

Persist the result before launching the new app with the existing restore ticket.
If launch fails or exits unsuccessfully during the immediate two-second check,
reacquire the installation lock, ensure no copy is running, exchange back and
restart the original with a recovery ticket. Later crashes are not classified by
this bounded launch check; the retained backup and durable workspace remain.
Cancelled or rejected transactions keep the original application running. Helpers
have bounded preparation, commit and exit waits and never kill the user's process.

The platform adapter also owns helper materialization and launch selection. The
shared transaction requests a helper through one platform-independent entry,
without adding operating-system branches to prepare/commit orchestration.
Native command output is spooled to temporary files while enforcing the existing
process deadline, then read with a 2 MiB limit per stream. Polling a child before
reading piped output can otherwise block both processes once a pipe fills. A
native regression produces 256 KiB on each stream and checks command failure.

## Rejected alternatives

- Replacing a running bundle: risks mixed resources and incomplete session saves.
- Removing the original before copying: leaves no working app after interruption.
- Shell interpolation / privileged installer: unnecessary for a writable app
  directory; paths are separate arguments and no elevation is requested.
- Installing an unverified package when metadata is unavailable: retain the
  existing manual download path instead.
- A second session format: reuse the shared update restore/acknowledgement flow.

## Consequences

Only packaged official-identity apps in writable locations support installation.
Developer binaries, read-only disk-image launches and App Translocation return an
actionable preparation error while keeping the app open. Preparation is background
work; scheduled installation uses the existing pre-window startup path. The old
bundle is retained until manually removed; no backup-retention policy is added.
Windows retains its existing installer helper. Linux retains manual installation.
No release assets, version tags, signing settings or distribution channels change.

## Validation

Native tests create disposable ad-hoc-signed Mach-O apps and real HFS+ DMGs. They
exercise the actual copied helper in successful replacement/relaunch, immediate
launch failure and rollback, cancellation, SHA-256 rejection and a second running
instance. Tests also cover atomic swap rollback, process identity, developer-path
rejection, native asset selection, DMG trailer validation and shared restore ticket
success/recovery/acknowledgement/crash-loop limits. All passed on macOS arm64. A separate packaged debug build with the existing
loopback-only update-test-source feature also completed the native UI flow:
verified DMG download, Restart and install, replacement, reopened terminal window,
and persisted `success: true` plus `restored.json`. This rehearsal used isolated
settings and same-version reinstall, which normal builds do not permit.

The isolated i18n contract passed, including zero-allocation lookup. Architecture
validation against the actual PR base passed without budget changes. Intel macOS,
Windows and Linux execution, Developer ID team migration and notarization remain
outside local runtime acceptance and need their platform CI/release validation.

## Supersedes

None. Extends the existing shared update handoff to a native macOS participant.

## Revisit when

Distribution changes to a notarized update feed, privileged installation becomes
a supported requirement, or backup retention / longer startup health acknowledgement
is given an explicit product policy.
