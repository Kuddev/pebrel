# Profile environment model and store foundation

## Status

Implemented in the model/store layer; validation and review in progress.

## Context

Independent local sessions need validated variable maps and stable saved identities before launch adapters or an editor can consume them. Publishing an editor before adapters preserve the overlay would expose a control that cannot affect its child process.

## Evidence

`TerminalProfile` and `TerminalProfiles` provide the local versioned store. `ProfileEnvironment` validates maps, preserves explicit empty values, and omits values from diagnostic formatting. Existing v1 records omit the optional overlay and remain readable.

## Decision

Introduce the standalone environment value and store API first. Save profile edits by stable identity under the existing atomic-file lock, preserve hidden unavailable commands, and reject stale edits. Keep the model/store layer free of new environment controls and application launch behavior. The next launch layer maps the stored overlay into application profiles and child PTY options; the editor depends on that layer.

## Rejected alternatives

- UI controls before child launch consumption would misleadingly accept ineffective settings.
- Executable-path ids merge different sessions using the same shell.
- Process-global variable mutation leaks one session's settings into another.

## Consequences

Model serialization and CAS can be tested independently from GPUI and PTYs. A model/store-only build does not claim to apply new overlays to launched sessions; the complete feature requires the launch and editor layers.

## Validation

Synthetic model, compatibility, validation and CAS regressions accompany this foundation. Build and execution results are recorded by the serial verifier, rather than inferred from source inspection.

## Supersedes

None.

## Revisit when

Profile persistence changes version or independent session settings gain a secure transfer contract.
