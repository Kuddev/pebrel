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
    let tools = devices::capture(Request { docker: true, processes: false }, |program, _| {
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
    let tools = devices::capture(Request { docker: true, processes: false }, |program, args| {
        if program != "docker" {
            return Output::default();
        }
        let stdout = match args {
            ["context", ..] => r#"["default","unix:///var/run/docker.sock"]"#,
            ["--host", "unix:///var/run/docker.sock", "container", "ls", ..] => "",
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

/// An unidentified target must never be paired with inventory from the current default.
#[test]
fn unresolved_context_stops_before_container_queries() {
    for response in ["not JSON", r#"["test",""]"#, r#"[null,"tcp://other"]"#] {
        let tools =
            devices::capture(Request { docker: true, processes: false }, |program, args| {
                if program != "docker" {
                    return Output::default();
                }
                assert_eq!(&args[..2], &["context", "inspect"]);
                Output { stdout: Some(response.to_owned()), error: None }
            });
        assert!(matches!(devices::decode(tools, &mut []).1, Probe::Failed(_)));
    }
}

/// Nontruncated statistics match exact IDs and cannot be attached to a similar container prefix.
#[test]
fn docker_statistics_use_exact_container_identity() {
    let inspect = r#"[{"Id":"abc123","Name":"/api","State":{"Status":"running","Running":true},"HostConfig":{"PortBindings":null},"Config":{"ExposedPorts":null}}]"#;
    let tools = devices::capture(Request { docker: true, processes: false }, |program, args| {
        if program != "docker" {
            return Output::default();
        }
        let stdout = match args {
            ["context", ..] => r#"["team","ssh://team"]"#,
            ["--context", "team", "container", "ls", ..] => "abc123",
            ["--context", "team", "container", "inspect", ..] => inspect,
            ["--context", "team", "info", ..] => "4",
            ["--context", "team", "stats", ..] => {
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

const SAMPLER_CHILD_READY: &str = "PEBREL_TEST_RESOURCE_SAMPLER_CHILD_READY";

// Re-enter the test binary so that the observed process owns the readiness marker on every OS.
fn idle_child(ready: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "resource_monitor::tests::new_process_needs_its_own_cpu_baseline"])
        .env(SAMPLER_CHILD_READY, ready)
        .stdout(std::process::Stdio::null());
    command
}

/// Cancellation after a real child starts cannot publish a successful sample or a new baseline.
#[test]
fn cancellation_during_device_read_discards_the_sample() {
    let mut collector = Collector::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    let result = collector.sample_with_devices(
        Request { docker: true, processes: false },
        &cancel,
        |_, cancel| {
            let interrupted = crate::platform::process_output::read_cancellable(
                idle_child(&ready),
                Duration::from_secs(15),
                1024,
                &|| {
                    if ready.is_file() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    cancel.load(Ordering::Relaxed)
                },
            );
            assert_eq!(interrupted.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
            Tools::default()
        },
    );
    assert_eq!(result.unwrap_err(), "cancelled");
    assert!(ready.is_file());
    assert!(collector.refreshed.is_none() && collector.network_refreshed.is_none());
}

/// A repeat request inside sysinfo's interval must not publish cached CPU percentages as fresh.
#[test]
fn immediate_sample_omits_cpu_without_postponing_the_next_interval() {
    let mut collector = Collector::new();
    let capture = |_: Request, _: &Arc<AtomicBool>| Tools::default();
    let request = Request { docker: false, processes: true };
    let cancel = Arc::new(AtomicBool::new(false));
    let first = collector.sample_with_devices(request, &cancel, capture).unwrap();
    assert!(first.cpu.is_none());
    // Place this call at the beginning of an interval even on a slow filesystem runner.
    let previous = Instant::now();
    collector.refreshed = Some(previous);
    let early = collector.sample_with_devices(request, &cancel, capture).unwrap();
    assert!(early.cpu.is_none() && early.summary.cpu_cores.iter().all(Option::is_none));
    assert!(early.processes.iter().all(|p| p.cpu.is_none()));
    assert_eq!(collector.refreshed, Some(previous));
}

/// A newly observed PID has no first CPU delta; an idle process later has a measured zero.
#[test]
fn new_process_needs_its_own_cpu_baseline() {
    if let Some(ready) = std::env::var_os(SAMPLER_CHILD_READY) {
        std::fs::write(ready, b"ready").unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    struct Sleeper(std::process::Child);
    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut collector = Collector::new();
    let mut sample = || {
        collector
            .sample_with_devices(
                Request { docker: false, processes: true },
                &Arc::new(AtomicBool::new(false)),
                |_, _| Tools::default(),
            )
            .unwrap()
    };
    let interval = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(30);
    sample();
    let dir = tempfile::tempdir().unwrap();
    let ready = dir.path().join("ready");
    let child = Sleeper(idle_child(&ready).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.is_file() {
        assert!(Instant::now() < deadline, "sampler child failed to start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let child_cpu = |s: &Snapshot| s.processes.iter().find(|p| p.pid == child.0.id()).unwrap().cpu;
    std::thread::sleep(interval);
    let first = sample();
    assert!(first.cpu.is_some());
    assert_eq!(child_cpu(&first), None);
    std::thread::sleep(interval);
    assert_eq!(child_cpu(&sample()), Some(0.0));
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
