//! Read-only host snapshots; native and authenticated guest probes share typed results.

mod devices;
pub(crate) mod filesystems;
mod guest;
#[cfg(test)]
mod tests;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crate::runtime_exec::PaneExecContext;

/// A collection plan separates compact overview output from explicitly requested process details.
#[derive(Clone, Copy)]
pub(crate) struct Request {
    /// Query container metadata and statistics only in the Docker tab.
    pub docker: bool,
    /// Include every visible process and its optional I/O and GPU counters.
    pub processes: bool,
}

/// Host summary fields are small enough to collect with the overview.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Summary {
    /// System product or distribution name.
    pub name: String,
    /// OS product version, excluding host-specific identifiers.
    pub version: String,
    /// Elapsed seconds since boot, absent when unavailable.
    pub uptime: Option<u64>,
    /// Free or available memory belonging to the displayed accounting breakdown, in bytes.
    pub free: u64,
    /// Reclaimable cache in bytes when the platform provides a matching memory breakdown.
    pub cache: Option<u64>,
    /// Logical core percentages; an empty list means per-core sampling is unavailable.
    pub cpu_cores: Vec<Option<f32>>,
    /// Interval user-mode CPU share, absent when the provider exposes only total usage.
    pub cpu_user: Option<f32>,
    /// Interval system-mode CPU share measured over the same interval as user-mode usage.
    pub cpu_system: Option<f32>,
    /// Network counters and interval rates for the selected non-loopback interfaces.
    pub network: Option<Network>,
}

/// Aggregate interface traffic; cumulative totals are scoped to the current host boot.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Network {
    /// Interface names make the scope of aggregate counters explicit.
    pub interfaces: Vec<String>,
    /// Cumulative received bytes across those interfaces.
    pub received: u64,
    /// Cumulative transmitted bytes across those interfaces.
    pub transmitted: u64,
    /// Received bytes per second, absent until a comparable baseline exists.
    pub receive_rate: Option<f64>,
    /// Transmitted bytes per second, absent until a comparable baseline exists.
    pub transmit_rate: Option<f64>,
}

/// Execution boundary captured from the focused terminal, never its interactive input.
#[derive(Clone)]
pub(crate) enum Target {
    /// The desktop host's native OS APIs.
    Native,
    /// Frozen WSL distribution, user and environment from pane creation.
    Wsl(PaneExecContext),
    /// An already authenticated SSH destination; probes never initiate authentication.
    Ssh(String),
}

/// Optional telemetry distinguishes missing tools, failures and deferred work from zero.
#[derive(Clone, Debug)]
pub(crate) enum Probe<T> {
    /// This telemetry tool or supported device is unavailable.
    Unavailable,
    /// Collection failed; the diagnostic belongs to this sample only.
    Failed(String),
    /// An authoritative response, including an empty collection.
    Ready(T),
    /// This category was not requested during this sample.
    Skipped,
}

/// Per-process measurements use stable identity and explicit unavailable values.
#[derive(Clone, Debug)]
pub(crate) struct Process {
    /// OS process identifier, scoped to the sampled host.
    pub pid: u32,
    /// Process name from OS metadata, never executable shell text.
    pub name: String,
    /// Share of all logical CPUs, 0–100%; absent before a second sample.
    pub cpu: Option<f32>,
    /// Resident memory in bytes.
    pub memory: u64,
    /// OS-accounted read/write I/O bytes/second; Windows includes non-disk I/O, macOS is unavailable.
    pub io_rate: Option<f64>,
    /// NVIDIA compute-process memory in bytes, absent when not reported.
    pub vram: Option<u64>,
}

/// Resource ordering shared by compact collection and process inspection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ranking {
    /// Whole-host CPU share in percent.
    Cpu,
    /// Resident memory in bytes.
    Memory,
    /// OS-accounted I/O byte rate.
    Io,
    /// Reported GPU process memory in bytes.
    Vram,
}

impl Process {
    /// Compare descending resource usage with unavailable values last and stable PID ties.
    /// Both processes must belong to the same host sample; this does not mutate either row.
    pub(crate) fn compare_usage(&self, other: &Self, ranking: Ranking) -> std::cmp::Ordering {
        let metric = |process: &Self| match ranking {
            Ranking::Cpu => process.cpu.map(f64::from),
            Ranking::Memory => Some(process.memory as f64),
            Ranking::Io => process.io_rate,
            Ranking::Vram => process.vram.map(|bytes| bytes as f64),
        };
        metric(other)
            .partial_cmp(&metric(self))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(self.pid.cmp(&other.pid))
    }
}

/// Mounted filesystem capacity; no SMART health claim is inferred from free space.
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct Disk {
    /// Mount point or Windows volume path.
    pub mount: String,
    /// Total filesystem capacity in bytes.
    pub total: u64,
    /// Available space for the current user in bytes.
    pub available: u64,
    /// Filesystem type when reported by the OS.
    pub filesystem: Option<String>,
    /// OS-reported filesystem source; presentation never infers identity from capacity.
    pub source: Option<String>,
    /// Filesystem-relative mounted root, used to distinguish subvolumes and bind mounts.
    pub root: Option<String>,
    /// Whether this mount is the sampled host's system volume.
    pub system: bool,
}

/// A supported NVIDIA device's independently reported telemetry.
#[derive(Clone, Debug)]
pub(crate) struct Gpu {
    /// Device index and model name.
    pub name: String,
    /// Engine utilization in percent; unsupported counters remain absent.
    pub usage: Option<f32>,
    /// Used framebuffer memory in bytes.
    pub used: Option<u64>,
    /// Total framebuffer memory in bytes.
    pub total: Option<u64>,
}

/// A container's published or declared port, independent of network reachability.
#[derive(Clone, Debug)]
pub(crate) struct Port {
    /// Docker's container-side port and protocol, for example `8000/tcp`.
    pub container: String,
    /// Host binding address; absent for an unpublished exposed port.
    pub address: Option<String>,
    /// Host-side port; absent for an unpublished exposed port.
    pub host: Option<String>,
}

/// One Docker container with optional live resource usage.
#[derive(Clone, Debug)]
pub(crate) struct Container {
    /// Full container identity, stable across list reordering.
    pub id: String,
    /// Docker container name without its leading slash.
    pub name: String,
    /// Docker's observed lifecycle status.
    pub status: String,
    /// Whether the container was running at inspection time.
    pub running: bool,
    /// CPU share normalized to the host's logical CPU count.
    pub cpu: Option<f32>,
    /// Current working-set memory in bytes, absent for stopped containers.
    pub memory: Option<u64>,
    /// Declared ports and host bindings; stopped bindings are configuration only.
    pub ports: Vec<Port>,
}

/// Docker endpoint identity remains visible when a context targets another host.
#[derive(Clone, Debug)]
pub(crate) struct Docker {
    /// Selected Docker context's actual daemon endpoint.
    pub endpoint: String,
    /// Container metadata from that endpoint.
    pub containers: Vec<Container>,
    /// Statistics failure can coexist with successfully inspected containers.
    pub stats_error: Option<String>,
}

/// Immutable measurements consumed by the sidebar without performing I/O.
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    /// Time the complete sample became available, used to label stale results.
    pub sampled: Instant,
    /// OS name and logical CPU count.
    pub platform: String,
    /// Compact system, memory, core and network overview.
    pub summary: Summary,
    /// Whole-host CPU utilization; absent during baseline collection.
    pub cpu: Option<f32>,
    /// Used physical memory in bytes.
    pub memory: u64,
    /// Total physical memory in bytes.
    pub total_memory: u64,
    /// Filesystem collection may fail independently of CPU and memory.
    pub disks: Probe<Vec<Disk>>,
    /// Visible processes only; protected processes may lack individual counters.
    pub processes: Vec<Process>,
    /// True only for a response collected while process details were requested.
    pub processes_complete: bool,
    /// NVIDIA telemetry availability and per-device measurements.
    pub gpus: Probe<Vec<Gpu>>,
    /// Docker is sampled only while its category is visible.
    pub docker: Probe<Docker>,
}

/// A single owned sampling sequence; moving it between workers preserves CPU baselines.
pub(crate) struct Collector {
    /// Captured execution boundary for the lifetime of this sequence.
    target: Target,
    /// Native CPU/process baselines, allocated only on the worker.
    native: sysinfo::System,
    /// Native interface counters retained between refreshes.
    networks: sysinfo::Networks,
    /// Previous native aggregate network counters, used only across identical interface sets.
    network_previous: Option<Network>,
    /// Native network sampling time, independent of slower optional device queries.
    network_refreshed: Option<Instant>,
    /// Previous guest cumulative counters, scoped to its connection generation.
    previous: Option<guest::RawSample>,
    /// Native refresh timestamp; absent before the first sample.
    refreshed: Option<Instant>,
    /// Whether the previous native sample collected process I/O, delimiting valid I/O deltas.
    refreshed_details: bool,
    /// Captured SSH generation prevents deltas across reconnections.
    connection: Option<u64>,
    /// Guest OS discovered on this connection generation, never inferred from the desktop OS.
    guest_os: Option<guest::GuestOs>,
}

impl Collector {
    /// Allocate a host-specific sampling sequence on a background worker.
    pub(crate) fn new(target: Target) -> Self {
        Self {
            target,
            native: sysinfo::System::new(),
            networks: sysinfo::Networks::new(),
            network_previous: None,
            network_refreshed: None,
            previous: None,
            refreshed: None,
            refreshed_details: false,
            connection: None,
            guest_os: None,
        }
    }

    /// Collect one bounded sample off the UI thread; cancellation reaps local children.
    /// Docker runs only when requested. Errors preserve the caller's previous snapshot.
    pub(crate) fn sample(
        &mut self,
        request: Request,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Snapshot, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if !matches!(self.target, Target::Native) {
            return self.sample_guest(request, cancel);
        }
        self.native.refresh_cpu_usage();
        self.native.refresh_memory();
        let mut refresh = sysinfo::ProcessRefreshKind::new().with_cpu().with_memory();
        if request.processes {
            refresh = refresh.with_disk_usage();
        }
        self.native.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, refresh);
        let now = Instant::now();
        let seconds = self.refreshed.map(|at| now.duration_since(at).as_secs_f64());
        let cores = self.native.cpus().len().max(1) as f32;
        let mut processes = self
            .native
            .processes()
            .iter()
            .map(|(pid, process)| {
                let usage = process.disk_usage();
                Process {
                    pid: pid.as_u32(),
                    name: process.name().to_string_lossy().into_owned(),
                    cpu: seconds.map(|_| (process.cpu_usage() / cores).clamp(0.0, 100.0)),
                    memory: process.memory(),
                    io_rate: seconds.filter(|_| request.processes && self.refreshed_details).map(
                        |s| {
                            usage.read_bytes.saturating_add(usage.written_bytes) as f64
                                / s.max(0.001)
                        },
                    ),
                    vram: None,
                }
            })
            .collect::<Vec<_>>();
        // Device commands have independent budgets and share cancellation with the owning view.
        let raw = devices::capture_native(request, cancel);
        let (gpus, docker) = devices::decode(raw, &mut processes);
        // Ranking requires lightweight counters for all PIDs, but overview retains five rows only.
        if !request.processes {
            retain_top_processes(&mut processes);
        }
        self.networks.refresh_list();
        self.networks.refresh();
        let mut interfaces =
            self.networks.iter().filter(|(name, _)| monitored_interface(name)).collect::<Vec<_>>();
        interfaces.sort_by(|a, b| a.0.cmp(b.0));
        let mut network = Network {
            interfaces: interfaces.iter().map(|(name, _)| (*name).clone()).collect(),
            received: interfaces.iter().map(|(_, data)| data.total_received()).sum(),
            transmitted: interfaces.iter().map(|(_, data)| data.total_transmitted()).sum(),
            receive_rate: None,
            transmit_rate: None,
        };
        let network_now = Instant::now();
        let network_seconds =
            self.network_refreshed.map(|at| network_now.duration_since(at).as_secs_f64());
        if let (Some(old), Some(seconds)) = (&self.network_previous, network_seconds) {
            update_network_rates(&mut network, old, seconds);
        }
        self.network_previous = Some(network.clone());
        self.network_refreshed = Some(network_now);
        let summary = Summary {
            name: sysinfo::System::name().unwrap_or_default(),
            version: sysinfo::System::os_version().unwrap_or_default(),
            uptime: Some(sysinfo::System::uptime()),
            free: self.native.total_memory().saturating_sub(self.native.used_memory()),
            cache: None,
            cpu_user: None,
            cpu_system: None,
            cpu_cores: self
                .native
                .cpus()
                .iter()
                .map(|cpu| seconds.map(|_| cpu.cpu_usage().clamp(0.0, 100.0)))
                .collect(),
            network: (!network.interfaces.is_empty()).then_some(network),
        };
        let disks = sysinfo::Disks::new_with_refreshed_list();
        self.refreshed = Some(now);
        self.refreshed_details = request.processes;
        Ok(Snapshot {
            sampled: Instant::now(),
            platform: format!(
                "{} · {} CPU",
                sysinfo::System::name().unwrap_or_default(),
                cores as usize
            ),
            cpu: seconds.map(|_| self.native.global_cpu_usage().clamp(0.0, 100.0)),
            summary,
            memory: self.native.used_memory(),
            total_memory: self.native.total_memory(),
            disks: Probe::Ready(
                disks
                    .list()
                    .iter()
                    .map(|disk| Disk {
                        mount: disk.mount_point().display().to_string(),
                        total: disk.total_space(),
                        available: disk.available_space(),
                        filesystem: Some(disk.file_system().to_string_lossy().into_owned()),
                        source: Some(disk.name().to_string_lossy().into_owned()),
                        root: None,
                        system: filesystems::native_system_mount(disk.mount_point()),
                    })
                    .collect(),
            ),
            processes,
            processes_complete: request.processes,
            gpus,
            docker,
        })
    }
}

/// Render byte quantities consistently across host, process, GPU and Docker tables.
pub(crate) fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", value as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MiB", value as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KiB", value as f64 / 1024.0)
    }
}

/// Keep overview rankings deterministic without returning full process detail to the UI.
fn retain_top_processes(processes: &mut Vec<Process>) {
    processes.sort_by(|a, b| a.compare_usage(b, Ranking::Cpu));
    processes.truncate(5);
}

/// Exclude loopback and container-side links from host network totals.
fn monitored_interface(name: &str) -> bool {
    !matches!(name, "lo" | "lo0" | "docker0") && !name.starts_with("veth")
}

/// Calculate traffic rates only across an unchanged interface set and nonregressing counters.
fn update_network_rates(current: &mut Network, old: &Network, seconds: f64) {
    if current.interfaces == old.interfaces && seconds.is_finite() && seconds > 0.0 {
        current.receive_rate =
            current.received.checked_sub(old.received).map(|bytes| bytes as f64 / seconds);
        current.transmit_rate =
            current.transmitted.checked_sub(old.transmitted).map(|bytes| bytes as f64 / seconds);
    }
}
