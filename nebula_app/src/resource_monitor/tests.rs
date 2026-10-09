//! Telemetry boundaries protect host identity, absent counters and port semantics.
use super::devices::{Output, Tools};
use super::*;

/// Overview and detail rankings share missing-value ordering and deterministic PID ties.
#[test]
fn process_ordering_preserves_unavailable_values_and_pid_ties() {
    let process = |pid, cpu, memory, io_rate, vram| Process {
        pid,
        name: "worker".into(),
        cpu,
        memory,
        io_rate,
        vram,
    };
    let mut rows = vec![
        process(3, None, 400, None, None),
        process(2, Some(0.0), 200, Some(0.0), Some(0)),
        process(1, Some(0.0), 100, Some(20.0), Some(100)),
    ];
    rows.sort_by(|a, b| a.compare_usage(b, Ranking::Cpu));
    assert_eq!(rows.iter().map(|process| process.pid).collect::<Vec<_>>(), [1, 2, 3]);
    rows.sort_by(|a, b| a.compare_usage(b, Ranking::Memory));
    assert_eq!(rows[0].pid, 3);
    for ranking in [Ranking::Io, Ranking::Vram] {
        rows.sort_by(|a, b| a.compare_usage(b, ranking));
        assert_eq!(rows.iter().map(|process| process.pid).collect::<Vec<_>>(), [1, 2, 3]);
    }
}

/// An absent Docker CLI stops queries without being reported as a daemon failure.
#[test]
fn absent_docker_stops_device_queries() {
    let mut queries = 0;
    let tools = devices::capture(Request { docker: true, processes: false }, None, |program, _| {
        if program == "docker" {
            queries += 1;
        }
        Output::default()
    });
    assert_eq!(queries, 1);
    assert!(matches!(devices::decode(tools, &mut []).1, Probe::Unavailable));
}

/// Empty container inventory does not query CPU topology or live statistics.
#[test]
fn empty_docker_inventory_skips_statistics() {
    let tools =
        devices::capture(Request { docker: true, processes: false }, None, |program, args| {
            if program != "docker" {
                return Output::default();
            }
            let stdout = match args {
                ["context", ..] => "unix:///var/run/docker.sock",
                ["container", "ls", ..] => "",
                _ => panic!("Empty inventory must not issue additional queries"),
            };
            Output { stdout: Some(stdout.into()), error: None }
        });
    let (_, Probe::Ready(docker)) = devices::decode(tools, &mut []) else {
        panic!("valid empty inventory")
    };
    assert!(docker.containers.is_empty());
    assert!(docker.stats_error.is_none());
}

/// Nontruncated statistics match exact IDs and cannot be attached to a similar container prefix.
#[test]
fn docker_statistics_use_exact_container_identity() {
    let inspect = r#"[{"Id":"abc123","Name":"/api","State":{"Status":"running","Running":true},"HostConfig":{"PortBindings":null},"Config":{"ExposedPorts":null}}]"#;
    let tools =
        devices::capture(Request { docker: true, processes: false }, None, |program, args| {
            if program != "docker" {
                return Output::default();
            }
            let stdout = match args {
                ["context", ..] => "unix:///var/run/docker.sock",
                ["container", "ls", ..] => "abc123",
                ["container", "inspect", ..] => inspect,
                ["info", ..] => "4",
                ["stats", ..] => {
                    assert!(args.contains(&"--no-trunc"));
                    r#"{"ID":"abc","CPUPerc":"80%","MemUsage":"1MiB / 4MiB"}"#
                },
                _ => panic!("Unexpected device query"),
            };
            Output { stdout: Some(stdout.into()), error: None }
        });
    let (_, Probe::Ready(docker)) = devices::decode(tools, &mut []) else {
        panic!("valid container response")
    };
    assert_eq!(docker.containers[0].cpu, None);
    assert_eq!(docker.containers[0].memory, None);
}

/// Construct an external Linux sample without invoking OS tools or SSH.
fn linux_sample(
    total: u64,
    idle: u64,
    ticks: u64,
    start: u64,
    boot: &str,
    time: u64,
) -> guest::RawSample {
    serde_json::from_str(&format!(r#"{{"boot":"{boot}","timestamp":{time},"summary":{{"name":"Test OS","version":"1","uptime":10,"free":100,"cache":null,"cpu_cores":[],"cpu_user":null,"cpu_system":null,"network":null}},"processes_complete":true,"total":{total},"idle":{idle},"cpu":null,"ticks_per_second":null,"cores":8,"memory":100,"total_memory":200,"disks":[],"disk_error":null,"processes":[{{"pid":42,"name":"worker","start":"{start}","ticks":{ticks},"memory":64,"io_bytes":100,"cpu":null}}],"nvidia_available":false,"docker_available":false,"docker_host":null,"docker_context":null}}"#)).unwrap()
}

/// CPU deltas are whole-host normalized and never cross PID reuse or host reboot.
#[test]
fn linux_cpu_deltas_preserve_process_and_boot_identity() {
    let mut collector = Collector::new(Target::Native);
    let first = collector
        .decode_guest(
            linux_sample(1000, 800, 10, 1, "boot-a", 1),
            Tools::default(),
            guest::GuestOs::Linux,
        )
        .unwrap();
    assert_eq!(first.cpu, None);
    assert_eq!(first.processes[0].cpu, None);
    let second = collector
        .decode_guest(
            linux_sample(1200, 900, 50, 1, "boot-a", 3),
            Tools::default(),
            guest::GuestOs::Linux,
        )
        .unwrap();
    assert_eq!(second.cpu, Some(50.0));
    assert_eq!(second.processes[0].cpu, Some(20.0));
    let reused = collector
        .decode_guest(
            linux_sample(1400, 1000, 100, 2, "boot-a", 5),
            Tools::default(),
            guest::GuestOs::Linux,
        )
        .unwrap();
    assert_eq!(reused.processes[0].cpu, None);
    let reboot = collector
        .decode_guest(
            linux_sample(200, 100, 10, 2, "boot-b", 7),
            Tools::default(),
            guest::GuestOs::Linux,
        )
        .unwrap();
    assert_eq!(reboot.cpu, None);
}

/// Construct a fixed-frequency process clock response shared by macOS and Windows fixtures.
fn clock_sample(frequency: u64, time: u64, ticks: u64, io: &str) -> guest::RawSample {
    let (total, idle, cpu) = if frequency == 1000 {
        ("null".to_owned(), "null".to_owned(), "25.0")
    } else {
        ((time * frequency * 4).to_string(), (time * frequency * 3).to_string(), "null")
    };
    serde_json::from_str(&format!(r#"{{"boot":"same-boot","timestamp":{time},"summary":{{"name":"Test OS","version":"1","uptime":10,"free":100,"cache":null,"cpu_cores":[],"cpu_user":null,"cpu_system":null,"network":null}},"processes_complete":true,"total":{total},"idle":{idle},"cpu":{cpu},"ticks_per_second":{frequency},"cores":4,"memory":100,"total_memory":200,"disks":null,"disk_error":"volume unavailable","processes":[{{"pid":42,"name":"a \"quoted\" process","start":"same-start","ticks":{ticks},"memory":64,"io_bytes":{io},"cpu":null}}],"nvidia_available":false,"docker_available":false,"docker_host":null,"docker_context":null}}"#)).unwrap()
}

/// macOS millisecond CPU deltas normalize across cores without inventing process I/O.
#[test]
fn macos_cpu_clocks_and_optional_metrics_remain_independent() {
    let mut collector = Collector::new(Target::Native);
    let first = collector
        .decode_guest(clock_sample(1000, 10, 1000, "null"), Tools::default(), guest::GuestOs::Macos)
        .unwrap();
    assert_eq!(first.cpu, Some(25.0));
    assert_eq!(first.processes[0].cpu, None);
    let second = collector
        .decode_guest(clock_sample(1000, 12, 3000, "null"), Tools::default(), guest::GuestOs::Macos)
        .unwrap();
    assert_eq!(second.processes[0].cpu, Some(25.0));
    assert_eq!(second.processes[0].io_rate, None);
    assert!(matches!(second.disks, Probe::Failed(_)));
    assert!(matches!(second.docker, Probe::Skipped));
    assert!(second.platform.starts_with("macOS"));
}

/// Windows CPU uses 100ns ticks and reports counter regression as unavailable, not zero.
#[test]
fn windows_cpu_and_io_rates_reject_regressed_counters() {
    let mut collector = Collector::new(Target::Native);
    collector
        .decode_guest(
            clock_sample(10000000, 10, 10000000, "100"),
            Tools::default(),
            guest::GuestOs::Windows,
        )
        .unwrap();
    let second = collector
        .decode_guest(
            clock_sample(10000000, 12, 30000000, "300"),
            Tools::default(),
            guest::GuestOs::Windows,
        )
        .unwrap();
    assert_eq!(second.cpu, Some(25.0));
    assert_eq!(second.processes[0].cpu, Some(25.0));
    assert_eq!(second.processes[0].io_rate, Some(100.0));
    let regressed = collector
        .decode_guest(clock_sample(10000000, 14, 1, "1"), Tools::default(), guest::GuestOs::Windows)
        .unwrap();
    assert_eq!(regressed.processes[0].cpu, None);
    assert_eq!(regressed.processes[0].io_rate, None);
    assert!(regressed.platform.starts_with("Windows"));
}

/// Published, exposed and stopped bindings survive Docker's null and dynamic port fields.
#[test]
fn docker_ports_and_remote_daemon_cpu_are_explicit() {
    let inspect = r#"[
        {"Id":"abc123","Name":"/api","State":{"Status":"running","Running":true},"HostConfig":{"PortBindings":{"8000/tcp":[{"HostIp":"","HostPort":"8080"}]}},"NetworkSettings":{"Ports":{"8000/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"},{"HostIp":"::","HostPort":"8080"}]}},"Config":{"ExposedPorts":{"8000/tcp":{},"9000/udp":{}}}},
        {"Id":"def456","Name":"/stopped","State":{"Status":"exited","Running":false},"HostConfig":{"PortBindings":{"80/tcp":[{"HostIp":"127.0.0.1","HostPort":"8081"}]}},"NetworkSettings":{"Ports":null},"Config":{"ExposedPorts":{"80/tcp":{}}}},
        {"Id":"ghi789","Name":"/no-ports","State":{"Status":"running","Running":true},"HostConfig":{"PortBindings":null},"NetworkSettings":{"Ports":null},"Config":{"ExposedPorts":null}}
    ]"#;
    let output = |value: &str| Output { stdout: Some(value.into()), error: None };
    let tools = Tools {
        inspect: output(inspect),
        cores: output("8"),
        endpoint: output("ssh://gpu-node"),
        stats: output(r#"{"ID":"abc123","CPUPerc":"160.0%","MemUsage":"1.5GiB / 8GiB"}"#),
        docker_requested: true,
        ..Tools::default()
    };
    let (_, Probe::Ready(docker)) = devices::decode(tools, &mut []) else {
        panic!("valid Docker response")
    };
    assert_eq!(docker.endpoint, "ssh://gpu-node");
    assert_eq!(docker.containers[0].cpu, Some(20.0));
    assert_eq!(docker.containers[0].memory, Some(1610612736));
    assert_eq!(docker.containers[0].ports.len(), 3);
    assert!(
        docker.containers[0]
            .ports
            .iter()
            .any(|port| port.container == "9000/udp" && port.host.is_none())
    );
    assert!(!docker.containers[1].running);
    assert_eq!(docker.containers[1].cpu, None);
    assert_eq!(docker.containers[1].ports[0].host.as_deref(), Some("8081"));
    assert!(docker.containers[2].ports.is_empty());
}

/// Unsupported GPU counters remain absent, while a genuine reported zero stays zero.
#[test]
fn gpu_measurements_distinguish_unsupported_from_zero() {
    let tools = Tools {
        gpu: Output { stdout: Some("0, NVIDIA GPU, [N/A], 0, 8192".into()), error: None },
        ..Tools::default()
    };
    let (Probe::Ready(gpus), Probe::Skipped) = devices::decode(tools, &mut []) else {
        panic!("valid GPU response")
    };
    assert_eq!(gpus[0].usage, None);
    assert_eq!(gpus[0].used, Some(0));
    assert_eq!(gpus[0].total, Some(8 * 1024 * 1024 * 1024));
}

/// Cancellation before sampling performs no native process, filesystem or network work.
#[test]
fn cancelled_sample_never_starts_collection() {
    let mut collector = Collector::new(Target::Native);
    assert!(
        collector
            .sample(Request { docker: true, processes: true }, &Arc::new(AtomicBool::new(true)))
            .is_err()
    );
}

/// Overview cannot be mistaken for a complete process response or return more than five rows.
#[test]
fn overview_responses_enforce_the_process_boundary() {
    let mut raw = linux_sample(1000, 800, 10, 1, "boot-a", 1);
    raw.processes_complete = false;
    let mut collector = Collector::new(Target::Native);
    let snapshot = collector.decode_guest(raw, Tools::default(), guest::GuestOs::Linux).unwrap();
    assert!(!snapshot.processes_complete);
    let mut raw = linux_sample(1000, 800, 10, 1, "boot-a", 1);
    raw.processes_complete = false;
    let encoded = serde_json::json!({"pid": 99, "name": "extra", "start": "1", "ticks": 0, "memory": 0, "io_bytes": null, "cpu": null});
    for _ in 0..5 {
        raw.processes.push(serde_json::from_value(encoded.clone()).unwrap());
    }
    assert!(collector.decode_guest(raw, Tools::default(), guest::GuestOs::Linux).is_err());
}

/// Interface changes and counter resets cannot create negative or unrelated traffic rates.
#[test]
fn network_rates_require_matching_interfaces_and_monotonic_counters() {
    let old = Network {
        interfaces: vec!["eth0".into()],
        received: 100,
        transmitted: 200,
        receive_rate: None,
        transmit_rate: None,
    };
    let mut current = Network { received: 300, transmitted: 800, ..old.clone() };
    update_network_rates(&mut current, &old, 2.0);
    assert_eq!(current.receive_rate, Some(100.0));
    assert_eq!(current.transmit_rate, Some(300.0));
    let mut reset = Network { received: 1, transmitted: 1, ..old.clone() };
    update_network_rates(&mut reset, &old, 2.0);
    assert_eq!(reset.receive_rate, None);
    let mut changed =
        Network { interfaces: vec!["eth1".into()], received: 300, transmitted: 800, ..old.clone() };
    update_network_rates(&mut changed, &old, 2.0);
    assert_eq!(changed.transmit_rate, None);
}
