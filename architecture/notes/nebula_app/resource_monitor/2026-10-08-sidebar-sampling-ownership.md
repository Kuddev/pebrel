# Resource sidebar sampling ownership

## Status

Implemented; build, test execution and runtime acceptance remain unverified.

## Context

The terminal workspace can contain native, WSL and SSH panes at the same time.
Resource measurements need an explicit host identity and must not block rendering,
write commands into terminal input, or continue polling after the sidebar closes.
Optional GPU and Docker tools can fail independently of CPU and memory collection.

## Evidence

- [Workspace binding](../../../../nebula_app/src/gpui_shell/workspace/details_panel.rs)
  captures the focused pane's execution context and SSH readiness.
- [Sidebar ownership](../../../../nebula_app/src/gpui_shell/resource_monitor.rs)
  owns the polling task, cancellation token and last completed snapshot.
- [Collector](../../../../nebula_app/src/resource_monitor.rs) produces typed snapshots;
  native sampling uses the sysinfo 0.31.4 version already present in the dependency graph.
- [Guest adapter](../../../../nebula_app/src/resource_monitor/guest.rs) reuses authenticated
  SSH channels and the existing frozen WSL command context.
- [Guest transport](../../../../nebula_app/src/resource_monitor/guest/transport.rs)
  discovers Linux/macOS through uname and Windows through its native PowerShell environment.
- Windows process units and creation identity follow [Win32_Process](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process);
  inverse CPU counters follow [WMI performance sampling](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/WmiSdk/wmi-tasks--performance-monitoring.md).
- [Boundary tests](../../../../nebula_app/src/resource_monitor/tests.rs) specify CPU
  baseline, boot identity, PID reuse, Docker port semantics and missing-counter behavior.

## Decision

The shared right sidebar owns a lazy resource view. One background collector performs
one sample at a time, then waits two seconds. Hiding the panel, pausing it, changing its
target or dropping the view cancels the owned task and signals bounded subprocess work.
Late results are rejected before updating UI state. Failures retain the previous sample
with an explicit stale indication; unavailable measurements are never displayed as zero.

Native counters come from sysinfo, enabled with the GPUI product feature. Guest probes use system-provided tools without Python,
an uploaded binary or a resident service. Linux reads /proc through sh and POSIX awk;
macOS uses sysctl, vm_stat, top, ps and df; Windows uses PowerShell and local CIM providers.
SSH connection generations, OS boot identities and process creation identities delimit deltas.
Discovery is cached only within one connection generation. The Windows stdin bootstrap is
UTF-16LE encoded for either OpenSSH default shell and emits UTF-8 responses; POSIX probes
explicitly enter sh. Arguments are quoted as literals rather than interpolated as source.

Guest JSON is converted into a typed counter snapshot before applying rates. Linux process
ticks share the whole-host CPU clock; Windows uses 100ns process ticks; macOS uses cumulative
ps milliseconds and elapsed sample time. macOS host CPU comes from an interval top sample.
All process percentages represent a share of the host's entire logical CPU capacity.

The sidebar has Overview and Docker tabs. NVIDIA telemetry uses nvidia-smi and exposes
GPU cards within Overview only after a supported device is reported. Compute-process memory coverage is stated in the UI. Docker commands
run only in the Docker category. The daemon endpoint remains visible because a context
can target a different host; Docker CPU percentages use that daemon's logical CPU count.
Container rows keep compact numeric columns and expand port mappings only on request.
Docker statistics request full IDs and match inspection rows exactly. Missing Docker
and empty inventories stop subsequent queries instead of probing unused statistics.
Published bindings, unpublished exposed ports and stopped-container configurations have
different presentation semantics. Port mappings make no network reachability claim.

## Rejected alternatives

- Terminal-input commands: interfere with interactive programs and terminal history.
- OS/network queries during rendering: make slow hosts and tools block the UI thread.
- Independent overlapping timers: permit stale host results and lose CPU delta ownership.
- Inferring a host from cwd text: cannot distinguish a native path from a guest scope.
- Installing a remote daemon: adds deployment and authentication responsibilities to a viewer.
- Requiring Python: excludes otherwise supported SSH hosts with only OS-provided tools.
- A single command for every OS: hides different counter units and native inspection interfaces.
- Fabricating unsupported counters: obscures permissions and platform coverage.
- Categorical line charts: the dependency's point scale and mandatory numeric values
  do not preserve elapsed-time spacing and missing-value gaps in the resource history.

## Consequences

The view has no persistence and performs read-only probes. Closing it cancels collection;
reopening it starts fresh CPU baselines. Native sysinfo calls themselves are not preemptible.
Local command owners reap cancelled children; SSH cancellation closes the owned channel.
The client bounds each command's waiting time and response size; remote process termination
after channel closure is controlled by the SSH server. Refresh cadence includes query time.
GPU support currently covers NVIDIA only. Protected processes can lack counters. Disk status
means filesystem capacity, not SMART health. macOS reports active, wired and compressed memory;
its ps creation identity has second precision and its sample clock uses wall time. Backward clock
changes invalidate deltas. macOS process I/O is unavailable; Windows process I/O includes non-disk
activity. I/O coverage is explained in the UI; missing measurements are not represented as zero.
Overview returns only the five highest CPU consumers. Ranking still scans lightweight
process CPU counters: native sysinfo preserves its baseline, while Linux/macOS/Windows
remote overview probes compare counters inside one bounded invocation. Linux reads
resident memory only for selected overview PIDs. Overview omits per-process I/O and
NVIDIA compute-process queries; guest overview output is bounded to five rows.

The read-only process dialog owns a detail-demand token and input/sidebar subscriptions.
The existing collector includes full process responses only while that token is active;
no additional collector or timer is created. Closing or pausing the modal releases demand
for subsequent queries. A bounded query already in flight may finish. A host switch replaces
the token, preventing an old modal from requesting details for another host even if focus
returns to the original key before observer delivery. The dialog rejects overview-only
responses and freezes on host/token changes. All collected detail rows remain searchable
and ranked through a virtualized table. The dialog shares an immutable row collection
with render callbacks and sorts indices instead of copying complete rows during each
render. Collector and UI rankings use the same process comparison rule; sidebar and
dialog freshness derive from one collector-status rule.

Overview adds OS product/version, uptime, per-core CPU and interface-scoped network counters.
Native network counters use sysinfo; Linux guests use /proc/net/dev, excluding loopback,
docker0 and veth links. Interface topology changes or counter regressions invalidate rates.
Aggregated interfaces can include tunnels or bridges, so totals are not a guarantee of
external-link traffic. Windows/macOS guest overview currently leaves network/per-core
metrics unavailable; native hosts have sysinfo core and network metrics. Linux guest memory
breaks down used, free and buffer/cache bytes; other adapters retain their existing memory
accounting without inventing a cache estimate. Linux and macOS guest CPU samples expose
user/system shares from the same interval as their total. Native sysinfo and Windows
currently expose total CPU only; their missing component values remain unavailable.

Disk overview selects the target-reported system mount: `/` on Unix and the sampled
Windows OS's system drive. Boot, kernel, temporary and OS-internal mounts stay out of
the compact overview. Other data mounts are collapsed initially. Linux mountinfo supplies
volume identity and filesystem-relative roots, so only proven identical mounts are
deduplicated; different subvolumes and unknown identities remain distinct. The complete
inventory remains available in a read-only dialog pinned to the opening host's immutable
snapshot. Native adapters without mount-root metadata retain uncertain duplicates.
Disk rows expose filesystem type where available; disk I/O rates and SMART health are
not collected.

Charts and card surfaces resolve the current theme's semantic/chart palette on render,
including light, dark and custom appearances. Category labels remain visible alongside
colored markers. Persistent CPU/network chart entities own pointer state and cached
paint bounds on the UI thread, sharing immutable series across pointer repaints. Hover selects the nearest actual sample and paints a
crosshair with a window-clamped popover; missing counters remain `—`. Parent refreshes
replace chart data without creating timers. Host changes clear inspection state.
No fixed black/green palette or bitmap chart assets are used.
Guest collection limits are 16,384 processes,
256 containers, an eight-second system query budget and a separate eight-second optional-tool
budget. Individual optional queries wait at most three seconds. Initial discovery adds at most
five seconds. Required system commands, a usable shell and inspection permissions remain necessary.

## Validation

Rust formatting, PowerShell parsing, POSIX awk syntax and translation key checks passed.
Awk parsing uses an initial unconditional exit and does not execute inspection commands.
The repository architecture checker passed during implementation.
Behavioral tests are supplied but have not been executed. No build, native UI acceptance,
real GPU/Docker validation, or SSH/WSL integration run has been performed.

## Supersedes

None.

## Revisit when

Additional GPU vendors, other SSH operating systems, persistent history or container control
operations are required; or measured probe costs warrant category-specific sampling.
