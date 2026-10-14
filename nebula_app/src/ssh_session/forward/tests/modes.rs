use super::*;
use std::net::Ipv6Addr;

async fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap().local_addr().unwrap().port()
}

struct TcpEcho {
    port: u16,
    task: JoinHandle<()>,
}

impl TcpEcho {
    async fn new() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            let mut copies = JoinSet::new();
            loop {
                tokio::select! {
                    Ok((mut stream, _)) = listener.accept() => {
                        copies.spawn(async move {
                            let mut buffer = [0; 256];
                            while let Ok(count) = stream.read(&mut buffer).await {
                                if count == 0 { break; }
                                if stream.write_all(&buffer[..count]).await.is_err() { return; }
                            }
                            let _ = stream.shutdown().await;
                        });
                    },
                    Some(_) = copies.join_next(), if !copies.is_empty() => {},
                }
            }
        });
        Self { port, task }
    }
}

impl Drop for TcpEcho {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn exchange(mut stream: TcpStream) {
    stream.write_all(b"mixed forwarding payload").await.unwrap();
    stream.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"mixed forwarding payload");
}

async fn socks_request(port: u16, address: &[u8], target_port: u16) -> TcpStream {
    let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    socket.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0; 2];
    socket.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [5, 0]);
    socket.write_all(&[5, 1, 0]).await.unwrap();
    socket.write_all(address).await.unwrap();
    socket.write_u16(target_port).await.unwrap();
    let mut reply = [0; 10];
    socket.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[0..3], [5, 0, 0]);
    socket
}

#[test]
fn local_remote_dynamic_share_one_authenticated_transport_and_stop_independently() {
    check(async {
        let mut fixture = Fixture::new().await;
        let echo = TcpEcho::new().await;
        let local = bind_forward(fixture.session.clone(), 0, 3000).await.unwrap();
        let dynamic =
            bind_listener(fixture.session.clone(), ForwardKind::Dynamic, 0, 0).await.unwrap();
        let remote_port = free_port().await;
        let remote =
            remote::bind_forward(fixture.session.clone(), echo.port, remote_port).await.unwrap();
        assert_eq!(
            fixture.events.recv().await,
            Some(ForwardEvent::Requested(u32::from(remote_port)))
        );
        assert_eq!(remote.kind(), ForwardKind::Remote);
        assert_eq!(dynamic.kind(), ForwardKind::Dynamic);
        assert_ne!(local.id(), dynamic.id());
        exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, local.local_port())).await.unwrap())
            .await;
        exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, remote_port)).await.unwrap()).await;
        exchange(socks_request(dynamic.local_port(), b"\x03\x11host-only.invalid", 3333).await)
            .await;
        assert_eq!(fixture.targets.recv().await.as_deref(), Some("127.0.0.1"));
        assert_eq!(
            fixture.targets.recv().await.as_deref(),
            Some("host-only.invalid"),
            "SOCKS hostname must be sent to SSH without local DNS"
        );
        drop(remote);
        assert_eq!(
            fixture.events.recv().await,
            Some(ForwardEvent::Cancelled(u32::from(remote_port)))
        );
        while TcpStream::connect((Ipv4Addr::LOCALHOST, remote_port)).await.is_ok() {
            tokio::task::yield_now().await;
        }
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, remote_port)).await.is_err());
        exchange(socks_request(dynamic.local_port(), &[1, 192, 0, 2, 42], 3334).await).await;
        drop(dynamic);
        exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, local.local_port())).await.unwrap())
            .await;
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn pane_forwarder_uses_its_exact_transport_and_scope_close_stops_established_forwards() {
    check(async {
        let mut owner = Fixture::new().await;
        let mut other = Fixture::new().await;
        let (scope, forwarder) = Scope::new(&owner.session);
        let local = forwarder.open(ForwardKind::Local, 0, 3444).await.unwrap();
        exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, local.local_port())).await.unwrap())
            .await;
        assert_eq!(owner.opened.recv().await, Some(3444));
        assert!(other.opened.try_recv().is_err());
        let echo = TcpEcho::new().await;
        let remote_port = free_port().await;
        let remote = forwarder.open(ForwardKind::Remote, echo.port, remote_port).await.unwrap();
        assert!(local.is_active());
        assert!(remote.is_active());
        drop(scope);
        assert!(!local.is_active());
        assert!(!remote.is_active());
        assert!(forwarder.open(ForwardKind::Dynamic, 0, 0).await.is_err());
        while TcpStream::connect((Ipv4Addr::LOCALHOST, local.local_port())).await.is_ok() {
            tokio::task::yield_now().await;
        }
        while owner.events.recv().await != Some(ForwardEvent::Cancelled(u32::from(remote_port))) {}
        assert!(!owner.session.is_closed());
        assert!(!other.session.is_closed());
    });
}

#[test]
fn remote_rejects_unregistered_channels_conflicts_refusal_and_releases_active_copies() {
    check(async {
        let mut fixture = Fixture::new().await;
        let echo = TcpEcho::new().await;
        let port = free_port().await;
        assert!(
            fixture
                .server_handle
                .channel_open_forwarded_tcpip("127.0.0.1", u32::from(port), "127.0.0.1", 1234)
                .await
                .is_err()
        );
        let remote = remote::bind_forward(fixture.session.clone(), echo.port, port).await.unwrap();
        assert!(
            remote::bind_forward(fixture.session.clone(), echo.port, port).await.is_err(),
            "another pane cannot claim the same remote registration"
        );
        assert!(
            fixture
                .server_handle
                .channel_open_forwarded_tcpip("0.0.0.0", u32::from(port), "127.0.0.1", 1234)
                .await
                .is_err()
        );
        assert!(
            fixture
                .server_handle
                .channel_open_forwarded_tcpip("127.0.0.1", u32::from(port) + 1, "127.0.0.1", 1234)
                .await
                .is_err()
        );
        assert!(remote::bind_forward(fixture.session.clone(), echo.port, 1).await.is_err());
        let mut active = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        active.write_all(b"alive").await.unwrap();
        let mut bytes = [0; 5];
        active.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"alive");
        drop(remote);
        assert!(matches!(active.read(&mut [0]).await, Ok(0) | Err(_)));
        while fixture.events.recv().await != Some(ForwardEvent::Cancelled(u32::from(port))) {}
        while fixture.session.remote_forwards.is_reserved(port) {
            tokio::task::yield_now().await;
        }
        let reopened =
            remote::bind_forward(fixture.session.clone(), echo.port, port).await.unwrap();
        exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap()).await;
        drop(reopened);
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn cancelling_creation_before_global_reply_still_cancels_the_remote_listener() {
    check(async {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let mut fixture = Fixture::with_gate(Some(gate.clone())).await;
        let echo = TcpEcho::new().await;
        let port = free_port().await;
        let creation = tokio::spawn(remote::bind_forward(fixture.session.clone(), echo.port, port));
        assert_eq!(fixture.events.recv().await, Some(ForwardEvent::Requested(u32::from(port))));
        creation.abort();
        let _ = creation.await;
        gate.add_permits(1);
        assert_eq!(fixture.events.recv().await, Some(ForwardEvent::Cancelled(u32::from(port))));
        while fixture.session.remote_forwards.is_reserved(port) {
            tokio::task::yield_now().await;
        }
        assert!(!fixture.session.remote_forwards.is_reserved(port));
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn uncertain_remote_cancellation_cannot_reassign_an_old_listener_to_another_target() {
    check_with_budget(Duration::from_secs(30), async {
        for delayed in [false, true] {
            let gate = delayed.then(|| Arc::new(tokio::sync::Semaphore::new(0)));
            let mut fixture = Fixture::with_cancellation(None, gate.clone(), !delayed).await;
            let echo = TcpEcho::new().await;
            let replacement = TcpEcho::new().await;
            let port = free_port().await;
            let registry_owners = Arc::strong_count(&fixture.session.remote_forwards);
            let forward =
                remote::bind_forward(fixture.session.clone(), echo.port, port).await.unwrap();
            assert_eq!(fixture.events.recv().await, Some(ForwardEvent::Requested(u32::from(port))));
            exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap()).await;
            drop(forward);
            assert_eq!(fixture.events.recv().await, Some(ForwardEvent::Cancelled(u32::from(port))));
            // Wait for the real worker to finish its provider reply/20s budget.
            while Arc::strong_count(&fixture.session.remote_forwards) > registry_owners {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert!(
                fixture.session.remote_forwards.is_reserved(port),
                "uncertain port retains a disabled tombstone"
            );
            assert!(
                remote::bind_forward(fixture.session.clone(), replacement.port, port)
                    .await
                    .is_err()
            );
            assert!(
                fixture.events.try_recv().is_err(),
                "retry must not send a replacement request"
            );
            if let Some(gate) = gate {
                // The late wire ACK cannot silently reauthorize this transport's port.
                gate.add_permits(1);
                while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.is_ok() {
                    tokio::task::yield_now().await;
                }
                assert!(
                    remote::bind_forward(fixture.session.clone(), replacement.port, port)
                        .await
                        .is_err()
                );
            } else {
                let mut old_listener =
                    TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
                assert!(matches!(old_listener.read(&mut [0]).await, Ok(0) | Err(_)));
            }
            assert!(
                fixture
                    .server_handle
                    .channel_open_forwarded_tcpip("127.0.0.1", u32::from(port), "127.0.0.1", 1234)
                    .await
                    .is_err()
            );
            assert!(!fixture.session.is_closed());
        }
    });
}

#[test]
fn remote_channels_flow_while_registration_confirmation_is_pending() {
    check(async {
        let fixture = Fixture::new().await;
        let echo = TcpEcho::new().await;
        let port = free_port().await;
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (acknowledged, ack) = tokio::sync::oneshot::channel();
        let creation = tokio::spawn(remote::bind_delayed_confirmation(
            fixture.session.clone(),
            echo.port,
            port,
            acknowledged,
            gate.clone(),
        ));
        ack.await.unwrap();
        assert!(!creation.is_finished());
        // The real SSH registration exists, but its application confirmation
        // is gated to reproduce forwarded channels racing the global reply.
        tokio::time::timeout(
            Duration::from_secs(1),
            exchange(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap()),
        )
        .await
        .expect("channels must not wait for registration confirmation");
        assert!(!creation.is_finished());
        gate.add_permits(1);
        let forward = creation.await.unwrap().unwrap();
        drop(forward);
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn dynamic_handles_ipv6_and_rejects_auth_commands_addresses_and_channel_failure() {
    check(async {
        let mut fixture = Fixture::new().await;
        let forward =
            bind_listener(fixture.session.clone(), ForwardKind::Dynamic, 0, 0).await.unwrap();
        let port = forward.local_port();
        let mut address = vec![4];
        address.extend(Ipv6Addr::LOCALHOST.octets());
        exchange(socks_request(port, &address, 3333).await).await;
        assert_eq!(fixture.targets.recv().await.as_deref(), Some("::1"));
        let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        socket.write_all(&[5, 1, 2]).await.unwrap();
        let mut result = [0; 2];
        socket.read_exact(&mut result).await.unwrap();
        assert_eq!(result, [5, 255]);
        for (request, expected) in [
            (vec![5, 2, 0, 1], 7),
            (vec![5, 3, 0, 1], 7),
            (vec![5, 1, 0, 9], 8),
            (vec![5, 1, 0, 3, 0], 4),
            (vec![5, 1, 0, 3, 1, 0], 4),
            (vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 1], 1),
        ] {
            let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
            socket.write_all(&[5, 1, 0]).await.unwrap();
            socket.read_exact(&mut result).await.unwrap();
            socket.write_all(&request).await.unwrap();
            let mut reply = [0; 10];
            socket.read_exact(&mut reply).await.unwrap();
            assert_eq!(reply[1], expected);
        }
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn remote_caps_queued_and_active_channels_and_recovers_when_a_copy_ends() {
    check(async {
        let fixture = Fixture::new().await;
        let echo = TcpEcho::new().await;
        let port = free_port().await;
        let _forward =
            remote::bind_forward(fixture.session.clone(), echo.port, port).await.unwrap();
        let mut clients = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
            socket.write_all(b"x").await.unwrap();
            let mut byte = [0];
            socket.read_exact(&mut byte).await.unwrap();
            assert_eq!(&byte, b"x");
            clients.push(socket);
        }
        let mut excess = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        assert!(
            matches!(excess.read(&mut [0]).await, Ok(0) | Err(_)),
            "overflow server channel is rejected"
        );
        clients.pop().unwrap().shutdown().await.unwrap();
        while fixture.session.remote_forwards.available_slots(port) == 0 {
            tokio::task::yield_now().await;
        }
        let mut next = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        next.write_all(b"available").await.unwrap();
        let mut bytes = [0; 9];
        next.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"available");
        assert!(!fixture.session.is_closed());
    });
}

#[test]
fn dynamic_caps_connections_releases_clients_and_times_out_incomplete_handshakes() {
    check(async {
        let mut fixture = Fixture::new().await;
        let forward =
            bind_listener(fixture.session.clone(), ForwardKind::Dynamic, 0, 0).await.unwrap();
        let port = forward.local_port();
        let mut clients = Vec::new();
        for _ in 0..MAX_CONNECTIONS {
            clients.push(socks_request(port, &[1, 127, 0, 0, 1], 3333).await);
            assert_eq!(fixture.opened.recv().await, Some(3333));
        }
        let mut waiting = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        waiting.write_all(&[5, 1, 0]).await.unwrap();
        let mut reply = [0; 2];
        assert!(
            tokio::time::timeout(Duration::from_millis(20), waiting.read_exact(&mut reply))
                .await
                .is_err()
        );
        drop(clients.pop());
        waiting.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [5, 0]);
        waiting.write_all(&[5, 1, 0, 1, 127, 0, 0, 1, 13, 6]).await.unwrap();
        let mut accepted = [0; 10];
        waiting.read_exact(&mut accepted).await.unwrap();
        assert_eq!(accepted[1], 0);
        assert_eq!(fixture.opened.recv().await, Some(3334));
        drop(forward);
        assert!(matches!(waiting.read(&mut [0]).await, Ok(0) | Err(_)));
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        let stalled =
            bind_listener(fixture.session.clone(), ForwardKind::Dynamic, 0, 0).await.unwrap();
        let mut socket =
            TcpStream::connect((Ipv4Addr::LOCALHOST, stalled.local_port())).await.unwrap();
        socket.write_all(&[5, 1]).await.unwrap();
        assert!(
            matches!(socket.read(&mut [0]).await, Ok(0) | Err(_)),
            "incomplete negotiation expires within the handshake budget"
        );
        assert!(!fixture.session.is_closed());
    });
}
