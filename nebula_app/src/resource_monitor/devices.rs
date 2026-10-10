//! Bounded NVIDIA and Docker command adapters; external formats stop at this boundary.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::Command;

/// One bounded command outcome from either native or authenticated guest execution.
#[derive(Default)]
pub(super) struct Output {
    /// UTF-8 stdout on success; absent for missing tools or failures.
    pub stdout: Option<String>,
    /// Execution diagnostic, absent when the executable does not exist.
    pub error: Option<String>,
}

/// Device tool responses use the same protocol on native and guest hosts.
#[derive(Default)]
pub(super) struct Tools {
    /// CSV device counters from nvidia-smi.
    pub gpu: Output,
    /// CSV per-compute-process framebuffer memory.
    pub gpu_processes: Output,
    /// Docker container inspect JSON.
    pub inspect: Output,
    /// Docker one-shot statistics as JSON lines.
    pub stats: Output,
    /// Actual daemon endpoint, possibly remote even for a local terminal.
    pub endpoint: Output,
    /// Logical CPU count returned by the Docker daemon itself.
    pub cores: Output,
    /// False means Docker was deliberately not queried.
    pub docker_requested: bool,
}

/// Execute independent tools with one shared deadline and the view's cancellation token.
pub(super) fn capture_native(request: Request, cancel: &Arc<AtomicBool>) -> Tools {
    let deadline = Instant::now() + Duration::from_secs(8);
    let read = |program: &str, args: &[&str]| {
        let mut command = Command::new(program);
        command.args(args);
        match crate::platform::process_output::read_cancellable(
            command,
            deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(3)),
            2 * 1024 * 1024,
            &|| cancel.load(Ordering::Relaxed),
        ) {
            Ok(bytes) => {
                Output { stdout: Some(String::from_utf8_lossy(&bytes).into_owned()), error: None }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Output::default(),
            Err(error) => Output { stdout: None, error: Some(error.to_string()) },
        }
    };
    let endpoint_override =
        if !std::env::var("DOCKER_CONTEXT").is_ok_and(|context| !context.is_empty()) {
            std::env::var("DOCKER_HOST").ok()
        } else {
            None
        };
    capture(request, endpoint_override, read)
}

/// Query one host's tools through a bounded native or authenticated guest executor.
/// The endpoint override follows that host's Docker environment, not the desktop's.
pub(super) fn capture(
    request: Request,
    endpoint_override: Option<String>,
    mut read: impl FnMut(&str, &[&str]) -> Output,
) -> Tools {
    let gpu = read(
        "nvidia-smi",
        &[
            "--query-gpu=index,name,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ],
    );
    let gpu_processes = if request.processes && gpu.stdout.is_some() {
        read(
            "nvidia-smi",
            &["--query-compute-apps=pid,used_gpu_memory", "--format=csv,noheader,nounits"],
        )
    } else {
        Output::default()
    };
    let mut tools =
        Tools { gpu, gpu_processes, docker_requested: request.docker, ..Tools::default() };
    if !request.docker {
        return tools;
    }
    tools.endpoint =
        read("docker", &["context", "inspect", "--format", "{{.Endpoints.docker.Host}}"]);
    if tools.endpoint.stdout.is_none() && tools.endpoint.error.is_none() {
        return tools;
    }
    if let Some(host) = endpoint_override {
        tools.endpoint.stdout = Some(host);
    }
    let ids = read("docker", &["container", "ls", "-aq", "--no-trunc"]);
    tools.inspect = match ids.stdout {
        Some(ids) => {
            let ids: Vec<_> = ids.split_whitespace().collect();
            if ids.is_empty() {
                tools.inspect = Output { stdout: Some("[]".into()), error: None };
                return tools;
            } else if ids.len() > 256 {
                Output { stdout: None, error: Some("Container limit exceeded (256)".into()) }
            } else {
                let mut args = vec!["container", "inspect"];
                args.extend(ids);
                read("docker", &args)
            }
        },
        None => Output { stdout: None, error: ids.error },
    };
    if tools.inspect.stdout.is_some() {
        tools.cores = read("docker", &["info", "--format", "{{.NCPU}}"]);
        tools.stats =
            read("docker", &["stats", "--no-stream", "--no-trunc", "--format", "{{json .}}"]);
    }
    tools
}

/// Convert tool stdout into domain snapshots; no dynamic maps escape this function.
pub(super) fn decode(tools: Tools, processes: &mut [Process]) -> (Probe<Vec<Gpu>>, Probe<Docker>) {
    let gpu = match tools.gpu.stdout {
        Some(csv) => {
            let devices = csv
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    let fields: Vec<_> = line.split(',').map(str::trim).collect();
                    if fields.len() != 5 {
                        return Err("Invalid NVIDIA device response".to_owned());
                    }
                    Ok(Gpu {
                        name: format!("GPU {} · {}", fields[0], fields[1]),
                        usage: fields[2].parse::<f32>().ok().filter(|n| n.is_finite()),
                        used: mib(fields[3]),
                        total: mib(fields[4]),
                    })
                })
                .collect::<Result<Vec<_>, String>>();
            match devices {
                Ok(devices) => Probe::Ready(devices),
                Err(error) => Probe::Failed(error),
            }
        },
        None => tools.gpu.error.map(Probe::Failed).unwrap_or(Probe::Unavailable),
    };
    if let Some(csv) = tools.gpu_processes.stdout {
        for line in csv.lines() {
            let Some((pid, memory)) = line.split_once(',') else { continue };
            if let (Ok(pid), Some(memory)) = (pid.trim().parse::<u32>(), mib(memory.trim())) {
                if let Some(process) = processes.iter_mut().find(|process| process.pid == pid) {
                    process.vram = Some(process.vram.unwrap_or(0).saturating_add(memory));
                }
            }
        }
    }
    if !tools.docker_requested {
        return (gpu, Probe::Skipped);
    }
    let Some(json) = tools.inspect.stdout else {
        return (gpu, tools.inspect.error.map(Probe::Failed).unwrap_or(Probe::Unavailable));
    };
    let inspected = match serde_json::from_str::<Vec<Inspected>>(&json) {
        Ok(items) => items,
        Err(error) => return (gpu, Probe::Failed(error.to_string())),
    };
    let cores = tools
        .cores
        .stdout
        .as_deref()
        .and_then(|text| text.trim().parse::<f32>().ok())
        .filter(|n| n.is_finite() && *n > 0.0);
    let mut stats_error = tools.stats.error;
    let mut stats = Vec::new();
    if let Some(lines) = tools.stats.stdout {
        for line in lines.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<Stats>(line) {
                Ok(row) => stats.push(row),
                Err(error) => stats_error = Some(error.to_string()),
            }
        }
    }
    let containers = inspected
        .into_iter()
        .map(|item| {
            let stat = stats.iter().find(|stat| item.id == stat.id);
            let mut ports = Vec::new();
            let published = if item.state.running {
                item.network.ports.as_ref().or(item.host.ports.as_ref())
            } else {
                item.host.ports.as_ref()
            };
            for (container, bindings) in published.into_iter().flatten() {
                for binding in bindings.iter().flatten() {
                    let port = Port {
                        container: container.clone(),
                        address: Some(binding.address.clone()),
                        host: Some(binding.port.clone()),
                    };
                    if !ports.iter().any(|p: &Port| {
                        p.container == port.container
                            && p.address == port.address
                            && p.host == port.host
                    }) {
                        ports.push(port);
                    }
                }
            }
            for container in item.config.exposed.as_ref().into_iter().flat_map(|ports| ports.keys())
            {
                if !ports.iter().any(|port| &port.container == container) {
                    ports.push(Port { container: container.clone(), address: None, host: None });
                }
            }
            ports.sort_by(|a, b| {
                (&a.container, &a.address, &a.host).cmp(&(&b.container, &b.address, &b.host))
            });
            Container {
                id: item.id,
                name: item.name.trim_start_matches('/').into(),
                status: item.state.status,
                running: item.state.running,
                cpu: stat
                    .filter(|_| item.state.running)
                    .and_then(|stat| stat.cpu.trim_end_matches('%').parse::<f32>().ok())
                    .filter(|usage| usage.is_finite())
                    .zip(cores)
                    .map(|(usage, cores)| (usage / cores).clamp(0.0, 100.0)),
                memory: stat
                    .filter(|_| item.state.running)
                    .and_then(|stat| stat.memory.split('/').next())
                    .and_then(parse_size),
                ports,
            }
        })
        .collect();
    (
        gpu,
        Probe::Ready(Docker {
            endpoint: tools.endpoint.stdout.unwrap_or_else(|| "—".into()).trim().into(),
            containers,
            stats_error,
        }),
    )
}

/// Parse NVIDIA MiB counters; unsupported strings remain unavailable.
fn mib(text: &str) -> Option<u64> {
    text.parse::<u64>().ok()?.checked_mul(1024 * 1024)
}

/// Decode Docker's human-readable byte units without discarding magnitude.
fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let value = text[..split].parse::<f64>().ok()?;
    let scale = match text[split..].trim() {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1048576.0,
        "GiB" => 1073741824.0,
        "TiB" => 1099511627776.0,
        _ => return None,
    };
    (value.is_finite() && value >= 0.0).then_some((value * scale) as u64)
}

// Docker uses dynamic port/protocol keys. These maps exist only at the external JSON boundary.
#[derive(Default, Deserialize)]
struct PortSet {
    /// Published port bindings from HostConfig or NetworkSettings.
    #[serde(rename = "PortBindings", alias = "Ports", default)]
    ports: Option<BTreeMap<String, Option<Vec<Binding>>>>,
}
#[derive(Deserialize)]
struct Binding {
    /// Docker-reported host interface address.
    #[serde(rename = "HostIp")]
    address: String,
    /// Docker-reported host port number.
    #[serde(rename = "HostPort")]
    port: String,
}
#[derive(Default, Deserialize)]
struct Config {
    /// Container-declared port keys; values carry no business data.
    #[serde(rename = "ExposedPorts", default)]
    exposed: Option<BTreeMap<String, serde::de::IgnoredAny>>,
}
#[derive(Deserialize)]
struct State {
    /// Current Docker lifecycle status.
    #[serde(rename = "Status")]
    status: String,
    /// Whether live counters are meaningful for this container.
    #[serde(rename = "Running")]
    running: bool,
}
#[derive(Deserialize)]
struct Inspected {
    /// Full daemon-scoped container ID.
    #[serde(rename = "Id")]
    id: String,
    /// Container name, including Docker's leading slash.
    #[serde(rename = "Name")]
    name: String,
    /// Observed lifecycle state.
    #[serde(rename = "State")]
    state: State,
    /// Configured bindings, including those of stopped containers.
    #[serde(rename = "HostConfig")]
    host: PortSet,
    /// Effective network bindings.
    #[serde(rename = "NetworkSettings", default)]
    network: PortSet,
    /// Exposed ports that may have no host binding.
    #[serde(rename = "Config")]
    config: Config,
}
#[derive(Deserialize)]
struct Stats {
    /// Full container ID from the nontruncated statistics response.
    #[serde(rename = "ID")]
    id: String,
    /// Docker CPU percentage, where one logical core is 100%.
    #[serde(rename = "CPUPerc")]
    cpu: String,
    /// Used / limit memory with explicit units.
    #[serde(rename = "MemUsage")]
    memory: String,
}
