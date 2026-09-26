//! Native adapters use isolated endpoints; never claim a user's system agent.

use super::test_support::{Agent, Identity, check, key};
use super::*;

#[cfg(windows)]
#[test]
fn named_pipe_adapter_enumerates_identities_and_bounds_a_busy_pipe() {
    use tokio::net::windows::named_pipe::ServerOptions;

    check(async {
        let path =
            format!(r"\\.\pipe\pebrel-agent-test-{}-{}", std::process::id(), rand::random::<u64>());
        let server =
            ServerOptions::new().first_pipe_instance(true).max_instances(1).create(&path).unwrap();
        let identity = key();
        let agent = Agent::new(vec![Identity::plain(identity.clone())]);
        let service = tokio::spawn(async move {
            server.connect().await.unwrap();
            agent.serve(server).await;
        });
        let (connection, identities) =
            discover_connection(connect_pipe(&path), Duration::from_secs(2)).await.unwrap();
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].public_key().key_data(), identity.public_key().key_data());

        // Keep the only instance occupied. The upstream pipe connector retries
        // ERROR_PIPE_BUSY forever unless our discovery deadline covers connect.
        let budget = Duration::from_millis(150);
        let started = tokio::time::Instant::now();
        let error = match discover_connection(connect_pipe(&path), budget).await {
            Err(error) => error,
            Ok(_) => panic!("a second client must not connect to a busy single-instance pipe"),
        };
        assert!(error.to_string().contains("discovery timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(connection);
        service.await.unwrap();
    });
}

#[cfg(unix)]
#[test]
fn environment_adapter_uses_only_the_child_process_socket() {
    const CHILD: &str = "PEBREL_TEST_SSH_AGENT_CHILD";
    const TEST: &str =
        "platform::ssh_agent::tests::environment_adapter_uses_only_the_child_process_socket";

    // The child receives a private environment before it starts any threads.
    // No unsafe process-global set_var can affect concurrent credential tests.
    if std::env::var_os(CHILD).is_some() {
        check(async {
            let (_, identities) =
                discover_connection(connect(Endpoint::Environment), Duration::from_secs(2))
                    .await
                    .unwrap();
            assert_eq!(identities.len(), 1);
            assert!(identities[0].public_key().algorithm().is_ed25519());
        });
        return;
    }
    check(async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let service = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            Agent::new(vec![Identity::plain(key())]).serve(stream).await;
        });
        let output = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .env("SSH_AUTH_SOCK", &path)
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        service.await.unwrap();
    });
}

#[test]
fn candidates_are_platform_specific_and_unsupported_endpoints_fail_explicitly() {
    check(async {
        #[cfg(windows)]
        {
            assert_eq!(ENDPOINTS, &[Endpoint::OpenSsh, Endpoint::Pageant]);
            assert!(connect(Endpoint::Environment).await.is_err());
        }
        #[cfg(unix)]
        {
            assert_eq!(ENDPOINTS, &[Endpoint::Environment]);
            assert!(connect(Endpoint::OpenSsh).await.is_err());
            assert!(connect(Endpoint::Pageant).await.is_err());
        }
        #[cfg(not(any(windows, unix)))]
        assert!(ENDPOINTS.is_empty());
    });
}
