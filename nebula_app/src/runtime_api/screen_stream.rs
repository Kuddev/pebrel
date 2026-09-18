//! Change-driven physical-screen stream. Runtime owns capture and identity;
//! relays forward ciphertext without polling or interpreting terminals.
use super::*;
use std::io;
mod flow;
#[cfg(all(test, feature = "gpui-shell"))]
mod tests;
pub(crate) use flow::Registry;

const MAX_FRAME: usize = 2 * 1024 * 1024 - 1024;
const BURST_INTERVAL: Duration = Duration::from_millis(33);
const HEARTBEAT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Subscribe {
    window_id: u64,
    pane_id: u64,
    #[serde(default = "default_lines")]
    lines: usize,
}
fn default_lines() -> usize {
    100
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    window_id: u64,
    pane_id: u64,
    subscription_id: u64,
    #[serde(default)]
    sequence: Option<u64>,
}

pub(super) fn control(
    stream: &mut TcpStream,
    request: ApiRequest,
    hub: &RuntimeHub,
) -> io::Result<()> {
    let result = (|| {
        let params: Control = parse_params(&request.params)?;
        if (request.method == "pane.screen.ack") != params.sequence.is_some()
            || (request.method == "pane.screen.unsubscribe"
                && request.params.get("sequence").is_some())
            || !hub.screens.control(
                params.subscription_id,
                (params.window_id, params.pane_id),
                params.sequence,
            )
        {
            return Err(ApiError::invalid_params("unknown stream, target or acknowledgement"));
        }
        Ok(json!({"accepted":true}))
    })();
    let response = match result {
        Ok(value) => ApiResponse::success(request.id, value),
        Err(error) => ApiResponse::failure(request.id, error),
    };
    write_response(stream, &response)
}

pub(super) fn subscribe(
    stream: &mut TcpStream,
    request: ApiRequest,
    sink: &EventSink,
    hub: &RuntimeHub,
) -> io::Result<()> {
    if !cfg!(feature = "gpui-shell") {
        return write_response(
            stream,
            &ApiResponse::failure(
                request.id,
                ApiError::new("unsupported_method", "screen streams require the GPUI runtime"),
            ),
        );
    }
    let params: Subscribe = match parse_params::<Subscribe>(&request.params) {
        Ok(params)
            if params.window_id != 0
                && params.pane_id != 0
                && (1..=100).contains(&params.lines) =>
        {
            params
        },
        _ => {
            return write_response(
                stream,
                &ApiResponse::failure(
                    request.id,
                    ApiError::invalid_params("explicit target and 1..100 lines required"),
                ),
            );
        },
    };
    let sub = hub.screens.register((params.window_id, params.pane_id));
    write_response(stream, &ApiResponse::success(request.id, json!({"subscription_id":sub.id})))?;
    let mut baseline = mobile_screen::ScreenBaseline::default();
    let mut previous: Option<Value> = None;
    let mut since = 0;
    let mut next_frame = Instant::now();
    let mut heartbeat = Instant::now() + HEARTBEAT;
    let mut last_progress = Instant::now();
    loop {
        if sub.watch.closed() {
            return Ok(());
        }
        let now = Instant::now();
        if now >= next_frame && sub.watch.take_dirty() {
            next_frame = now + BURST_INTERVAL;
            let captured = dispatch_runtime_command(
                RuntimeCommand::ReadPane {
                    window_id: Some(params.window_id),
                    pane_id: params.pane_id,
                    lines: params.lines,
                    screen: true,
                },
                sink,
                hub,
            );
            let mut data = match captured {
                Ok(data) => data,
                Err(error) => {
                    write_event(
                        stream,
                        &json!({"event":"pane.screen.error","subscription_id":sub.id,"error":error}),
                    )?;
                    return Ok(());
                },
            };
            if previous.as_ref() != Some(&data) {
                let full = data.clone();
                let mut candidate = baseline.clone();
                candidate.encode(&mut data, since);
                // Conservatively account for the protocol envelope and newline.
                let bytes = serde_json::to_vec(&data).map_err(io::Error::other)?.len() + 256;
                if bytes > MAX_FRAME {
                    return Err(io::Error::other("screen_frame_too_large"));
                }
                let Some(sequence) = sub.watch.reserve(bytes) else {
                    continue;
                };
                previous = Some(full);
                baseline = candidate;
                since = data["screen_seq"].as_u64().unwrap_or(0);
                write_event(
                    stream,
                    &json!({"event":"pane.screen","subscription_id":sub.id,"sequence":sequence,"data":data}),
                )?;
                // Stay below the relay's byte budget during full-grid redraws.
                // Sparse interactive output is immediate after an idle window.
                next_frame = Instant::now()
                    + BURST_INTERVAL
                        .max(Duration::from_secs_f64(bytes as f64 / (2.0 * 1024.0 * 1024.0)));
            }
            last_progress = Instant::now();
        }
        if now >= heartbeat {
            write_event(
                stream,
                &json!({"event":"pane.screen.heartbeat","subscription_id":sub.id}),
            )?;
            heartbeat = Instant::now() + HEARTBEAT;
        }
        if last_progress.elapsed() > Duration::from_secs(60) && sub.watch.blocked() {
            return Err(io::Error::other("screen_ack_timeout"));
        }
        let delay = if next_frame > Instant::now() {
            next_frame.saturating_duration_since(Instant::now())
        } else {
            HEARTBEAT
        };
        let _ = sub
            .receiver
            .recv_timeout(delay.min(heartbeat.saturating_duration_since(Instant::now())));
    }
}

fn write_event(stream: &mut TcpStream, data: &Value) -> io::Result<()> {
    let mut event = data.clone();
    event["protocol"] = json!(PROTOCOL_NAME);
    event["version"] = json!(PROTOCOL_VERSION);
    write_json_line(stream, &event)
}
