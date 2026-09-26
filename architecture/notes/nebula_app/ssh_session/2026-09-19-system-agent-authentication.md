# System SSH agent authentication and transport ownership

## Status

Implemented on 2026-09-19. This records the authentication repair and its limits;
it does not claim a release or acceptance with every credential-manager UI.

## Context

Auto had no system-agent step, so an identity held only by an agent failed both
saved connections and Test Connection. Legacy `agent` profiles decoded as Auto
but had lost that capability. Restoring platform APIs without compile-time guards
would repeat a cross-platform build defect. Treating a signer error as rejection
can leave the underlying SSH task waiting for a signature.

## Evidence

[The shared authentication plan](../../../../nebula_app/src/ssh_session.rs),
[native transport adapter](../../../../nebula_app/src/platform/ssh_agent.rs) and
[agent policy](../../../../nebula_app/src/ssh_session/agent.rs) define the current
boundaries. [Loopback and wire tests](../../../../nebula_app/src/ssh_session/agent/tests.rs)
exercise real SSH authentication with generated identities. The locked protocol
dependency supplies public-key, certificate and RSA signing support.

## Decision

One plan serves both entry points: explicit private keys, system agent, resolved
or default disk keys, saved password, keyboard-interactive, then a password prompt.
This keeps an explicitly selected key first while avoiding missing/default disk
keys consuming attempts before a usable agent. Deduplication spans both disk-key
groups. A draft password replaces the saved-password step rather than bypassing
the selected mode. Strict modes and the legacy profile migration retain their
existing semantics.

The platform adapter owns endpoint selection, connection and bounded identity
enumeration. The SSH layer owns per-host selectors, the total discovery budget,
signing outcomes and diagnostics. Windows uses the standard OpenSSH pipe and then
Pageant; Unix uses `SSH_AUTH_SOCK`. APIs are compiled only for their platform.

Each host, including a jump host, gets a fresh agent connection and its own identity
ranking. Public selectors rank matching identities first; comments do not affect
key equality. Certificate selectors match their underlying public key, but full
certificate blobs remain distinct from raw keys during duplicate suppression.
The agent helper never loads private material or persists identities.

Discovery and public-selector reads have short, bounded budgets. Signing uses the
longer interactive authentication budget because a credential manager may request
confirmation. Test Connection and unattended jump authentication retain their
shorter outer authentication budget. Current values are defined in the policy and
lifecycle modules; confirmation time does not consume later discovery allowance.

Unavailable, empty and server-rejected agents allow fallback. Partial success
continues to the second factor. A signer, session-channel or signing-timeout error
propagates immediately and drops the unpooled transport. Password retries, changing
agents or merely enqueueing Disconnect cannot repair a session awaiting Signed.

## Rejected alternatives

- Restoring unconditional Windows calls would break Unix builds again.
- Flattening explicit and resolved keys loses either the user's selected-key
  priority or agent priority over default disk paths.
- Converting every agent error into `false` would reuse a potentially stuck SSH
  authentication task after a signer error.
- A silent fixed identity cutoff can hide the required identity. Stable ranking,
  duplicate suppression and returned/offered diagnostics make attempts reviewable.
- Agent forwarding and provider-specific discovery are separate policies and are
  not implicitly introduced by restoring login authentication.

## Consequences

Auto can authenticate without a disk private key. The existing UI modes, stored
configuration, credential store and dependencies remain the same. Only completely
authenticated sessions enter the pool; an authenticated jump retains its own
lifetime when a target fails.

The locked Pageant 0.2.1 window-message transport calls synchronous `SendMessageA`
from an async worker. A caller deadline cannot preempt that call or reclaim a
worker blocked by an unresponsive legacy provider. The named-pipe busy case is
cancellable and separately tested. A stronger legacy-transport guarantee requires
an upstream or native adapter change; protocol timeout tests do not prove it.

## Validation

Generated keys, an isolated agent wire service and real loopback SSH servers cover
both entry points, strict modes, disk-key ordering, fallback, discovery timeouts,
signer refusal/disconnect, cancellation, transport closure, certificates, both RSA
hashes and independent jump/target selectors. Native fixtures use private pipe or
socket names. The Unix environment test runs in a child so parallel tests cannot
change another test's agent. No fixture enumerates a user's real credentials.
Native CI and live credential-manager UI acceptance are separate evidence.

## Supersedes

None. This is the initial module-scoped record for the restored authentication path.

## Revisit when

Custom IdentityAgent, IdentitiesOnly or forwarding needs an explicit policy.
Reconsider identity-attempt selection with server-limit evidence; do not silently
reintroduce truncation. Harden the legacy native transport before claiming that
every provider call can be forcibly cancelled.
