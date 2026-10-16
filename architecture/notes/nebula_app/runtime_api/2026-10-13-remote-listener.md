# Opt-in remote Runtime API listener

## Status

Implemented; CI validation pending.

## Context

The Runtime API only listened on an ephemeral `127.0.0.1` port with a token that
changes every launch and is published in `runtime.port`. Other applications on
the same machine could read that file, but a LAN host could neither connect nor
keep a credential across restarts.

## Evidence

- `RuntimeServer::spawn_at` binds `Ipv4Addr::LOCALHOST:0` and writes the token.
- `transport::serve` already takes a listener and token, and every method is
  dispatched through the same `EventSink` and `RuntimeHub`.

## Decision

Add `runtime_api_exposure = off | local | lan` (default `off`). After the
resident instance owns the private endpoint, `runtime_api::remote` starts a
second `serve` on fixed port 47600 (`127.0.0.1` or `0.0.0.0`) with a token kept
in `runtime-remote.token`. The setting is read at startup only. Isolated
(elevated) instances never start it.

## Rejected alternatives

- Rebinding the existing listener: it would change the discovery contract that
  CLI children and `runtime.port` readers depend on.
- Live apply: needs a cancellable accept loop and listener ownership for a rarely
  changed advanced switch; restart is explicit in the UI text.
- TLS or pairing: out of scope for this switch. Plain TCP is called out in the
  setting description and docs.

## Consequences

- Anyone holding the token on a reachable network controls every terminal.
- Only the instance that owns `runtime.port` opens the port, so a second
  Pebrel process never fights over 47600.
- Changing the port is not supported yet.

## Validation

Unit tests cover the setting round trip and token persistence
(`nebula_settings/src/runtime_api.rs`, `nebula_app/src/runtime_api/remote.rs`);
the remaining checks run in GitHub Actions.

## Supersedes

None.

## Revisit when

A configurable port, live apply, or transport encryption is needed.
