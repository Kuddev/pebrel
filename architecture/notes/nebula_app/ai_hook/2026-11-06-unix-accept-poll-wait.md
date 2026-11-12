# Unix AI hook accept wait uses `poll`

## Status

Implemented and validated on Linux. Windows named-pipe transport is unchanged.

## Context

The Unix AI hook worker accepts connections from a nonblocking `UnixListener`
and, before this decision, slept 20 ms after every `WouldBlock`. The helper
connects once per AI turn and waits for the acknowledgement byte before exiting,
so the sleep added an expected ~10 ms (observed ~20 ms) to every turn's ack even
when the worker had been idle for a long time.

The same wait bounds how often `stopping` is observed while the listener is
idle, and therefore how long `Drop` can spend joining an idle worker. A
connected peer that sends nothing already keeps `read_message` waiting for up to
one second, before and after this change, so that read is the bound for a
connected worker. A replacement must keep the idle stop granularity, must not
introduce a blocking `accept`, and must not need a synthetic wake-up connection
during shutdown.

## Evidence

- Before (`sleep(20 ms)`), three 100-round runs: ack p50 20.196 / 20.221 /
  20.185 ms; every one of the 300 samples was at least 1 ms, and p95 was about
  42 ms.
- After (`poll`), three 100-round runs: ack p50 0.063 / 0.067 / 0.050 ms. The
  only sample at or above 1 ms in each run was round 0 (~22 ms), the cold
  `nearest_agent_ancestor` process snapshot that both builds pay.
- Harness: the `#[ignore]` `manual_ack_latency_samples` test times
  connect -> envelope write -> one ack byte read -> channel drain. Numbers are
  single-machine, debug-profile point estimates (Linux 7.0.0-31, i5-12400,
  rustc 1.97.1), not a general performance guarantee.
- Code: `nebula_app/src/ai_hook/unix.rs` `WouldBlock` branch and
  `wait_until_readable`. Behaviour tests:
  `connections_are_accepted_in_order_and_each_receives_its_ack` and
  `drop_stops_the_idle_worker_and_removes_its_private_directory`.
## Decision

In the `WouldBlock` branch, wait for `POLLIN` on the listener descriptor with
`libc::poll` for the same 20 ms budget, then return to the top of the loop and
let the existing nonblocking `accept` classify the readiness. `accept` stays
nonblocking; `stopping` is still checked once per loop iteration, so for an idle
listener the stop granularity and `Drop` join bound remain one 20 ms timeout. A
connected peer that sends nothing can still hold the worker in `read_message`
for up to one second, as before; this decision does not change that read.

The deadline is tracked with `Instant`. `EINTR` retries the poll against the
same deadline, so repeated signals cannot extend the stop granularity. Any other
poll error is logged and stops the worker, the same fail-closed direction as an
accept error. `POLLERR`, `POLLHUP`, and `POLLNVAL` are deliberately handed to the
following `accept` so existing accept error behaviour is unchanged.

## Rejected alternatives

- **Blocking `accept` plus a wake-up connection on shutdown.** This changes the
  shutdown ownership and adds a synthetic connection to a path that verifies
  peer identity, for no latency benefit over `poll`.
- **A shorter sleep (for example 1 ms).** It cuts the average but multiplies
  idle wake-ups by 20 and still leaves a fixed alignment delay; `poll` blocks
  until readiness without spinning.
- **A stop channel or condvar in addition to the socket.** It requires new
  synchronization and does not make accept itself more responsive.
- **`SO_RCVTIMEO` on the listener.** Its effect on `accept` is not portable
  across the supported Unix targets and it does not express readiness as
  directly as `poll`.

## Consequences

- Ack latency no longer pays the sleep alignment; p50 is dominated by accept,
  peer verification, read, parse, ancestry lookup, enqueue, and ack write.
- Idle wake-up rate, idle stop-check granularity, and the `Drop` join bound for
  an idle listener are unchanged: one bounded 20 ms wait per idle iteration. A
  connected-but-silent peer can still hold `Drop`'s join for up to about one
  second through the existing `read_message` deadline, exactly as before.
- Poll failures other than `EINTR` stop the worker with a log line and no retry
  loop, matching the fail-closed accept path.
- The cold first-event process snapshot (~22 ms) remains; it is unrelated to the
  accept wait and is not addressed here.
- No new dependency: `libc` was already used by the transport.

## Validation

- The two behaviour tests above cover ordered accepts, per-connection acks, a
  forced wait-timeout iteration (40 ms gap between connections), and shutdown
  while idle (worker exits, private directory removed).
- At the change commit, `cargo test -p nebula --features gpui-shell ai_hook`
  passed with 133 passed and 2 ignored tests.
- The full `nebula_terminal` and product builds are unaffected; the Windows
  transport was not edited.
- No timing assertion runs in CI; the latency probe is `#[ignore]`.

## Supersedes

None. Transport and ownership rules remain in
[native agent transport](2026-09-29-native-agent-transport.md); this note records
only the accept-wait mechanism and its rejected alternatives.

## Revisit when

- Shutdown must become faster than the 20 ms idle stop granularity, or idle
  wake-ups must be eliminated entirely.
- A portable cross-Unix readiness API replaces `libc::poll`.
- The cold first-event snapshot becomes the dominant ack latency.
