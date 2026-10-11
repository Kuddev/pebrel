# Cold profile reload preserves a known WSL guest location

## Status

Implemented; validation and review in progress.

## Context

The new local-profile cold resolver reloads a saved recipe and overlay. Replacing the complete profile can discard the snapshot's WSL own-directory option, even though the host cwd preservation branch already retains a valid local location. That regression must be closed before publishing the launch layer.

## Evidence

[#351](https://github.com/Kuddev/pebrel/pull/351) established the own-option parser and raw directory transformation in `shell_detect`. Guest options after `--exec`, `-e`, `--` or an unknown command belong to the guest command. `profile_launch::tests::cold_wsl_profile_reload_carries_directory_only_for_known_same_guest_and_user` specifies the new resolver contract using synthetic argv and an owned host directory.

## Decision

Extend the existing `WslOptions` authority with its raw directory value and opaque distribution-selector state. Carry that own directory into the newly loaded recipe only when both WSL launches explicitly name the same nonempty distribution and user and neither uses an opaque selector. Preserve the existing raw quoting by calling the existing directory transformation; do not decode and re-quote it or turn a guest path into a host `PathBuf`.

Keep a valid snapshot host cwd ahead of the local profile cwd. A host cwd, missing/unknown distribution or user, changed guest/user, system guest or distribution GUID prevents guest-location carry. The local recipe then owns its startup directory. This policy does not query WSL or infer identity from defaults that may have changed since the snapshot.

## Rejected alternatives

- A second option parser would drift from #351's guest-command boundary.
- Guessing the default guest or user risks forwarding a directory into another account.
- Copying old complete argv would discard newly loaded recipe parameters.
- A host path wrapper for guest cwd confuses the two path domains.

## Consequences

Cold reload keeps a known same-guest location while incorporating the local recipe's other arguments and overlay. Unknown identity uses the current local recipe. Warm launches still use their frozen launch snapshot. No session-schema marker or native guest probe is required.

## Validation

The synthetic regression covers quoted paths, changed distribution/user, missing selectors in either direction, opaque selectors, guest command options and the existing host-cwd contract. Native Windows/WSL acceptance remains separate; no Mac test is presented as native execution evidence.

## Supersedes

None; extends the local profile resolver while preserving #351's authority.

## Revisit when

Snapshots carry a verified typed guest identity that can safely resolve default users or distribution GUIDs.
