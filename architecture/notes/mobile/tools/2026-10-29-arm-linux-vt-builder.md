# Build the pinned Android VT core on ARM Linux

## Status

Implemented; validation results accompany the development commit.

## Context

The mobile VT builder accepted Linux x86_64 and Windows x64 only. The development
host is Linux aarch64, so it rejected the host before compiling the pinned source.
Reusing an old APK's native library would not rebuild the current JNI adapter.

## Evidence

The existing `build_ghostty.py` explicitly rejected aarch64. Zig publishes a native
Linux aarch64 distribution for the already-pinned 0.15.2 compiler. Its official
download index provides the URL and SHA256 now recorded in `UPSTREAM.json`.
The source revision, source hash, Android API and target ABIs remain the same.

## Decision

Select an explicit pinned Zig archive by host OS and architecture. Linux aarch64
and arm64 aliases use the native ARM archive; existing x86_64/AMD64 selections
remain unchanged. Unsupported hosts fail before downloading the wrong executable.
All source/tool checksum and NDK version checks remain mandatory.

## Rejected alternatives

- Relabeling the host as x86_64 would select an incompatible executable.
- Unverified prebuilt libraries obscure whether the adapter and source match.
- Relaxing restore provenance checks would hide mismatched producer inputs.
- Changing the upstream VT revision or compiler version is unnecessary for this
  host-selection problem.

## Consequences

The VT dependency can compile natively on ARM Linux. This is not a claim that the
entire Android SDK/NDK ships ARM Linux tools: the NDK and other packaging utilities
may still require an x86_64 host or separately configured host emulation. The local
test package uses the official pinned NDK through host-only QEMU wrappers; those
tool caches are not repository content.

## Validation

`python3 -m unittest discover -s mobile/tools -p 'test_build_ghostty.py'` checks
supported host aliases and rejects unsupported OS/architecture pairs. Actual
`build_ghostty.py` builds must produce both ABI libraries and source/hash records.
APK JNI, license and 16 KiB alignment checks remain separate packaging gates.

## Supersedes

None.

## Revisit when

The pinned compiler or supported host set changes, or Android supplies a supported
native ARM Linux NDK distribution that removes the need for host emulation.
