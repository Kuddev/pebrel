//! Guest OS discovery, bounded execution and counter deltas share one authenticated scope.
use super::*;
use base64::Engine as _;
use serde::Deserialize;

mod transport;
use transport::Transport;

/// Supported guest families use only their system-provided shell and inspection tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuestOs {
    Linux,
    Macos,
    Windows,
}

/// One observable process lifetime and its cumulative counters.
#[derive(Deserialize)]
pub(super) struct RawProcess {
    /// Guest process identifier.
    pid: u32,
    /// OS-reported process name, escaped at the transport boundary.
    name: String,
    /// Boot-relative tick or OS creation timestamp; absent when identity is inaccessible.
    start: Option<String>,
    /// Cumulative CPU time in the sample's declared unit; absent when inaccessible.
    ticks: Option<u64>,
    /// Resident memory in bytes.
    memory: u64,
    /// Cumulative read/write I/O bytes; accounting coverage is OS-specific.
    io_bytes: Option<u64>,
    /// Interval CPU percentage computed remotely for compact overview rows.
    cpu: Option<f32>,
}

/// Counter response converted at the external JSON boundary before UI use.
#[derive(Deserialize)]
pub(super) struct RawSample {
    /// OS boot identity prevents deltas after host restart.
    boot: String,
    /// Compact OS, core, memory and network summary from the guest.
    summary: Summary,
    /// Distinguishes complete process responses from overview rankings.
    pub(super) processes_complete: bool,
    /// Guest sampling clock in seconds; backward changes invalidate all deltas.
    timestamp: f64,
    /// System CPU reference clock with source-specific normalization; absent on macOS.
    total: Option<u64>,
    /// Cumulative idle clock in the same unit and normalization as total; absent on macOS.
    idle: Option<u64>,
    /// Direct interval CPU percentage from macOS top; absent for delta-based sources.
    cpu: Option<f32>,
    /// Process CPU clock frequency; absent when process ticks share the aggregate CPU clock.
    ticks_per_second: Option<u64>,
    /// Host logical CPU count used to normalize process percentages.
    cores: usize,
    /// Used physical memory in bytes, according to the platform's accounting.
    memory: u64,
    /// Total physical memory in bytes.
    total_memory: u64,
    /// Filesystem capacity rows; absent when the independent query fails.
    disks: Option<Vec<Disk>>,
    /// Filesystem diagnostic, independent of process and memory availability.
    disk_error: Option<String>,
    /// Processes visible to the authenticated user.
    pub(super) processes: Vec<RawProcess>,
    /// Whether nvidia-smi resolves in the guest's noninteractive environment.
    nvidia_available: bool,
    /// Whether Docker CLI resolves in the guest's noninteractive environment.
    docker_available: bool,
    /// Guest Docker host override; empty or absent means no override.
    docker_host: Option<String>,
    /// Guest explicit Docker context, which takes precedence over Docker host.
    docker_context: Option<String>,
}

impl Collector {
    /// Collect through the captured WSL context or an existing SSH session, without installation.
    /// Discovery is cached only for one connection generation. Every command is bounded/cancellable.
    pub(super) fn sample_guest(
        &mut self,
        request: Request,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Snapshot, String> {
        let cancelled = || cancel.load(Ordering::Relaxed);
        let transport = match &self.target {
            Target::Wsl(context) => Transport::Wsl(context.clone()),
            Target::Ssh(destination) => {
                let runtime = crate::ssh_session::runtime().map_err(|error| error.to_string())?;
                let connection = runtime.block_on(async {
                    tokio::time::timeout(
                        Duration::from_secs(1),
                        crate::ssh_session::completion::capture(destination),
                    )
                    .await
                    .map_err(|_| "SSH connection unavailable".to_owned())?
                    .map_err(|error| error.to_string())
                })?;
                if self.connection != Some(connection.key()) {
                    self.previous = None;
                    self.guest_os = None;
                    self.connection = Some(connection.key());
                }
                Transport::Ssh(connection)
            },
            Target::Native => unreachable!("native sampling has its own adapter"),
        };
        let os = if let Some(os) = self.guest_os {
            os
        } else {
            let os = transport.discover(&cancelled)?;
            self.guest_os = Some(os);
            os
        };
        // System sampling is independent of optional drivers and container daemons.
        let script = system_script(os, request.processes);
        let output =
            transport.script(os, &script, Duration::from_secs(8), 4 * 1024 * 1024, &cancelled)?;
        let raw: RawSample = serde_json::from_slice(&output)
            .map_err(|error| format!("Invalid guest resource response: {error}"))?;
        let endpoint_override = raw.docker_host.clone().filter(|host| {
            !host.is_empty() && raw.docker_context.as_ref().is_none_or(String::is_empty)
        });
        // Each optional command shares a total deadline and the same authenticated transport.
        let deadline = Instant::now() + Duration::from_secs(8);
        let tools = devices::capture(request, endpoint_override, |program, args| {
            let available = match program {
                "nvidia-smi" => raw.nvidia_available,
                "docker" => raw.docker_available,
                _ => false,
            };
            if !available {
                return devices::Output::default();
            }
            let budget =
                deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(3));
            if budget.is_zero() || cancelled() {
                return devices::Output {
                    stdout: None,
                    error: Some("Device query cancelled or timed out".into()),
                };
            }
            let script = tool_script(os, program, args);
            match transport.script(os, &script, budget, 2 * 1024 * 1024, &cancelled) {
                Ok(bytes) => devices::Output {
                    stdout: Some(String::from_utf8_lossy(&bytes).into_owned()),
                    error: None,
                },
                Err(error) => devices::Output { stdout: None, error: Some(error) },
            }
        });
        self.decode_guest(raw, tools, os)
    }

    /// Validate system counters and calculate rates within one boot and process lifetime.
    /// Missing counters remain absent; regressions invalidate a delta instead of fabricating zero.
    pub(super) fn decode_guest(
        &mut self,
        mut raw: RawSample,
        tools: devices::Tools,
        os: GuestOs,
    ) -> Result<Snapshot, String> {
        if !raw.timestamp.is_finite()
            || raw.boot.is_empty()
            || raw.cores == 0
            || raw.total_memory == 0
            || raw.memory > raw.total_memory
            || raw.processes.len() > 16384
            || raw.ticks_per_second == Some(0)
            || raw.summary.free > raw.total_memory
            || raw
                .summary
                .cache
                .is_some_and(|cache| cache > raw.total_memory.saturating_sub(raw.summary.free))
            || raw
                .summary
                .free
                .saturating_add(raw.summary.cache.unwrap_or(0))
                .saturating_add(raw.memory)
                > raw.total_memory
            || raw.summary.cpu_cores.len() > 4096
            || [raw.summary.cpu_user, raw.summary.cpu_system]
                .into_iter()
                .flatten()
                .any(|cpu| !cpu.is_finite() || !(0.0..=100.0).contains(&cpu))
            || raw
                .summary
                .cpu_cores
                .iter()
                .flatten()
                .any(|cpu| !cpu.is_finite() || !(0.0..=100.0).contains(cpu))
            || raw.processes.iter().any(|process| {
                process.cpu.is_some_and(|cpu| !cpu.is_finite() || !(0.0..=100.0).contains(&cpu))
            })
            || (!raw.processes_complete && raw.processes.len() > 5)
            || raw.cpu.is_some_and(|cpu| !cpu.is_finite() || !(0.0..=100.0).contains(&cpu))
        {
            return Err("Invalid guest resource counters".into());
        }
        raw.processes.sort_by_key(|process| process.pid);
        // Host boot, clock direction and CPU topology delimit comparable cumulative counters.
        let previous = self.previous.as_ref().filter(|old| {
            old.boot == raw.boot
                && raw.timestamp > old.timestamp
                && old.cores == raw.cores
                && old.ticks_per_second == raw.ticks_per_second
        });
        let delta =
            previous.and_then(|old| raw.total?.checked_sub(old.total?)).filter(|total| *total > 0);
        let cpu = raw.cpu.or_else(|| {
            let idle = raw.idle?.checked_sub(previous?.idle?)?;
            let total = delta?;
            (idle <= total).then_some(100.0 * (1.0 - idle as f64 / total as f64) as f32)
        });
        let denominator = if let Some(frequency) = raw.ticks_per_second {
            previous
                .map(|old| (raw.timestamp - old.timestamp) * frequency as f64 * raw.cores as f64)
        } else {
            delta.map(|ticks| ticks as f64)
        };
        // Creation identity and monotonic counters prevent rates from crossing PID reuse.
        let mut processes = raw
            .processes
            .iter()
            .map(|process| {
                let old = previous
                    .and_then(|sample| {
                        sample
                            .processes
                            .binary_search_by_key(&process.pid, |p| p.pid)
                            .ok()
                            .map(|index| &sample.processes[index])
                    })
                    .filter(|old| {
                        process.start.as_ref().is_some_and(|start| !start.is_empty())
                            && old.start == process.start
                    });
                Process {
                    pid: process.pid,
                    name: process.name.clone(),
                    memory: process.memory,
                    cpu: process.cpu.or_else(|| {
                        old.and_then(|old| process.ticks?.checked_sub(old.ticks?))
                            .zip(denominator)
                            .map(|(ticks, total)| {
                                (100.0 * ticks as f64 / total).clamp(0.0, 100.0) as f32
                            })
                    }),
                    io_rate: old
                        .and_then(|old| process.io_bytes?.checked_sub(old.io_bytes?))
                        .zip(previous)
                        .map(|(bytes, sample)| bytes as f64 / (raw.timestamp - sample.timestamp)),
                    vram: None,
                }
            })
            .collect::<Vec<_>>();
        let (gpus, docker) = devices::decode(tools, &mut processes);
        let disks = raw.disks.take().map(Probe::Ready).unwrap_or_else(|| {
            Probe::Failed(raw.disk_error.take().unwrap_or_else(|| "Filesystem query failed".into()))
        });
        if !raw.processes_complete {
            retain_top_processes(&mut processes);
        }
        if let (Some(network), Some(old)) = (&mut raw.summary.network, previous) {
            if let Some(old_network) = &old.summary.network {
                update_network_rates(network, old_network, raw.timestamp - old.timestamp);
            }
        }
        let platform = match os {
            GuestOs::Linux => "Linux",
            GuestOs::Macos => "macOS",
            GuestOs::Windows => "Windows",
        };
        let snapshot = Snapshot {
            sampled: Instant::now(),
            platform: format!("{platform} · {} CPU", raw.cores),
            cpu,
            summary: raw.summary.clone(),
            memory: raw.memory,
            total_memory: raw.total_memory,
            disks,
            processes,
            processes_complete: raw.processes_complete,
            gpus,
            docker,
        };
        self.previous = Some(raw);
        Ok(snapshot)
    }
}

/// Build a probe exclusively from embedded source and fixed platform tool names.
fn system_script(os: GuestOs, details: bool) -> String {
    match os {
        GuestOs::Windows => format!(
            "$includeProcesses = ${}\n{}",
            if details { "true" } else { "false" },
            include_str!("windows_probe.ps1")
        ),
        GuestOs::Linux | GuestOs::Macos => {
            let source = match os {
                GuestOs::Linux => include_str!("linux_probe.awk"),
                GuestOs::Macos => include_str!("macos_probe.awk"),
                GuestOs::Windows => unreachable!(),
            };
            let awk = format!("{}\n{source}", include_str!("unix_probe.awk"));
            let files = if os == GuestOs::Linux { " /proc/[0-9]*/stat" } else { "" };
            format!(
                "LC_ALL=C; export LC_ALL; PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH; export PATH; exec awk -v details={} {}{files}\n",
                u8::from(details),
                quote_shell(&awk)
            )
        },
    }
}

/// Quote literal arguments for POSIX sh; values are never evaluated as shell source.
fn quote_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Encode a short stdin bootstrap so both Windows OpenSSH default shells preserve its quoting.
fn powershell_command() -> String {
    let bootstrap = "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$ErrorActionPreference='Stop';try { & ([scriptblock]::Create([Console]::In.ReadToEnd())) } catch { [Console]::Error.WriteLine($_.Exception.Message);exit 1 }";
    let bytes = bootstrap.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Build one optional tool invocation in the selected system shell with literal arguments.
fn tool_script(os: GuestOs, program: &str, args: &[&str]) -> String {
    let quote = |value: &str| {
        if os == GuestOs::Windows {
            format!("'{}'", value.replace('\'', "''"))
        } else {
            quote_shell(value)
        }
    };
    let command = std::iter::once(program)
        .chain(args.iter().copied())
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ");
    if os == GuestOs::Windows {
        format!(
            "$OutputEncoding=[Console]::OutputEncoding; & {command}; if ($LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}\n"
        )
    } else {
        format!(
            "LC_ALL=C; export LC_ALL; PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH; export PATH; exec {command}\n"
        )
    }
}
