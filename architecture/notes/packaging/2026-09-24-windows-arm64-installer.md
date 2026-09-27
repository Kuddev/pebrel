# Windows ARM64 installer

## Status

Reviewed for integration on 2026-09-25.

## Context

Pebrel already builds and validates a native Windows ARM64 application and portable ZIP,
but automatic installation still has no ARM64 installer asset.

## Evidence

- The native ARM64 application, Hook and console runtime already build and pass ARM64 conformance.
- The existing Windows update handoff is architecture-independent once the selected installer and installed payload are native.
- [Inno Setup documents `ArchitecturesAllowed=arm64`](https://jrsoftware.org/ishelp/topic_setup_architecturesallowed.htm)
  for installers shipping ARM64 binaries.
- [The published v1.9.1 release](https://github.com/Kuddev/pebrel/releases/tag/v1.9.1)
  contains an ARM64 ZIP, not an ARM64 installer (verified on 2026-09-25).

## Decision

Parameterize the existing Inno installer by architecture instead of creating a second
installer path. The ARM64 release job reuses its already-built native payload, validates
that payload as ARM64, builds `windows-arm64-setup.exe`, and publishes it beside the ZIP.
Release discovery selects that exact asset on ARM64; Windows x64 behavior is unchanged.

Pebrel 1.9.0 and 1.9.1 remain historical ARM64 portable-only releases. Stable manifests
from 1.9.2 require the ARM64 installer; published notes, tags and assets stay unchanged.

## Rejected alternatives

A separate ARM64 installer script and a second updater transaction were rejected because
the existing installer and handoff contracts are architecture-independent once the payload
and release asset are native.

## Consequences

Windows ARM64 gains the same installer-managed update path as x64 without introducing a second
transaction or migration implementation. Stable releases after 1.9.1 gain one additional asset.

## Validation

- Stable-release and branding contracts: 26 tests passed, including the historical
  1.9.1 manifest and rejection of a missing ARM64 installer from 1.9.2 onward.
- Existing PE payload tests passed for x64 and ARM64, including mismatched helpers
  and truncated executables.
- [Native ARM64 installer generation](https://github.com/WilliamWang1721/pebrel/actions/runs/35831885007)
  passed using the published 1.9.0 ARM64 payload as a packaging fixture. The builder
  and payload-architecture validator match this candidate; later version defaults
  and upstream WSL menu messages are separate from that generation evidence.

## Supersedes

None.

## Revisit when

Revisit only if Windows ARM64 requires installer behavior that cannot share the existing x64
migration and update handoff.
