# Cancel unsupported ZMODEM sessions

## Status

Accepted. This is a stop-gap for issue #806. It does not implement ZMODEM file transfer.
Native Windows ConPTY acceptance is still pending and is recorded on the PR.

## Context

When `rz` or `sz` runs on a remote host, the remote program sends a ZMODEM
session-start header and then waits for a binary ZMODEM peer. Pebrel had no
ZMODEM handling, so the header was rendered as text. The remote program kept
waiting, and the pane looked frozen until the user typed enough cancel bytes to
stop it. Local PTYs, built-in SSH and serial sessions all deliver remote output
through `event_loop::StreamProcessor::feed`, which already owns protocol replies
such as `Event::PtyWrite`.

## Evidence

The issue screenshot (Windows 11, PowerShell 7, `ssh` in a local pane) shows
`rz waiting to receive.**B0100000023be50`. That is the lrzsz ZRINIT hex header.
Its type is `01`, its flags are `0x23`, and its CRC-16/XMODEM is `be50`.
No ZDLE glyph is visible between `**` and `B`.
lrzsz `sz` starts with a ZRQINIT header (`**\x18B00000000000000`). lrzsz `canit`
aborts a session by sending ten CAN bytes followed by ten backspaces.

## Decision

`nebula_terminal::zmodem::ZmodemSniffer` watches the raw output stream. It is
an allocation-free state machine that persists across reads. It matches two or
more `*`, an optional ZDLE, `B`, and fourteen hex digits. The CRC must be valid
and the frame type must be ZRQINIT (`00`) or ZRINIT (`01`). The optional ZDLE
covers hosts or ConPTY rendering that drop the control byte.

Once the bytes have been parsed (so the header stays visible as context), the
processor writes `zmodem::ABORT_SEQUENCE` through `Event::PtyWrite`. It then
emits `Event::ZmodemCancelled`. The GPUI shell shows a localized warning toast
that points users to scp/SFTP. The retiring legacy shell already lacks an arm for
`Event::KittyImage` and its `event.rs` is at its line budget, so it gets no new
arm: it still writes the abort through its `PtyWrite` path but shows no toast.
Every new session start is cancelled again; there is no per-pane suppression state.

## Rejected alternatives

- Implementing a ZMODEM receiver/sender with a file picker. Windows local panes
  run through ConPTY without passthrough mode, and ConPTY is not byte-transparent
  for binary ZMODEM frames. It would therefore not fix the reported platform.
  It would also need a new protocol stack, a transfer thread, progress UI and
  per-transport byte paths. Each of those is a separate design decision.
- Detecting the header only in the GPUI view. Legacy, SSH and serial frontends
  would remain stuck, and protocol replies already belong to the core.
- Matching the text `rz waiting to receive`. This misses `sz`, localized or
  patched builds, and other ZMODEM senders, and it triggers on ordinary text.
- Requiring the ZDLE byte. This misses the observed rendering in the issue.

## Consequences

The pane becomes usable again, but users cannot transfer files over ZMODEM in
Pebrel. Wrappers such as `zssh`, which handle ZMODEM locally after the stream has
passed through Pebrel, are cancelled too. Output that happens to contain a
CRC-valid `**B00`/`**B01` hex header (for example, printing a captured ZMODEM
trace) also triggers a cancel. The required CRC and frame type make this rare.
There is no new dependency, thread, allocation or copy. Each read gets one extra
scan for `*`, done in branch-free 16-byte blocks. In a release build on macOS arm64
(32 MiB per corpus, 4 KiB reads), plain `ls`-style output costs 0.36% of
`StreamProcessor::feed` time. A synthetic corpus with several `*` per line, the
worst case, costs 4.65%. This is a single-machine measurement, not a general
guarantee.

## Validation

`nebula_terminal/src/zmodem.rs` unit tests cover every chunk split, byte-at-a-time
input, headers with and without ZDLE, bad CRC, truncation, wrong frame types and
lookalike text. `event_loop/zmodem_tests.rs` drives the real `StreamProcessor`
and checks that exactly one abort is written and the header stays on screen. A
GPUI test checks that the abort reaches the session input channel and the
view event is raised once. Native Windows ConPTY delivery of CAN/BS to `ssh.exe`
was not verified in this change.

## Supersedes

None.

## Revisit when

Revisit if Pebrel gains an 8-bit clean transfer path (ConPTY passthrough or the
built-in SSH channel) and a product decision to support ZMODEM uploads, or if
users need an opt-out for local ZMODEM wrappers.
