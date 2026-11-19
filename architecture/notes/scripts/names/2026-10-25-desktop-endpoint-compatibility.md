# Desktop endpoint metadata compatibility

## Status

Proposed for maintainer review with desktop SSH recovery.

## Context

Remote session recovery identifies an executable and socket from process metadata.
The desktop adapter uses Rust and a POSIX probe, while the existing compatibility
rules recognize only the corresponding Kotlin forms.

## Evidence

`test_desktop_endpoint_protocol_references_do_not_allow_prose` records the rejected
executable default, process comparisons, path match and process fixture. These
fragments are protocol data, not product descriptions.

## Decision

Recognize only these exact interface fragments in the four owning source files.
Continue scanning the rest of each line. Other files, unknown commands and ordinary
prose remain rejected. No source file or directory is exempted.

## Rejected alternatives

Obfuscating executable names hides compatibility dependencies. Exempting strings
or files would also admit unrelated descriptions and promotional references.

## Consequences

Both transport adapters can maintain the same remote endpoint behavior. Changes
to this protocol syntax require new positive and negative examples.

## Validation

Regression cases include valid interface references, wrong files, ordinary labels,
unknown commands and unrelated same-line mentions. Existing tests stay unchanged.

## Supersedes

None; extends the remote endpoint compatibility contract to its desktop adapter.

## Revisit when

The endpoint discovery protocol or owning transport changes.
