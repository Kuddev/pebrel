use super::*;

fn resident(
    respond: impl FnOnce(&mut TcpStream, ApiRequest) + Send + 'static,
) -> (Endpoint, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let endpoint =
        Endpoint { port: listener.local_addr().unwrap().port(), token: "fixture".into() };
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
        let request: ApiRequest = serde_json::from_str(&line).unwrap();
        assert!(matches!(request.method.as_str(), "tab.new" | "window.create"));
        assert_eq!(request.params["cwd"], "/fixture/Chinese 空格");
        respond(&mut stream, request);
        // No connection for a replay may be queued, even after a lost response.
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    (endpoint, server)
}

#[test]
fn lost_reply_does_not_repeat_the_creation_or_allow_a_fallback() {
    for method in ["tab.new", "window.create"] {
        let (endpoint, server) = resident(|_, _| {});
        let mut attempts = 0;
        // Same contract as startup: only unavailable discovery/connect is retried.
        for _ in 0..40 {
            attempts += 1;
            if !matches!(
                exchange(
                    &endpoint,
                    method,
                    json!({"cwd":"/fixture/Chinese 空格"}),
                    Duration::from_secs(1)
                ),
                Outcome::Unavailable
            ) {
                break;
            }
        }
        assert_eq!(attempts, 1);
        server.join().unwrap();
    }
}

#[test]
fn slow_resident_and_mismatched_replies_are_not_retry_signals() {
    for mode in ["timeout", "wrong-id", "rejected", "truncated"] {
        let (endpoint, server) = resident(move |stream, request| {
            let mut reply = ApiResponse::success(request.id, json!({"tab_id": 7}));
            match mode {
                "timeout" => {
                    std::thread::sleep(Duration::from_millis(150));
                    return;
                },
                "wrong-id" => reply.id = "different-request".into(),
                "rejected" => {
                    reply =
                        ApiResponse::failure(reply.id, ApiError::new("timeout", "may finish later"))
                },
                "truncated" => {
                    stream.write_all(b"{\"ok\":true}").unwrap();
                    return;
                },
                _ => unreachable!(),
            }
            write_response(stream, &reply).unwrap();
        });
        assert!(matches!(
            exchange(
                &endpoint,
                "tab.new",
                json!({"cwd":"/fixture/Chinese 空格"}),
                Duration::from_millis(40)
            ),
            Outcome::Unconfirmed
        ));
        server.join().unwrap();
    }
}

#[test]
fn confirmed_reply_and_unavailable_resident_stay_distinct() {
    let (endpoint, server) = resident(|stream, request| {
        write_response(stream, &ApiResponse::success(request.id, json!({"tab_id": 7}))).unwrap();
    });
    assert!(matches!(
        exchange(
            &endpoint,
            "tab.new",
            json!({"cwd":"/fixture/Chinese 空格"}),
            Duration::from_secs(1)
        ),
        Outcome::Confirmed
    ));
    server.join().unwrap();
    assert!(matches!(
        exchange(&endpoint, "tab.new", json!({}), Duration::from_secs(1)),
        Outcome::Unavailable
    ));
}
