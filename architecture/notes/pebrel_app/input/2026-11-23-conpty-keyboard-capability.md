# Keyboard capability follows the session transport

## Status

Implemented locally; pending maintainer review.

## Context

Issue #1080 reports that an application pushing Kitty keyboard disambiguation
loses Escape and Ctrl+C in a local Windows pane while arrow keys still work.
Both desktop adapters previously enabled that protocol for every terminal.
ConPTY translates the resulting CSI-u input instead of delivering those bytes intact.

## Evidence

The native regression starts Windows PowerShell through the production PTY,
feeds its output through the terminal parser, and sends keys through the GPUI
encoder. Without an enhanced-keyboard request, ReadKey reports Escape 27,
C 3 with Control, and UpArrow. On the unchanged implementation, after
CSI > 1 u, only UpArrow arrives. This reproduces on Windows 11 build 26300
with the in-box ConPTY.

The SSH regression uses a loopback SSH server and the real channel pump. A
local configuration with the enhanced protocol disabled previously omitted
its keyboard query reply when reused by SSH, establishing the need for a
separate transport capability decision at that boundary.

## Decision

Both desktop adapters read `CAPABILITIES.local_kitty_keyboard`, whose
Windows value is false because ConPTY cannot preserve the enhanced input.
The terminal's existing configuration gates decline the query and ignore
push/set/pop requests, so input stays on the negotiated Win32 or legacy path.
Non-Windows local sessions retain the enhanced protocol. The direct SSH adapter
explicitly enables it because it has no intervening ConPTY input decoder.

## Rejected alternatives

- Change enhanced-keyboard/Win32 encoder precedence: this breaks correctly
  negotiated byte-stream sessions and leaves the terminal advertising an
  unsupported protocol to local applications.
- Special-case Escape and Ctrl+C: other enhanced keys still cross the same
  unsupported transport, and protocol state no longer describes sent input.
- Disable the enhanced protocol for all sessions on a Windows desktop:
  direct SSH does not pass through the local console and should retain it.

## Consequences

Windows programs using a local ConPTY, including local SSH/WSL commands,
must use the supported Win32 or legacy protocol. Applications that query
the enhanced keyboard capability can fall back. An application that requires
it unconditionally is not made compatible by this change. No persistent
setting or second protocol state machine is introduced; the existing encoder
precedence remains intact.

## Validation

Run the transport-negotiation test for both desktop configurations, the native
ReadKey regression with and without an enhanced-keyboard request, existing
keymap protocol tests, and the loopback SSH capability/query tests. Also build
the actual GPUI product and check the legacy adapter. The reporter's
Antigravity version, bundled OpenConsole, and non-Windows hosts require
separate native acceptance.

## Supersedes

None. Input is still encoded from the live protocol state as described in
[deferred submit](2026-11-08-submit-keyboard-negotiation.md).

## Revisit when

ConPTY offers verified enhanced-keyboard input support with a detectable
capability, or the local Windows transport can deliver the negotiated bytes intact.
