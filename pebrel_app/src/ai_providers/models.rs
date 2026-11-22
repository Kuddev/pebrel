//! 供应商模型目录。子 Agent 只引用当前供应商的模型，不隐式改变凭据或线路。

use super::*;

pub fn parse_model_list(text: &str) -> Vec<String> {
    let mut models = Vec::new();
    for id in text.split(|c: char| c == ',' || c == '，' || c.is_whitespace()) {
        let id = clean_provider_field(id, false);
        if !id.is_empty() && !models.contains(&id) {
            models.push(id);
        }
    }
    models
}

pub fn configured_models(provider: &AiProvider) -> Vec<String> {
    let mut models = parse_model_list(&provider.models.join("\n"));
    if !provider.model.is_empty() && !models.contains(&provider.model) {
        models.insert(0, provider.model.clone());
    }
    models
}

pub fn fetch_models(
    provider: &AiProvider,
    draft_key: Option<Zeroizing<String>>,
) -> Result<Vec<String>, String> {
    let key = match draft_key {
        Some(key) => key,
        None => Zeroizing::new(
            String::from_utf8(
                load_api_key(&provider.id)
                    .map_err(|_| "Credential read failed")?
                    .unwrap_or_default(),
            )
            .map_err(|_| "Invalid credential encoding")?,
        ),
    };
    fetch_with_key(provider, &key)
}

fn fetch_with_key(provider: &AiProvider, key: &str) -> Result<Vec<String>, String> {
    if provider.full_url {
        return Err("Model discovery needs an API base URL".into());
    }
    if provider.kind.requires_api_key() && key.is_empty() {
        return Err("Enter an API key before fetching models".into());
    }
    let base = test_url(provider).map_err(|_| "Invalid API endpoint")?;
    let uri: ureq::http::Uri = base.parse().map_err(|_| "Invalid API endpoint")?;
    let host = uri.host().unwrap_or("").trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let started = Instant::now();
    let budget = Duration::from_secs(20);
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    let mut models = std::collections::BTreeSet::new();
    for _ in 0..32 {
        let remaining = budget.checked_sub(started.elapsed()).ok_or("Model discovery timed out")?;
        let config = ureq::config::Config::builder()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false);
        // 本地服务不能被全局 HTTP_PROXY 转发到外部，外部地址保留现有代理行为。
        let config = if loopback { config.proxy(None) } else { config };
        let agent: ureq::Agent = config.build().new_agent();
        let mut request = agent.get(&base);
        request = match provider.kind {
            ProviderKind::Anthropic => {
                let request = request
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01")
                    .query("limit", "1000");
                if let Some(cursor) = &cursor { request.query("after_id", cursor) } else { request }
            },
            ProviderKind::Google => {
                let request = request.header("x-goog-api-key", key).query("pageSize", "1000");
                if let Some(cursor) = &cursor {
                    request.query("pageToken", cursor)
                } else {
                    request
                }
            },
            ProviderKind::AzureOpenAi => request.header("api-key", key),
            _ if key.is_empty() => request,
            _ => request.header("Authorization", &format!("Bearer {key}")),
        };
        let mut response = request.call().map_err(|_| "Model request failed")?;
        if !response.status().is_success() {
            return Err(format!("Model request: HTTP {}", response.status().as_u16()));
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_to_string()
            .map_err(|_| "Invalid model response")?;
        models.extend(parse_models_response(&text, provider.kind)?);
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| "Invalid model JSON")?;
        cursor = match provider.kind {
            ProviderKind::Google => value
                .get("nextPageToken")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_owned),
            ProviderKind::Anthropic
                if value.get("has_more").and_then(|v| v.as_bool()) == Some(true) =>
            {
                Some(
                    value
                        .get("last_id")
                        .and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty())
                        .ok_or("Missing model page cursor")?
                        .to_owned(),
                )
            },
            _ => None,
        };
        match &cursor {
            None => return Ok(models.into_iter().collect()),
            Some(cursor) if !seen.insert(cursor.clone()) => {
                return Err("Provider repeated a model page cursor".into());
            },
            _ => {},
        }
    }
    Err("Provider model catalog exceeded 32 pages".into())
}

fn parse_models_response(text: &str, kind: ProviderKind) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| "Invalid model JSON")?;
    let field = if kind == ProviderKind::Google { "models" } else { "data" };
    let rows = value.get(field).and_then(|v| v.as_array()).ok_or("Missing model list")?;
    let mut models = Vec::new();
    for row in rows {
        let id = if kind == ProviderKind::Google {
            row.get("name").and_then(|v| v.as_str()).map(|v| v.strip_prefix("models/").unwrap_or(v))
        } else {
            row.get("id").and_then(|v| v.as_str())
        };
        if let Some(id) = id.filter(|id| !id.is_empty() && id.len() <= 512)
            && !models.iter().any(|model| model == id)
        {
            models.push(id.to_owned());
        }
    }
    models.sort();
    if models.is_empty() {
        return Err("The provider returned no models; enter model IDs manually".into());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serve(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "request was not sent");
                            std::thread::sleep(Duration::from_millis(5));
                        },
                        Err(error) => panic!("{error}"),
                    }
                };
                // Windows accepts inherit the listener's nonblocking mode.
                // Header reads below use a blocking stream with a bounded timeout.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 16384);
                }
                requests.push(String::from_utf8(bytes).unwrap());
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
            requests
        });
        (base, worker)
    }

    #[test]
    fn actual_http_uses_base_url_and_api_key_for_each_protocol_and_paginates() {
        for (kind, responses, header, page) in [
            (
                ProviderKind::OpenAi,
                vec![r#"{"data":[{"id":"first"}]}"#],
                "authorization: bearer fixture-key",
                "",
            ),
            (
                ProviderKind::Anthropic,
                vec![
                    r#"{"data":[{"id":"first"}],"has_more":true,"last_id":"first"}"#,
                    r#"{"data":[{"id":"second"}],"has_more":false}"#,
                ],
                "x-api-key: fixture-key",
                "after_id=first",
            ),
            (
                ProviderKind::Google,
                vec![
                    r#"{"models":[{"name":"models/first"}],"nextPageToken":"next"}"#,
                    r#"{"models":[{"name":"models/second"}]}"#,
                ],
                "x-goog-api-key: fixture-key",
                "pageToken=next",
            ),
        ] {
            let pages = responses.len();
            let (base, worker) = serve(responses);
            let mut provider = AiProvider::preset(kind, "http-fixture");
            provider.base_url = base;
            let models = fetch_with_key(&provider, "fixture-key").unwrap();
            assert_eq!(models.len(), pages);
            let requests = worker.join().unwrap();
            assert!(
                requests
                    .iter()
                    .all(|r| r.starts_with("GET /v1/models")
                        && r.to_ascii_lowercase().contains(header)),
                "{kind:?}"
            );
            if pages > 1 {
                assert!(requests[1].contains(page));
            }
        }
    }

    #[test]
    fn repeated_cursor_fails_instead_of_returning_an_incomplete_catalog() {
        let (base, worker) =
            serve(vec![r#"{"models":[{"name":"models/first"}],"nextPageToken":"same"}"#; 2]);
        let mut provider = AiProvider::preset(ProviderKind::Google, "cursor-fixture");
        provider.base_url = base;
        assert!(fetch_with_key(&provider, "fixture-key").unwrap_err().contains("repeated"));
        assert_eq!(worker.join().unwrap().len(), 2);
    }

    #[test]
    fn model_lists_preserve_identity_and_only_include_this_provider() {
        let mut provider = AiProvider::preset(ProviderKind::Custom, "one");
        provider.model = "vendor/model".into();
        provider.models = parse_model_list("other-model, vendor/model\nother-model，last");
        assert_eq!(configured_models(&provider), ["other-model", "vendor/model", "last"]);
        let other = AiProvider::preset(ProviderKind::OpenAi, "two");
        assert!(!configured_models(&provider).contains(&other.model));
    }

    #[test]
    fn model_responses_use_protocol_fields_without_rewriting_ids() {
        assert_eq!(
            parse_models_response(
                r#"{"data":[{"id":"a/b"},{"id":"a/b"},{"id":"c"}]}"#,
                ProviderKind::OpenAi
            )
            .unwrap(),
            ["a/b", "c"]
        );
        assert_eq!(
            parse_models_response(
                r#"{"models":[{"name":"models/gemini-test"}]}"#,
                ProviderKind::Google
            )
            .unwrap(),
            ["gemini-test"]
        );
        assert!(parse_models_response(r#"{"error":"bad"}"#, ProviderKind::Custom).is_err());
    }
}
