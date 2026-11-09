//! Narrow HTTP bridge for native plugins whose host only exposes fetch.
//! No runtime control methods, CORS, redirects or credential endpoints.
use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;

use super::service::{Request, Service};

const MAX_HEADERS: usize = 8192;
const MAX_BODY: usize = 128 * 1024;

pub fn handle(
    stream: &mut TcpStream,
    first: &str,
    reader: &mut impl BufRead,
    service: &Service,
) -> io::Result<()> {
    let mut header_bytes = first.len();
    let mut authorized = false;
    let mut authorization_seen = false;
    let mut length = None;
    loop {
        let mut line = String::new();
        let read = reader.by_ref().take((MAX_HEADERS + 1) as u64).read_line(&mut line)?;
        header_bytes += read;
        if read == 0 || header_bytes > MAX_HEADERS || !line.ends_with("\r\n") {
            return reply(stream, 400, &serde_json::json!({"error":"invalid_headers"}));
        }
        if line == "\r\n" {
            break;
        }
        let Some((name, value)) = line.trim_end().split_once(':') else {
            return reply(stream, 400, &serde_json::json!({"error":"invalid_headers"}));
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            if authorization_seen {
                return reply(stream, 400, &serde_json::json!({"error":"duplicate_authorization"}));
            }
            authorization_seen = true;
            authorized = !service.token.is_empty()
                && value.strip_prefix("Bearer ") == Some(service.token.as_str());
        } else if name.eq_ignore_ascii_case("content-length") {
            let parsed = value.parse::<usize>().ok();
            if length.is_some() || parsed.is_none() {
                return reply(stream, 400, &serde_json::json!({"error":"invalid_length"}));
            }
            length = parsed;
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            || name.eq_ignore_ascii_case("origin")
        {
            return reply(stream, 400, &serde_json::json!({"error":"unsupported_headers"}));
        }
    }
    if !authorized {
        return reply(stream, 401, &serde_json::json!({"error":"unauthorized"}));
    }
    match first {
        "GET /v1/prompt-suggestions/config HTTP/1.1\r\n" if length.unwrap_or(0) == 0 => {
            reply(stream, 200, &service.config())
        },
        "POST /v1/prompt-suggestions HTTP/1.1\r\n" => {
            let Some(length) = length.filter(|length| *length <= MAX_BODY) else {
                return reply(stream, 413, &serde_json::json!({"error":"invalid_body_size"}));
            };
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            let request: Request = match serde_json::from_slice(&body) {
                Ok(request) => request,
                Err(_) => {
                    return reply(stream, 400, &serde_json::json!({"error":"invalid_request"}));
                },
            };
            if !request.valid() {
                return reply(stream, 400, &serde_json::json!({"error":"invalid_request"}));
            }
            reply(stream, 200, &service.predict(request))
        },
        _ => reply(stream, 404, &serde_json::json!({"error":"not_found"})),
    }
}

fn reply(stream: &mut TcpStream, status: u16, body: &impl serde::Serialize) -> io::Result<()> {
    let body = serde_json::to_vec(body).map_err(io::Error::other)?;
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Content Too Large",
        _ => "Bad Request",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::net::TcpListener;

    fn exchange(headers: &str, body: &str, token: bool) -> String {
        let service = Service::test_disabled();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        let auth = if token { service.token.clone() } else { "invalid".into() };
        // Headers and body arrive in one TCP write, exercising BufReader read-ahead.
        let wire = format!("{headers}Authorization: Bearer {auth}\r\n\r\n{body}");
        client.write_all(wire.as_bytes()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(server.try_clone().unwrap());
        let mut first = String::new();
        reader.read_line(&mut first).unwrap();
        handle(&mut server, &first, &mut reader, &service).unwrap();
        drop(reader);
        drop(server);
        let mut response = String::new();
        // Rejected bodies can cause a TCP reset after the complete response.
        let _ = client.read_to_string(&mut response);
        response
    }

    #[test]
    fn authenticated_config_and_buffered_body_use_the_narrow_routes() {
        let reply = exchange("GET /v1/prompt-suggestions/config HTTP/1.1\r\n", "", true);
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(reply.contains("Cache-Control: no-store"));
        assert!(reply.contains("fixture-model 正在预测"));
        let body = r#"{"agent":"claude","session":"s","turn":"t","revision":1,"messages":[{"role":"user","text":"继续测中文"}]}"#;
        let reply = exchange(
            &format!("POST /v1/prompt-suggestions HTTP/1.1\r\nContent-Length: {}\r\n", body.len()),
            body,
            true,
        );
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(reply.contains("\"suggestion\":null"));
    }

    #[test]
    fn authentication_smuggling_and_unsupported_routes_fail_closed() {
        for (headers, auth, status) in [
            ("GET /v1/prompt-suggestions/config HTTP/1.1\r\n", false, 401),
            ("POST /runtime/command HTTP/1.1\r\n", true, 404),
            (
                "GET /v1/prompt-suggestions/config HTTP/1.1\r\nOrigin: https://example.invalid\r\n",
                true,
                400,
            ),
            ("POST /v1/prompt-suggestions HTTP/1.1\r\nTransfer-Encoding: chunked\r\n", true, 400),
            (
                "POST /v1/prompt-suggestions HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n",
                true,
                400,
            ),
            ("POST /v1/prompt-suggestions HTTP/1.1\r\nContent-Length: 131073\r\n", true, 413),
        ] {
            assert!(
                exchange(headers, "", auth).starts_with(&format!("HTTP/1.1 {status}")),
                "{headers}"
            );
        }
    }
}
