use base64::Engine as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::{Duration, Instant};

fn powershells() -> Vec<std::path::PathBuf> {
    vec![
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        std::env::var_os("PEBREL_TEST_PWSH").map(Into::into).unwrap_or_else(|| "pwsh.exe".into()),
    ]
}

fn powershell_command(shell: &std::path::Path, script: &str) -> Command {
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut command = Command::new(shell);
    command.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded]);
    command
}

#[test]
fn selected_http_proxy_reaches_real_powershell5_and_7_requests() {
    for shell in powershells() {
        for authenticated in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let worker = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    let (mut stream, _) = match listener.accept() {
                        Ok(stream) => stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "PowerShell never reached the proxy"
                            );
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        },
                        Err(error) => panic!("{error}"),
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        assert!(request.len() < 16 * 1024);
                        stream.read_exact(&mut byte).unwrap();
                        request.push(byte[0]);
                    }
                    let request = String::from_utf8(request).unwrap();
                    assert!(request.starts_with("GET http://pebrel-proxy.invalid/probe "));
                    // user:p:ss, encoded independently for the proxy challenge.
                    let authorized = request.lines().any(|line| {
                        line.split_once(':').is_some_and(|(name, value)| {
                            name.eq_ignore_ascii_case("Proxy-Authorization")
                                && value.trim() == "Basic dXNlcjpwOnNz"
                        })
                    });
                    if authenticated && !authorized {
                        stream.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=pebrel-test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                        continue;
                    }
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nok").unwrap();
                    return;
                }
            });
            let user = if authenticated { "user:p%3Ass@" } else { "" };
            let script = format!(
                "{}\n$ErrorActionPreference = 'Stop'\n(Invoke-WebRequest -UseBasicParsing -Uri 'http://pebrel-proxy.invalid/probe' -TimeoutSec 10).Content",
                include_str!("proxy.ps1")
            );
            let output = powershell_command(&shell, &script)
                .env("PEBREL_HTTP_PROXY", format!("http://{user}127.0.0.1:{port}"))
                .env_remove("HTTP_PROXY")
                .env_remove("HTTPS_PROXY")
                .env_remove("ALL_PROXY")
                .env_remove("NO_PROXY")
                .output()
                .unwrap_or_else(|error| panic!("{shell:?}: {error}"));
            let served = worker.join();
            assert!(
                output.status.success(),
                "{shell:?}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            served.unwrap();
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "ok", "{shell:?}");
        }
    }
}

#[test]
fn empty_http_marker_does_not_configure_a_socks_proxy_as_http() {
    for shell in powershells() {
        let script = format!(
            "{}\nif ($PSDefaultParameterValues.ContainsKey('Invoke-WebRequest:Proxy')) {{ exit 1 }}",
            include_str!("proxy.ps1")
        );
        let output = powershell_command(&shell, &script)
            .env("PEBREL_HTTP_PROXY", "")
            .env("http_proxy", "socks5://127.0.0.1:1080")
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell:?}: {:?}", output.stderr);
    }
}
