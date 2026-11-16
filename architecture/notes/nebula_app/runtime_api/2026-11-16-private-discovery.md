# Private runtime discovery and token handling

## Status

Implemented.

## Context

The local runtime discovery file holds a bearer token for the loopback API. Binding
to loopback limits network reachability but does not separate local user accounts.

## Evidence

The old discovery writer used the ordinary state-file mode; with a typical Unix
umask, that creates a record readable by other local accounts. The code mixed the
process id and clock with a keyed hasher to generate the token. Three transport
entry points used ordinary string comparisons to authenticate requests.

## Decision

Use the existing private atomic writer for the discovery record. Keep generation
for runtime, SSH, and WSL bridge tokens in one module, using operating system entropy.
Use the existing locked `subtle` package for comparisons of equal-length, nonempty
credentials. The endpoint does not start if a secure token or discovery record cannot
be created.

## Rejected alternatives

Changing permissions after writing leaves time when the file is readable. A manual
fold with an optimization hint provides no constant-time comparison contract.
Changing the token's wire format would break existing clients without fixing file
permissions.

## Consequences

The 32-character hexadecimal runtime and bridge formats stay the same. Unix creation
uses owner permissions; Windows continues to inherit its directory's ACL.

## Validation

The runtime publication test checks the discovery file permissions and token format.
The shared matcher tests exact, missing, empty, and incorrect credentials.

## Supersedes

None.

## Revisit when

Runtime discovery or bridge credentials move to a different transport or credential
store.
