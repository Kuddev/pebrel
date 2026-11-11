# Ubuntu package sources for release runners

## Status

Implemented for stable Linux packaging, Android packaging, and the native test matrix.

## Context

These jobs need Ubuntu packages before they can build or test release binaries.
The runner's mirror list and resolver can fail independently of the source build.

## Evidence

[Release run 43055303415](https://github.com/Kuddev/pebrel/actions/runs/43055303415)
failed on both Android relay architectures before compilation. The x64 job
could not resolve the default regional archive; ARM64 could not resolve the ports
archive. Both jobs reached the package installer with stale package indexes.

## Decision

Use the official Ubuntu archive sources for the runner architecture through a
temporary APT source list. Fail if any index update fails. If the runner cannot
resolve an archive hostname, recover its public IPv4 address over HTTPS for
that job and restore the original hosts file after the installation. Keep HTTPS
hostname checks and the Ubuntu archive signature checks enabled.

## Rejected alternatives

- Retrying the runner mirror list can repeat the same DNS failure and still use
  incomplete indexes.
- Disabling signature or certificate checks would remove protections needed by
  a release build.

## Consequences

Only the CI dependency installation uses the temporary sources. Release assets,
test selection, and package contents still come from the normal release workflow.

## Validation

Verified the source list and failure mode for amd64 and arm64, exercised the
public DNS fallback with a simulated local failure, and checked the official
archive signatures. The release workflow exercises installation on hosted runners.

## Supersedes

None.

## Revisit when

The runner image provides consistently reachable official archive sources and
can fail immediately when an index update is incomplete.
