use std::time::Duration;

use crate::session::{self, Failure, Open, Session, runtime};

#[test]
fn cancellation_wakes_trust_events_reads_and_queued_open() {
    runtime().block_on(async {
        let (owner, _worker) = Session::new();
        let read_owner = owner.clone();
        let read = tokio::spawn(async move { read_owner.read(false, 1024).await });
        let open_owner = owner.clone();
        let open = tokio::spawn(async move { open_owner.open(Open::Exec("true".into())).await });
        let event_owner = owner.clone();
        let event = tokio::spawn(async move { event_owner.event().await });
        owner.close();
        tokio::time::timeout(Duration::from_secs(2), async {
            assert_eq!(read.await.unwrap().unwrap_err(), Failure("CLOSED"));
            assert_eq!(open.await.unwrap().unwrap_err(), Failure("CLOSED"));
            assert_eq!(event.await.unwrap().unwrap_err(), Failure("CLOSED"));
        })
        .await
        .unwrap();
    });
}

#[test]
fn partial_reads_preserve_bytes_and_drain_before_exit() {
    runtime().block_on(async {
        let (owner, worker) = Session::new();
        worker.stdout.send(b"abcdef".to_vec()).await.unwrap();
        worker.stderr.send(b"error".to_vec()).await.unwrap();
        worker.outcome.send_replace(Some(Ok(7)));
        drop(worker);
        assert_eq!(owner.read(false, 2).await.unwrap(), b"ab");
        assert_eq!(owner.read(false, 8).await.unwrap(), b"cdef");
        assert_eq!(owner.read(true, 8).await.unwrap(), b"error");
        assert!(owner.read(false, 8).await.unwrap().is_empty());
        assert_eq!(owner.exit().await.unwrap(), 7);
    });
}

#[test]
fn closed_handle_is_rejected_without_accessing_memory() {
    session::close(-1);
    assert!(matches!(session::get(-1), Err(Failure("CLOSED"))));
}

#[test]
fn transport_failure_does_not_turn_into_successful_eof() {
    runtime().block_on(async {
        let (owner, worker) = Session::new();
        worker.stdout.send(b"last output".to_vec()).await.unwrap();
        worker.outcome.send_replace(Some(Err(Failure("NETWORK"))));
        drop(worker);
        assert_eq!(owner.read(false, 32).await.unwrap(), b"last output");
        assert_eq!(owner.read(false, 32).await.unwrap_err(), Failure("NETWORK"));
    });
}

#[test]
fn cancelling_key_exchange_closes_the_actual_socket() {
    use tokio::io::AsyncReadExt;
    use zeroize::Zeroizing;

    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let id = session::start(session::Options {
                host: "127.0.0.1".into(),
                port: listener.local_addr().unwrap().port(),
                user: "fixture".into(),
                password: Zeroizing::new(Vec::new()),
                fingerprint: String::new(),
            })
            .unwrap();
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut bytes = [0u8; 256];
            // Receive the client's banner but never send a server banner. This
            // leaves connect_stream pending, before a russh Handle is returned.
            assert!(peer.read(&mut bytes).await.unwrap() > 0);
            session::close(id);
            assert!(matches!(session::get(id), Err(Failure("CLOSED"))));
            loop {
                match peer.read(&mut bytes).await {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                    Err(error) => panic!("Unexpected loopback read failure: {error}"),
                }
            }
        })
        .await
        .expect("Cancellation left the KEX socket open");
    });
}
