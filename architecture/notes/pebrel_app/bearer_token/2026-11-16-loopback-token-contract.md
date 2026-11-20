# Bearer tokens for local control services

## Status

Implemented while integrating PR #989 into the renamed workspace.

## Context

The Runtime discovery record contains a credential for the local control plane.
Runtime requests and prompt-suggestion HTTP requests previously compared that
credential with ordinary string equality. Runtime token generation also used
hash-map seed material instead of the application's existing OS-entropy helper.

## Evidence

`runtime_api/server.rs` published discovery through `atomic_file::write`.
`runtime_api/transport.rs` and `prompt_suggestions/http.rs` each performed their
own equality check. `ai_hook::remote::new_token` already produces 128-bit tokens
from OS entropy, and `atomic_file::write_private` already owns private file
publication. The lockfile already contains `subtle` 2.6.1 through cryptographic
dependencies.

## Decision

Reuse the entropy and private-publication helpers. If entropy acquisition or
discovery publication fails, do not start the Runtime control service.

Keep comparison in `bearer_token::matches`, shared by the two local services.
Use `subtle::ConstantTimeEq` for equal-length byte strings. Missing credentials,
an empty expected token and a length mismatch fail before comparison. Token
length is public. The application adds a direct dependency on the existing
locked `subtle` package; no package version or source changes.

## Rejected alternatives

- A hand-written XOR loop with `black_box`: this is not a cryptographic
  constant-time contract.
- Separate comparison helpers per transport: they would duplicate credential
  acceptance rules.
- Fall back to a predictable token when entropy is unavailable: this would
  expose a running control service with weakened authentication.

## Consequences

Discovery files use owner-only permissions on Unix. Windows publication retains
the directory's inherited ACL; this change does not add a Windows ACL policy.
The wire format, token length and request framing remain compatible.

## Validation

The existing PR regressions check missing, empty, mismatched and exact tokens,
the discovery file's Unix permissions, and the generated hexadecimal token.
Integration verifies that the lockfile changes only the application's direct
dependency list. Native CI validates the integrated source before merge.

## Supersedes

None.

## Revisit when

The control services adopt a different credential format or authentication
transport, or discovery publication gains a separately reviewed Windows ACL
contract.
