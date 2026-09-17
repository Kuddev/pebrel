use std::{io, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use serde::Deserialize;
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
    time,
};
use tokio_rustls::{TlsAcceptor, rustls};
use tokio_util::sync::CancellationToken;

use super::{
    RelayConfig,
    registry::{Lease, Registry, Role, SharedRegistry},
};
use crate::crypto::MAX_PACKET;

#[derive(Clone)]
struct AppState {
    registry: SharedRegistry,
    peers: Arc<Semaphore>,
    shutdown: CancellationToken,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkQuery {
    device: String,
    role: String,
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok\n" }))
        .route(
            "/readyz",
            get(|State(state): State<AppState>| async move {
                if state.shutdown.is_cancelled() || state.peers.available_permits() == 0 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                }
            }),
        )
        .route("/v2/link", get(upgrade))
        .with_state(state)
}

async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<LinkQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if state.shutdown.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Some(role) = Role::parse(&query.role) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let Ok(permit) = state.peers.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (lease, queue) = match Registry::reserve(&state.registry, &query.device, role, token) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    };
    ws.max_message_size(MAX_PACKET)
        .max_frame_size(MAX_PACKET)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_PACKET * 2)
        .on_upgrade(move |socket| run_socket(socket, lease, queue, permit, state.shutdown))
}

async fn run_socket(
    socket: WebSocket,
    lease: Lease,
    mut queue: mpsc::Receiver<Message>,
    _permit: OwnedSemaphorePermit,
    shutdown: CancellationToken,
) {
    if lease.announce().is_err() {
        return;
    }
    let (mut send, mut receive) = socket.split();
    let mut heartbeat = time::interval(Duration::from_secs(25));
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut awaiting_pong = None;
    let mut ping_id = 0_u64;
    let mut window = time::Instant::now();
    let mut frames = 0;
    let mut bytes = 0;
    loop {
        let output = tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = lease.cancel.cancelled() => break,
            queued = queue.recv() => match queued { Some(message) => Some(message), None => break },
            _ = heartbeat.tick() => {
                if awaiting_pong.is_some() { break }
                ping_id += 1;
                let payload = ping_id.to_be_bytes().to_vec();
                awaiting_pong = Some(payload.clone());
                Some(Message::Ping(payload.into()))
            },
            received = receive.next() => {
                let Some(Ok(message)) = received else { break };
                if window.elapsed() >= Duration::from_secs(1) { frames = 0; bytes = 0; window = time::Instant::now(); }
                frames += 1;
                bytes += match &message {
                    Message::Binary(v) | Message::Ping(v) | Message::Pong(v) => v.len(),
                    Message::Text(v) => v.len(),
                    Message::Close(_) => 0,
                };
                if frames > 128 || bytes > 4 * 1024 * 1024 { break }
                match message {
                    Message::Binary(packet) if !packet.is_empty() => {
                        if lease.forward(packet).is_err() { break }
                        None
                    },
                    Message::Pong(payload) => {
                        if awaiting_pong.as_deref() == Some(payload.as_ref()) { awaiting_pong = None; }
                        None
                    },
                    Message::Ping(payload) => Some(Message::Pong(payload)),
                    _ => break,
                }
            }
        };
        if let Some(message) = output {
            if !matches!(
                time::timeout(Duration::from_secs(10), send.send(message)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    }
    // Cancel the peer immediately, even if this network can no longer flush.
    drop(lease);
    let _ = time::timeout(Duration::from_secs(1), send.close()).await;
}

fn tls(config: &super::TlsConfig) -> io::Result<TlsAcceptor> {
    let cert = super::setup::read_bounded(&config.certificate, 64 * 1024)?;
    let key = zeroize::Zeroizing::new(super::setup::read_bounded(&config.private_key, 64 * 1024)?);
    let certificates =
        rustls_pemfile::certs(&mut cert.as_slice()).collect::<Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut key.as_slice())?
        .ok_or_else(|| io::Error::other("missing_tls_key"))?;
    let provider = rustls::crypto::ring::default_provider();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map_err(io::Error::other)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn serve(config: RelayConfig, shutdown: CancellationToken) -> io::Result<()> {
    config.validate().map_err(io::Error::other)?;
    let listener = TcpListener::bind(config.listen).await?;
    serve_listener(config, listener, shutdown).await
}

pub(crate) async fn serve_listener(
    config: RelayConfig,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let acceptor = config.tls.as_ref().map(tls).transpose()?;
    let connections = Arc::new(Semaphore::new(config.max_peers * 2));
    let state = AppState {
        registry: Registry::new(config.rooms),
        peers: Arc::new(Semaphore::new(config.max_peers)),
        shutdown: shutdown.clone(),
    };
    let app = router(state);
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let Ok(permit) = connections.clone().try_acquire_owned() else { continue };
                stream.set_nodelay(true)?;
                let app = app.clone();
                let acceptor = acceptor.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let service = TowerToHyperService::new(app);
                    let builder = Builder::new(TokioExecutor::new());
                    // Bounds TLS/HTTP headers and idle non-upgraded HTTP clients.
                    // After upgrade the socket owner has its own limits/heartbeat.
                    let _ = time::timeout(Duration::from_secs(10), async {
                        if let Some(acceptor) = acceptor {
                            if let Ok(stream) = acceptor.accept(stream).await {
                                let _ = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).await;
                            }
                        } else {
                            let _ = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).await;
                        }
                    }).await;
                });
            }
        }
    }
    tasks.shutdown().await;
    Ok(())
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
