//! Mobile's first real desktop transport: one authenticated SSH exec channel.
//!
//! Session ownership and command validation remain in RuntimeHub. This adapter
//! fixes the local endpoint for its entire lifetime, hides its token, bounds wire
//! messages, and exposes a small allowlist. No new public listener or cloud is
//! involved. Device pairing, an input lease, durable notifications and a standalone
//! relay are separate capabilities; this transport does not claim to provide them.

use super::*;
use std::io;
use std::sync::atomic::AtomicBool;

const MAX_BRIDGE_FRAME: usize = 2 * 1024 * 1024;
const MAX_BRIDGE_REQUEST: usize = 40 * 1024;

fn bridge_policy() -> &'static Value {
    static POLICY: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("../../../mobile/protocol/bridge-policy.json"))
            .expect("valid embedded mobile bridge policy")
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    method: String,
    #[serde(default = "empty_object")]
    params: Value,
}

impl Request {
    fn validate(&self, allow_input: bool) -> Result<(), ApiError> {
        if self.id.is_empty() || self.id.len() > 80 || self.id.chars().any(char::is_control) {
            return Err(ApiError::invalid_params("id must contain 1..80 bytes without controls"));
        }
        if !self.params.is_object() {
            return Err(ApiError::invalid_params("params must be an object"));
        }
        let listed = |group: &str| {
            bridge_policy()[group]
                .as_array()
                .expect("method array")
                .iter()
                .any(|method| method.as_str() == Some(self.method.as_str()))
        };
        if !listed("read") {
            if !listed("input") {
                return Err(ApiError::new("method_not_found", "method is not available on mobile"));
            }
            if !allow_input {
                return Err(ApiError::new("input_not_authorized", "this channel is view-only"));
            }
        }
        if self.method.starts_with("pane.") {
            for key in ["window_id", "pane_id"] {
                if self.params.get(key).and_then(Value::as_u64).is_none_or(|id| id == 0) {
                    return Err(ApiError::invalid_params(format!(
                        "{key} must be explicit and nonzero"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Read one bounded line without allocating in proportion to untrusted input.
fn read_frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::with_capacity(1024.min(limit));
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "incomplete frame"))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |index| index + 1);
        if bytes.len() + count > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds limit"));
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(bytes));
        }
    }
}

fn connect(endpoint: &Endpoint, request: &ApiRequest) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect_timeout(&endpoint_addr(endpoint), CONNECT_TIMEOUT)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(COMMAND_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    write_json_line(&mut stream, request)?;
    stream.shutdown(Shutdown::Write)?;
    Ok(stream)
}

type Output = Arc<dyn Fn(Vec<u8>) -> io::Result<()> + Send + Sync>;

fn write_frame(output: &Output, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() >= MAX_BRIDGE_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "response exceeds limit"));
    }
    bytes.push(b'\n');
    output(bytes)
}

struct Subscription {
    shutdown: TcpStream,
    thread: std::thread::JoinHandle<()>,
}

impl Subscription {
    fn stop(self) {
        let _ = self.shutdown.shutdown(Shutdown::Both);
        // A hostile/paused SSH peer can stop reading stdout indefinitely. Do not
        // join its blocked writer. This CLI process owns no desktop session;
        // exiting closes its remaining I/O and worker with the process.
        drop(self.thread);
    }
}

fn start_subscription(
    endpoint: &Endpoint,
    request: Request,
    output: Output,
    stopped: Arc<AtomicBool>,
) -> io::Result<Subscription> {
    let mut local = ApiRequest::new(endpoint.token.clone(), request.method, request.params);
    local.id = request.id;
    let stream = connect(endpoint, &local)?;
    let shutdown = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let first = read_frame(&mut reader, MAX_BRIDGE_FRAME)?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "missing subscription response")
    })?;
    let response: ApiResponse = serde_json::from_slice(&first).map_err(io::Error::other)?;
    write_frame(&output, &response)?;
    if !response.ok {
        return Err(io::Error::other("runtime rejected subscription"));
    }
    reader.get_mut().set_read_timeout(None)?;
    let thread =
        std::thread::Builder::new().name("pebrel-mobile-events".into()).spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok(Some(frame)) = read_frame(&mut reader, MAX_BRIDGE_FRAME) else { break };
                let Ok(event) = serde_json::from_slice::<Value>(&frame) else { break };
                if write_frame(&output, &event).is_err() {
                    break;
                }
            }
            if !stopped.swap(true, Ordering::AcqRel) {
                let _ = write_frame(&output, &json!({"type":"mobile.disconnected"}));
            }
        })?;
    Ok(Subscription { shutdown, thread })
}

pub(crate) fn run(allow_input: bool) -> Result<(), Box<dyn Error>> {
    let stdout = Mutex::new(io::stdout());
    let output: Output = Arc::new(move |bytes| {
        let mut stdout = stdout.lock().map_err(|_| io::Error::other("output lock poisoned"))?;
        stdout.write_all(&bytes)?;
        stdout.flush()
    });
    let mut session = BridgeSession::open(allow_input, output)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    while !session.stopped.load(Ordering::Acquire) {
        let Some(frame) = read_frame(&mut input, MAX_BRIDGE_REQUEST)? else { break };
        session.request(&frame)?;
    }
    Ok(())
}

/// The native settings transport and SSH CLI share this exact authorization,
/// endpoint pinning and subscription owner; networking never forks RPC policy.
pub(crate) struct BridgeSession {
    endpoint: Endpoint,
    output: Output,
    stopped: Arc<AtomicBool>,
    subscription: Option<Subscription>,
    allow_input: bool,
    screen: super::mobile_screen::ScreenBaseline,
}

impl BridgeSession {
    pub(crate) fn open(allow_input: bool, output: Output) -> io::Result<Self> {
        let endpoint =
            read_endpoint().ok_or_else(|| io::Error::other("no resident Pebrel runtime"))?;
        let stopped = Arc::new(AtomicBool::new(false));
        write_frame(
            &output,
            &json!({
                "type": "mobile.ready", "protocol": "pebrel.mobile.ssh", "version": 1,
                "capabilities": {
                    "snapshot": true, "read_tail": true, "state_subscription": true,
                    "input": allow_input, "exclusive_input": false, "replay_notifications": false,
                    "terminal_grid_stream": false, "screen_delta": true
                },
                "max_request_bytes": MAX_BRIDGE_REQUEST,
                "max_frame_bytes": MAX_BRIDGE_FRAME
            }),
        )?;
        Ok(Self {
            endpoint,
            output,
            stopped,
            subscription: None,
            allow_input,
            screen: Default::default(),
        })
    }

    pub(crate) fn request(&mut self, frame: &[u8]) -> io::Result<()> {
        if self.stopped.load(Ordering::Acquire) || frame.len() > MAX_BRIDGE_REQUEST {
            return Err(io::Error::other("mobile_connection_closed"));
        }
        let Self { endpoint, output, stopped, subscription, allow_input, screen } = self;
        let request: Request = match serde_json::from_slice(&frame) {
            Ok(request) => request,
            Err(_) => {
                write_frame(
                    &output,
                    &ApiResponse::failure(
                        "invalid",
                        ApiError::new("invalid_request", "invalid mobile request"),
                    ),
                )?;
                return Ok(());
            },
        };
        if let Err(error) = request.validate(*allow_input) {
            write_frame(&output, &ApiResponse::failure(request.id, error))?;
            return Ok(());
        }
        if request.method == "events.subscribe" {
            if subscription.is_some() {
                write_frame(
                    &output,
                    &ApiResponse::failure(
                        request.id,
                        ApiError::new("already_subscribed", "one state subscription per channel"),
                    ),
                )?;
            } else {
                *subscription =
                    Some(start_subscription(endpoint, request, output.clone(), stopped.clone())?);
            }
            return Ok(());
        }
        let mut local = ApiRequest::new(endpoint.token.clone(), request.method, request.params);
        local.id = request.id.clone();
        // This extension belongs to the link, not the resident Runtime API.
        let baseline = if local.method == "pane.read" && local.params["screen"] == true {
            local
                .params
                .as_object_mut()
                .and_then(|params| params.remove("screen_since"))
                .and_then(|value| value.as_u64())
        } else {
            None
        };
        // Never rediscover the endpoint mid-channel: a runtime replacement
        // must cause disconnect, not retarget a queued prompt to a new pane.
        let response = (|| -> io::Result<ApiResponse> {
            let mut reader = BufReader::new(connect(&endpoint, &local)?);
            let frame = read_frame(&mut reader, MAX_BRIDGE_FRAME)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing response"))?;
            serde_json::from_slice(&frame).map_err(io::Error::other)
        })();
        match response {
            Ok(mut response) => {
                if response.ok
                    && let (Some(result), Some(sequence)) = (&mut response.result, baseline)
                {
                    screen.encode(result, sequence);
                }
                write_frame(&output, &response)?;
            },
            Err(_) => {
                write_frame(
                    &output,
                    &ApiResponse::failure(
                        request.id,
                        ApiError::new(
                            "runtime_connection_lost",
                            "delivery may be unknown; do not replay input",
                        ),
                    ),
                )?;
                stopped.store(true, Ordering::Release);
                return Err(io::Error::other("runtime_connection_lost"));
            },
        }
        Ok(())
    }
}

impl Drop for BridgeSession {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(subscription) = self.subscription.take() {
            subscription.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_and_relay_share_wire_budgets() {
        assert_eq!(bridge_policy()["maxFrameBytes"].as_u64(), Some(MAX_BRIDGE_FRAME as u64));
        assert_eq!(bridge_policy()["maxRequestBytes"].as_u64(), Some(MAX_BRIDGE_REQUEST as u64));
    }

    #[test]
    fn readonly_default_denies_writes_and_unlisted_methods() {
        for method in
            ["pane.prompt", "pane.send_key", "pane.exec", "window.close", "runtime.orchestrate"]
        {
            let request = Request {
                id: "1".into(),
                method: method.into(),
                params: json!({"window_id":1,"pane_id":2}),
            };
            assert!(request.validate(false).is_err(), "{method}");
        }
    }

    #[test]
    fn explicit_input_still_requires_complete_target_and_allowlist() {
        let mut request =
            Request { id: "2".into(), method: "pane.prompt".into(), params: json!({"pane_id":2}) };
        assert!(request.validate(true).is_err());
        request.params["window_id"] = json!(1);
        assert!(request.validate(true).is_ok());
        request.method = "pane.exec".into();
        assert!(request.validate(true).is_err());
        request.method = "pane.read".into();
        assert!(request.validate(false).is_ok());
    }

    #[test]
    fn frames_are_bounded_and_truncation_is_not_a_valid_request() {
        let mut input = io::Cursor::new(b"abc\nnext\n");
        assert_eq!(read_frame(&mut input, 4).unwrap().unwrap(), b"abc\n");
        assert!(read_frame(&mut input, 4).is_err());
        assert!(read_frame(&mut io::Cursor::new(b"abc"), 8).is_err());
        assert!(read_frame(&mut io::Cursor::new(b""), 8).unwrap().is_none());
    }

    #[test]
    fn caller_cannot_supply_the_local_runtime_token() {
        assert!(
            serde_json::from_value::<Request>(json!({
                "id":"1", "method":"runtime.snapshot", "token":"attacker"
            }))
            .is_err()
        );
    }
}
