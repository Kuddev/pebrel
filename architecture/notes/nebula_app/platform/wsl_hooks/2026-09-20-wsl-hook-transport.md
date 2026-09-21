# WSL command hooks use the guest home and authenticated pane transport

## Status

Proposed for review with implementation.

## Context

A Windows terminal can launch Codex inside WSL. Windows hook installation does
not configure the guest home, discover its CLI version, or deliver native Linux
hook events through a Windows named pipe. Completion-only installation and screen
observations therefore remained active even when Windows reported native hooks.

## Evidence

`ai_hook/win.rs` installs host configuration. `terminal/session.rs` launches WSL
as a local PTY. The shared lifecycle already gives full native hooks precedence
and has no authority to infer that a separate host's installation covers WSL.
Codex can render compaction and queued input above its ordinary prompt row.

## Decision

Reuse `ai_hook/remote.rs` file planning and its POSIX adapter for WSL command hooks.
A bounded worker targets the original distribution and user without replacing the
interactive shell. Each PTY receives a fresh random token through WSLENV; its
stream parser checks that token before forwarding the existing OSC hook envelope.
Linux process identity remains separate from host PIDs.

Guest CLI discovery first respects PATH, then probes bounded conventional user
installation locations. Discovery can select a fallback NVM version when shell
initialization is unavailable; it does not claim to execute arbitrary shell
configuration. Explicit `setup-ai --wsl` permits repair/removal through the same
planner. Provider opt-out, unrelated hooks and notify chains remain preserved.

Claude question tools and terminal errors enter the shared lifecycle. Failed,
incomplete and cancelled outcomes do not display successful completion. Screen
fallback recognizes Codex queue/compaction chrome only where lifecycle coverage
is absent; it cannot override a native running turn.

## Rejected alternatives

- Reuse Windows hook paths inside WSL: wrong executable and configuration domain.
- Infer completion from output silence: tools and compaction can be silent.
- Overwrite guest settings: loses custom hooks, trust and user opt-out.
- Start a poller per pane: unbounded process and thread cost.
- Install plugin adapters without their startup environment: incomplete delivery.

## Consequences

One worker owns guest setup with an eight-second budget, bounded queue and retry
cache. Installation is asynchronous; an already-running CLI does not gain hooks
retroactively. Codex may require review in `/hooks`, and a fresh PTY is necessary
for its channel token. A CLI launched before initial setup finishes may require
restart. Unsupported/missing Python or customized owned assets cause a logged
failure while preserving configuration and screen fallback.

## Validation

Planner tests cover guest paths, custom notify preservation and repeated setup.
Python tests exercise PATH/NVM discovery, actual PTY delivery and transactional
writes. Stream tests check token rejection and every frame split. Lifecycle tests
cover queued input, compaction, question/tool-error/end-error distinctions.
Windows WSL delivery and hook trust still require integration acceptance.

## Supersedes

None; extends the existing SSH installation and native Codex contracts.

## Revisit when

WSL supplies a provider-neutral guest integration service, native hooks gain a
portable host transport, or plugin adapters can use the same command transport.
