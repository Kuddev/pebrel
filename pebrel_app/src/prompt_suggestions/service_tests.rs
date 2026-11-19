use super::*;
use crate::ai_providers::{AiProvider, ProviderKind};
use std::io::{BufRead, BufReader, Read, Write};

fn request(turn: &str) -> Request {
    Request {
        agent: "claude".into(),
        session: "s".into(),
        turn: turn.into(),
        revision: 1,
        messages: vec![Message { role: "user".into(), text: "帮我修中文输入".into() }],
    }
}

#[test]
fn repeated_turns_and_provider_changes_never_publish_another_prediction() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut provider = AiProvider::preset(ProviderKind::Custom, "service-fixture");
    provider.enabled = true;
    provider.model = "fixture-model".into();
    provider.wire_api = "responses".into();
    provider.base_url = format!("http://{}", listener.local_addr().unwrap());
    let service = Service::test_snapshot(Some(PreparedClient::test_with_key(provider)));
    let revision = service.revision.clone();
    let server = std::thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["input"][0]["content"], "帮我修中文输入");
            if index == 1 {
                revision.fetch_add(1, Ordering::AcqRel);
            }
            let body = serde_json::json!({"status":"completed", "output":[{"type":"message",
                "content":[{"type":"output_text", "text":"{\"suggestion\":\"继续测中文\"}"}]}]})
            .to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    assert_eq!(service.predict(request("t1")).suggestion.as_deref(), Some("继续测中文"));
    let mut same = request("t1");
    same.revision = 2;
    assert!(service.predict(same).suggestion.is_none());
    assert!(
        service.predict(request("t2")).suggestion.is_none(),
        "settings change must reject an in-flight result"
    );
    server.join().unwrap();
}

#[test]
fn rejects_unknown_clients_tool_payloads_and_unbounded_identity() {
    assert!(request("t").valid());
    let mut value = request("t");
    value.agent = "shell".into();
    assert!(!value.valid());
    value.agent = "claude".into();
    value.messages[0].role = "tool".into();
    assert!(!value.valid());
    value.messages[0].role = "user".into();
    value.session = "x".repeat(257);
    assert!(!value.valid());
}
