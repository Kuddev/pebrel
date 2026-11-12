# Select the console host per Windows shell

## Status

Implemented as a compatibility rule for the configuration reported in #958.

## Context

Pebrel normally starts the bundled OpenConsole because it handles viewport
resizes better than older system ConPTY hosts. Windows PowerShell 5.1 with
PSReadLine 2.0.0 can leave colored cells after backspacing on Windows 10 22H2.
The terminal must retain the explicit background colors that applications draw.

## Evidence

The [Windows 10 report](https://github.com/Kuddev/pebrel/issues/958) reproduced
with `-NoLogo -NoProfile`. The same profile and shell cleared the cells
normally when the reporter [selected system ConPTY](https://github.com/Kuddev/pebrel/issues/958#issuecomment-6815114024).
On [Windows Server 2022](https://github.com/Kuddev/pebrel/actions/runs/43151886039),
both hosts cleared the cells with the expected background in short and
wrapped input tests. That capture did not identify the incorrect Windows 10
byte sequence.

## Decision

On build 19045, use system ConPTY by default for a directly launched
`powershell.exe`. Match the executable selected for launch, including the
default shell. Keep the bundled host as the default for other shells and builds.
Explicit `openconsole` on/off values take precedence.

The terminal I/O loop derives DA1 suppression from the live PTY. A settings
preference cannot establish that the bundled host was loaded and its first
query answered, especially when the runtime files are missing.

## Rejected alternatives

- Replace ANSI black with the default background during erasure. The terminal
  cannot distinguish an intentionally colored blank from this console defect.
- Disable the bundled host for all Windows 10 applications. The evidence
  concerns Windows PowerShell, and other programs benefit from the bundled host.

## Consequences

New Windows PowerShell sessions on the affected build use the same backend
as `openconsole=off` without editing settings. If system ConPTY rejects
Win32 input mode, the existing flag negotiation uses its VT input path.
An already running console retains its original host.

## Validation

The selector tests cover the affected build, explicit overrides, and the
unchanged defaults for PowerShell 7, CMD, WSL, Bash, Server 2022 and Windows 11.
For Windows 10 22H2 acceptance, launch Windows PowerShell in a fresh tab with
automatic host selection, type and erase short and wrapped input, and verify
both the selected host and the visible cell backgrounds.

## Supersedes

None.

## Revisit when

A Win10 22H2 capture identifies the offending operation or a verified
OpenConsole build fixes the erase behavior in that configuration.
