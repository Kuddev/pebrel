# Changed SSH host-key confirmation

## Status

Proposed for review.

## Context

Issue #252 requests an explicit update when a known SSH server changes identity.
The existing russh check rejects changed keys, without a confirmation path.

## Evidence

russh 0.62.2 checks literal/hashed host records and reports `KeyChanged` for an
incoming algorithm with a different recorded key. Its line index excludes
comments; marker records are not interpreted as ordinary host entries.
`ssh-key::known_hosts::Entry` supplies the existing semantic parser.

## Decision

Keep the handshake refused until the existing bounded prompt receives explicit
trust and the changed record is saved successfully. Show host/port, old SHA256
fingerprints, the new SHA256 fingerprint and the identity-change warning.

Use the library parser, preserve unrelated original lines, and split only the
literal target from multi-host records. Preserve hashed names, comments, line
endings and other algorithms. Marked matching records and changed wildcard or
negated patterns fail closed; Pebrel does not implement certificate trust or
silently convert revocation into ordinary trust.

Confirmation owns a file snapshot. The short application writer lock and two
byte comparisons reject concurrent edits observed before replacement. Stage a
regular sibling file, preserve file permissions, sync and use existing atomic
replacement. Reject symlinks rather than replacing a user's indirection.

## Rejected alternatives

- Automatically trusting a changed server bypasses identity verification.
- Deleting a reported line can remove other hosts and uses an unreliable index.
- Appending a literal record leaves the stale key authoritative.
- Reimplementing the complete OpenSSH parser adds unnecessary protocol policy.

## Consequences

Canceled, unattended and unavailable UI paths cannot update trust. Pattern and
marked records require external known_hosts administration. Invalid input fails
closed. Independent OpenSSH writers do not share Pebrel's lock: the comparison
is not a cross-tool atomic compare-and-swap. Native ACLs beyond Rust file
permissions are not claimed to be preserved.

## Validation

Active regressions cover isolated files, shared host lines, hashed names, ports,
other algorithms, comments/endings, markers, patterns and changed snapshots.
A real loopback SSH service rejects the old record, remains refused after
cancellation, and connects after explicit confirmation saves the new key.
The GPUI cancellation path and English/Chinese fingerprint formatting are covered.
All execution is delegated to GitHub Actions; local automated execution is prohibited.

## Supersedes

None.

## Revisit when

Safe wildcard replacement, certificate-authority policy, stronger cross-tool
coordination or platform ACL preservation is requested.
