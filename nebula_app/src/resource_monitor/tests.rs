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
    let mut collector = Collector::new();
    assert!(
        collector
            .sample(Request { docker: true, processes: true }, &Arc::new(AtomicBool::new(true)))
            .is_err()
    );
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
