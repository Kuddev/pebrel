use super::*;
use std::io::{Read, Write};

fn provider(wire: &str) -> AiProvider {
    let mut provider = AiProvider::preset(ProviderKind::Custom, "prediction-test");
    provider.enabled = true;
    provider.model = "gpt-6.1-sol".into();
    provider.base_url = "https://example.invalid/v1".into();
    provider.wire_api = wire.into();
    provider
}

#[test]
fn partial_refused_and_ambiguous_responses_never_become_drafts() {
    let provider = provider("responses");
    let text = r#"{"suggestion":"跑一下中文输入测试"}"#;
    let mut value = json!({"status":"completed","output":[
        {"type":"reasoning","summary":[]},
        {"type":"message","content":[{"type":"output_text","text":text}]}
    ]});
    assert_eq!(response_text(&provider, &value).as_deref(), Some(text));
    value["status"] = json!("incomplete");
    assert!(response_text(&provider, &value).is_none());
    value["status"] = json!("completed");
    value["output"][1]["content"][0]["type"] = json!("refusal");
    assert!(response_text(&provider, &value).is_none());
    let chat = provider_for_chat();
    assert!(
        response_text(
            &chat,
            &json!({"choices":[{"finish_reason":"length",
        "message":{"content":text}}]})
        )
        .is_none()
    );
}

fn provider_for_chat() -> AiProvider {
    provider("chat")
}

#[test]
fn requests_are_short_independent_and_contain_user_voice() {
    let messages = [Message { role: "user".into(), text: "先把光标测完".into() }];
    let responses = provider("responses");
    let (url, body) = payload(&responses, "zh-CN", &messages).unwrap();
    assert_eq!(url, "https://example.invalid/v1/responses");
    assert_eq!(body["store"], false);
    assert!(body.get("previous_response_id").is_none());
    assert!(body.get("tools").is_none());
    let input: Value = serde_json::from_str(body["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["conversation"][0]["text"], "先把光标测完");
    assert_eq!(input["language"], "zh-CN");
    assert_eq!(input["user_voice_examples"], json!(["先把光标测完"]));
    let mut chat = provider_for_chat();
    chat.model = "local-small-model".into();
    let (_, body) = payload(&chat, "en", &messages).unwrap();
    assert_eq!(body["max_tokens"], 512);
    assert!(body.get("reasoning_effort").is_none());
    chat.full_url = true;
    chat.base_url = "https://example.invalid/custom-route".into();
    assert_eq!(payload(&chat, "en", &messages).unwrap().0, chat.base_url);
}

#[test]
fn real_http_path_accepts_only_a_complete_result_and_never_follows_redirects() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut target = provider("responses");
    target.base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for redirect in [false, true] {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 1024];
            loop {
                let count = stream.read(&mut buf).unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&buf[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = json!({"status":"completed","output":[{"type":"message",
                "content":[{"type":"output_text","text":"{\"suggestion\":\"先测中文输入\"}"}]}]})
            .to_string();
            let status = if redirect {
                "307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/leak"
            } else {
                "200 OK"
            };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let messages = [Message { role: "user".into(), text: "帮我修中文输入".into() }];
    let cancelled = AtomicBool::new(false);
    // Exercise the reusable production entrypoint; no provider file or keychain
    // is required after the snapshot has been prepared.
    let client =
        PreparedClient { provider: target, key: Zeroizing::new(b"synthetic-test-key".to_vec()) };
    assert_eq!(
        client.request("zh-CN", &messages, &cancelled).unwrap().as_deref(),
        Some("先测中文输入")
    );
    assert_eq!(client.request("zh-CN", &messages, &cancelled), Err(Failure::Network));
    server.join().unwrap();
    cancelled.store(true, Ordering::Release);
    assert_eq!(client.request("zh-CN", &messages, &cancelled), Err(Failure::Cancelled));
}

/// Explicitly opted-in, paid evaluation. Uses synthetic fixtures only; never loads CLI history.
#[test]
#[ignore = "requires a private evaluation directory and an explicitly authorized model request"]
fn live_empty_prompt_voice_evaluation() {
    let root = std::path::PathBuf::from(
        std::env::var_os("PEBREL_PROMPT_EVAL_DIR").expect("evaluation directory"),
    );
    let config: Value =
        serde_json::from_slice(&std::fs::read(root.join("model-config.json")).unwrap()).unwrap();
    let key = Zeroizing::new(std::fs::read_to_string(root.join("api-key")).unwrap());
    // Private fixtures permit multi-turn voice evaluations without rebuilding
    // the product or checking synthetic role-play scenarios into the app.
    let cases = match std::fs::read_to_string(root.join("evaluation.json")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            include_str!("evaluation.json").to_owned()
        },
        Err(error) => panic!("cannot read evaluation fixtures: {error}"),
    };
    let cases: Value = serde_json::from_str(&cases).unwrap();
    let mut target = provider("responses");
    target.base_url = config["base_url"].as_str().unwrap().into();
    target.model = config["model"].as_str().unwrap().into();
    let client =
        PreparedClient { provider: target, key: Zeroizing::new(key.trim().as_bytes().to_vec()) };
    let mut results = Vec::new();
    for case in cases.as_array().unwrap() {
        let messages: Vec<_> = case["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| Message {
                role: m["role"].as_str().unwrap().into(),
                text: m["text"].as_str().unwrap().into(),
            })
            .collect();
        let started = std::time::Instant::now();
        let response =
            client.request(case["language"].as_str().unwrap(), &messages, &AtomicBool::new(false));
        let error = response.as_ref().err().copied();
        results.push(json!({"id":case["id"],"suggestion":response.ok().flatten(),"error":error,"elapsed_ms":started.elapsed().as_millis()}));
    }
    std::fs::write(
        root.join("empty-prompt-results.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    // Semantic voice/intent quality is reviewed from this artifact, not asserted by a keyword test.
    assert!(
        results.iter().all(|r| r["error"].is_null()),
        "request failures; inspect private results"
    );
    assert!(
        results.iter().filter(|r| r["suggestion"].is_string()).count() >= 4,
        "too few complete predictions; inspect private results"
    );
}
