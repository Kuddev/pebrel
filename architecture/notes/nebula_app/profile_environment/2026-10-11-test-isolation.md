# Owned profile regression fixture isolation

## Status

Implemented for the selected profile regression fixtures.

## Context

Profile model/store is a standalone API foundation. Its existing bare-WSL workspace
regression is retained but is run only in the launch layer's isolated child, not
in the pure model plan. The fixture program being missing cannot prevent Windows
WSL hook preparation: that worker independently uses `wsl.exe`.

## Evidence

The [fresh-child fixture](../../../../nebula_app/src/platform/profile_test_isolation.rs) asserts actual settings and SSH resolver ownership before construction. The [home resolver](../../../../nebula_app/src/platform/dirs.rs) and [SSH routes](../../../../nebula_app/src/ssh.rs) consume the same test override.

## Decision

The cfg(test) profile fixture starts the exact test in a fresh process and writes
owned `pebrel_settings.txt` before configuration OnceLocks or constructors. It
asserts all AgentHook preferences plus the legacy switch are false; automatic
updates, session restore/resume and environment refresh are disabled. Config/data
paths must resolve to the owned directory. It clears open-document, agent socket,
PTY-record, provider identity redirects and actual runtime/notify/helper/token
markers without redefining host HOME. Their names are removed without reading values.

## Rejected alternatives

A missing fixture executable does not prevent the independent WSL hook worker. Redefining host HOME does not establish the actual configuration authority. An owned SSH config file does not isolate native credential access.

## Consequences

`platform::dirs::ssh_config_home` is a narrow cfg(test) override consumed by both
SSH config path/probe and host discovery, preserving the production home resolver.
The child asserts the path, read and discovery routes all see its empty owned SSH
file. This is not an override for native stored SSH password/keychain access:
Auto-auth and mixed SSH fixtures remain excluded from the exact plan.

## Validation

Workspace/PTY/editor scenarios use synthetic argv, environment values and owned
files. Neither this isolation nor Mac synthetic WSL argv tests establish native
Windows WSL behavior. No native credentials are read or written.

## Supersedes

None.

## Revisit when

The selected constructors, configuration authority or SSH resolver routes change.
