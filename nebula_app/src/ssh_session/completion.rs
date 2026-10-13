//! Completion uses an already authenticated connection and its own bounded channel.

use super::*;
use russh::ChannelMsg;

#[derive(Clone)]
pub(crate) struct Connection {
    session: SharedSession,
    id: u64,
}

impl Connection {
    pub(crate) fn key(&self) -> u64 {
        self.id
    }
}

pub(crate) async fn capture(destination: &str) -> Result<Connection, SessionError> {
    let pool = connection_pool().lock().await;
    let mut matches = pool
        .values()
        .filter(|entry| entry.destination == destination && !entry.session.is_closed());
    let entry = matches.next().ok_or("completion_connection_unavailable")?;
    if matches.next().is_some() {
        return Err("completion_connection_ambiguous".into());
    }
    Ok(Connection { session: entry.session.clone(), id: entry.id })
}

#[cfg(test)]
pub(crate) async fn prepare_owned_fixture(
    destination: &str,
    key: &std::path::Path,
    known_hosts: &std::path::Path,
) -> Result<(), SessionError> {
    let parsed = SshDestination::resolve(destination)?;
    if parsed.host != "127.0.0.1" {
        return Err("completion fixture must use the owned loopback server".into());
    }
    let path = crate::display::nebula_data_dir().join("ssh_profiles.json");
    let mut profiles = crate::ssh_profiles::SshProfiles::load(&path)?;
    let mut profile = profiles.for_destination(destination);
    profile.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
    profile.private_keys = vec![key.to_owned()];
    profiles.upsert(profile.clone());
    profiles.save(&path)?;
    let route = route::ResolvedRoute {
        destination: parsed,
        profile,
        transport: route::RouteTransport::Direct,
        known_hosts_path: Some(known_hosts.to_owned()),
    };
    authenticated_route(&route, None::<&NoopSshEventHost>, false, false).await?;
    Ok(())
}

#[cfg(test)]
pub(crate) async fn read(
    destination: &str,
    command: &str,
    script: &[u8],
    budget: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, SessionError> {
    let connection = capture(destination).await?;
    read_connection(&connection, command, script, budget, limit, cancelled).await
}

pub(crate) async fn read_connection(
    connection: &Connection,
    command: &str,
    script: &[u8],
    budget: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, SessionError> {
    let query = async {
        if cancelled() {
            return Err("completion_cancelled".into());
        }
        let channel = connection.session.channel_open_session().await?;
        let mut channel = lifecycle::own_channel(channel);
        channel.exec(true, command).await?;
        let mut submitted = script.is_empty();
        let mut output = Vec::new();
        let mut status = None;
        let mut eof = false;
        let mut poll = tokio::time::interval(Duration::from_millis(20));
        loop {
            let message = tokio::select! {
                message = channel.wait() => message,
                _ = poll.tick() => {
                    if cancelled() { return Err("completion_cancelled".into()); }
                    continue;
                }
            };
            match message {
                Some(ChannelMsg::Success) if !submitted => {
                    channel.data_bytes(script.to_vec()).await?;
                    channel.eof().await?;
                    submitted = true;
                },
                Some(ChannelMsg::Data { data }) => {
                    if output.len().saturating_add(data.len()) > limit {
                        return Err("completion_output_limit".into());
                    }
                    output.extend_from_slice(&data);
                },
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    status = Some(exit_status);
                    if eof {
                        break;
                    }
                },
                Some(ChannelMsg::Failure) => return Err("completion_exec_rejected".into()),
                Some(ChannelMsg::Eof) => {
                    eof = true;
                    if status.is_some() {
                        break;
                    }
                },
                Some(ChannelMsg::Close) | None => break,
                _ => {},
            }
        }
        channel.finish().await?;
        if status != Some(0) || cancelled() {
            return Err("completion_exec_failed".into());
        }
        Ok(output)
    };
    tokio::time::timeout(budget, query).await.map_err(|_| "completion_query_timeout")?
}
