use super::{
    Handle, MAX_FRAME, MAX_REQUEST, RelaySettings, RuntimeFactory, Status, bridge, set_status,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time,
};
use tokio_tungstenite::{
    WebSocketStream, connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;

async fn send<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    value: Value,
) -> io::Result<()> {
    let message = value.to_string();
    if message.len() > MAX_FRAME {
        return Err(io::Error::other("frame_too_large"));
    }
    time::timeout(Duration::from_secs(8), socket.send(Message::Text(message.into())))
        .await
        .map_err(|_| io::Error::other("mobile_send_timeout"))?
        .map_err(io::Error::other)
}

pub(super) async fn serve_phone<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    factory: RuntimeFactory,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let epoch = crate::identity::Secret::generate().map_err(io::Error::other)?.hash();
    send(&mut socket, json!({"type":"relay.paired","version":1,"link":epoch})).await?;
    exchange(&mut socket, &epoch, factory, shutdown).await
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    epoch: &str,
    factory: RuntimeFactory,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let (input, mut output) = bridge(factory);
    let mut heartbeat = time::interval(Duration::from_secs(25));
    heartbeat.tick().await;
    let mut last_received = time::Instant::now();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            _ = heartbeat.tick() => {
                if last_received.elapsed() > Duration::from_secs(65) { return Err(io::Error::other("mobile_peer_timeout")); }
                time::timeout(Duration::from_secs(8),socket.send(Message::Ping(Vec::new().into()))).await
                    .map_err(|_|io::Error::other("mobile_send_timeout"))?.map_err(io::Error::other)?;
            },
            reply = output.recv() => {
                let bytes = reply.ok_or_else(||io::Error::other("runtime_connection_lost"))?;
                let body: Value = serde_json::from_slice(&bytes).map_err(|_|io::Error::other("invalid_runtime_frame"))?;
                send(socket,json!({"type":"relay.data","link":epoch,"body":body})).await?;
            },
            frame = async {
                let permit = input.reserve().await.map_err(|_|io::Error::other("mobile_input_closed"))?;
                Ok::<_,io::Error>((permit,socket.next().await))
            } => {
                let (permit,frame) = frame?;
                last_received = time::Instant::now();
                match frame {
                    Some(Ok(Message::Ping(_)|Message::Pong(_))) => {},
                    Some(Ok(Message::Text(text))) if text.len() <= MAX_REQUEST + 1024 => {
                        let value: Value = serde_json::from_str(&text).map_err(|_|io::Error::other("invalid_mobile_frame"))?;
                        if value["type"] == "relay.peer_left" { return Ok(()); }
                        if value["type"] != "relay.data" || value["link"] != epoch || !value["body"].is_object() {
                            return Err(io::Error::other("invalid_mobile_frame"));
                        }
                        let request = serde_json::to_vec(&value["body"]).map_err(io::Error::other)?;
                        if request.len() > MAX_REQUEST { return Err(io::Error::other("mobile_request_too_large")); }
                        permit.send(request);
                    },
                    _ => return Err(io::Error::other("mobile_disconnected")),
                }
            }
        }
    }
}

pub async fn start_relay(settings: RelaySettings, factory: RuntimeFactory) -> io::Result<Handle> {
    let invitation = settings.invitation()?;
    let url = format!(
        "{}/v1/link?device={}&role=desktop",
        settings.url.trim_end_matches('/'),
        settings.device
    );
    let mut request =
        url.into_client_request().map_err(|_| io::Error::other("invalid_relay_url"))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", settings.desktop_token)
            .parse()
            .map_err(|_| io::Error::other("invalid_relay_settings"))?,
    );
    let shutdown = CancellationToken::new();
    let status = Arc::new(Mutex::new(Status::Starting));
    let mut socket = connect(request.clone()).await?;
    // Show QR only after the relay has accepted the desktop credential.
    let initial_epoch =
        time::timeout(Duration::from_secs(12), wait_pairing_notice(&mut socket, false))
            .await
            .map_err(|_| io::Error::other("relay_timeout"))??;
    set_status(&status, Status::Waiting);
    let handle = Handle { invitation, shutdown: shutdown.clone(), status: status.clone() };
    tokio::spawn(async move {
        let mut failures = 0_u32;
        let mut pending_epoch = initial_epoch;
        loop {
            let paired = if let Some(epoch) = pending_epoch.take() {
                Ok(Some(epoch))
            } else {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    result = wait_pairing_notice(&mut socket, true) => result,
                }
            };
            if let Ok(Some(epoch)) = paired {
                failures = 0;
                set_status(&status, Status::Connected);
                let _ = exchange(&mut socket, &epoch, factory.clone(), shutdown.clone()).await;
            }
            if shutdown.is_cancelled() {
                break;
            }
            let _ = time::timeout(Duration::from_secs(1), socket.close(None)).await;
            set_status(&status, Status::Reconnecting);
            loop {
                failures = failures.saturating_add(1);
                // Bounded backoff, no replay queue. Authentication errors require
                // user action instead of an infinite retry storm.
                tokio::select! {
                    _ = shutdown.cancelled() => { set_status(&status,Status::Stopped); return; },
                    _ = time::sleep(Duration::from_secs((1_u64 << failures.min(5)).min(30))) => {},
                }
                let result = tokio::select! {
                    _ = shutdown.cancelled() => { set_status(&status,Status::Stopped); return; },
                    result = connect(request.clone()) => result,
                };
                match result {
                    Ok(next) => {
                        socket = next;
                        set_status(&status, Status::Waiting);
                        break;
                    },
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                        set_status(&status, Status::Failed);
                        return;
                    },
                    Err(_) => {},
                }
            }
        }
        set_status(&status, Status::Stopped);
    });
    Ok(handle)
}

type RelaySocket = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn connect(
    request: tokio_tungstenite::tungstenite::http::Request<()>,
) -> io::Result<RelaySocket> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME))
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_FRAME * 2);
    match time::timeout(
        Duration::from_secs(12),
        connect_async_with_config(request, Some(config), true),
    )
    .await
    {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
            if matches!(response.status().as_u16(), 401 | 403 | 409) =>
        {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "relay_authentication_failed"))
        },
        _ => Err(io::Error::other("relay_connection_failed")),
    }
}

async fn wait_pairing_notice(
    socket: &mut RelaySocket,
    wait_paired: bool,
) -> io::Result<Option<String>> {
    loop {
        // The v1 relay pings every 30 seconds; a silent peer cannot hold this
        // phase forever, even before a phone connects.
        let frame = time::timeout(Duration::from_secs(65), socket.next())
            .await
            .map_err(|_| io::Error::other("relay_timeout"))?;
        match frame {
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
            Some(Ok(Message::Text(text))) if text.len() <= 1024 => {
                let value: Value = serde_json::from_str(&text)
                    .map_err(|_| io::Error::other("invalid_relay_frame"))?;
                match value["type"].as_str() {
                    Some("relay.waiting" | "relay.peer_left") => {
                        if !wait_paired {
                            return Ok(None);
                        }
                    },
                    Some("relay.paired") => {
                        let epoch = value["link"]
                            .as_str()
                            .filter(|v| crate::identity::valid_id(v))
                            .ok_or_else(|| io::Error::other("invalid_relay_frame"))?;
                        // A phone cannot know a newly imported QR yet, but saved
                        // phones may already be waiting. Preserve this notice.
                        return Ok(Some(epoch.into()));
                    },
                    _ => return Err(io::Error::other("invalid_relay_frame")),
                }
            },
            _ => return Err(io::Error::other("relay_disconnected")),
        }
    }
}
