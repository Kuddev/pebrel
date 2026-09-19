//! Local HTTP/CONNECT fixtures for the updater's actual request paths.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) struct Server {
    pub(crate) address: SocketAddr,
    worker: Option<JoinHandle<Vec<(String, String)>>>,
}

impl Server {
    pub(crate) fn start(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "fixture received no connection");
                            thread::sleep(Duration::from_millis(5));
                        },
                        Err(error) => panic!("fixture accept failed: {error}"),
                    }
                };
                // Windows accepted sockets inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut reader = BufReader::new(stream);
                let (connect, request) = if reader.fill_buf().unwrap().first() == Some(&5) {
                    (socks_connect(&mut reader), read_headers(&mut reader))
                } else {
                    let first = read_headers(&mut reader);
                    if first.starts_with("CONNECT ") {
                        reader
                            .get_mut()
                            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                            .unwrap();
                        (first, read_headers(&mut reader))
                    } else {
                        (String::new(), first)
                    }
                };
                requests.push((connect, request));
                reader.get_mut().write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        Self { address, worker: Some(worker) }
    }

    pub(crate) fn agent(&self, exclusions: &[&str]) -> ureq::Agent {
        self.agent_for_server(&self.address.to_string(), exclusions)
    }

    pub(crate) fn socks_agent(&self) -> ureq::Agent {
        self.agent_for_server(&format!("socks=socks5h://{}", self.address), &[])
    }

    fn agent_for_server(&self, server: &str, exclusions: &[&str]) -> ureq::Agent {
        // Go through the same system-value parser and fallback policy as production.
        let url = super::parse_windows_proxy_server(server, "http").unwrap();
        let proxy = super::resolve_with_system(
            Some((url, exclusions.iter().map(|entry| (*entry).to_owned()).collect())),
            || panic!("system proxy should win"),
        );
        ureq::config::Config::builder()
            .proxy(proxy)
            .timeout_global(Some(Duration::from_secs(4)))
            .build()
            .new_agent()
    }

    pub(crate) fn finish(mut self) -> Vec<(String, String)> {
        self.worker.take().unwrap().join().expect("HTTP fixture failed")
    }
}

fn socks_connect(reader: &mut BufReader<TcpStream>) -> String {
    let mut greeting = [0; 2];
    reader.read_exact(&mut greeting).unwrap();
    assert_eq!(greeting[0], 5);
    let mut methods = vec![0; greeting[1] as usize];
    reader.read_exact(&mut methods).unwrap();
    assert!(methods.contains(&0));
    reader.get_mut().write_all(&[5, 0]).unwrap();
    let mut request = [0; 5];
    reader.read_exact(&mut request).unwrap();
    assert_eq!(&request[..4], &[5, 1, 0, 3], "SOCKS5h must send the hostname to the proxy");
    let mut host = vec![0; request[4] as usize];
    reader.read_exact(&mut host).unwrap();
    let mut port = [0; 2];
    reader.read_exact(&mut port).unwrap();
    reader.get_mut().write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).unwrap();
    format!("SOCKS5 {}:{}", String::from_utf8(host).unwrap(), u16::from_be_bytes(port))
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            // Even a failing test retires its bounded fixture thread.
            let _ = worker.join();
        }
    }
}

fn read_headers(reader: &mut BufReader<TcpStream>) -> String {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "unexpected fixture EOF");
        if line == "\r\n" {
            return headers;
        }
        headers.push_str(&line);
        assert!(headers.len() < 16 * 1024);
    }
}

pub(crate) fn response(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}
