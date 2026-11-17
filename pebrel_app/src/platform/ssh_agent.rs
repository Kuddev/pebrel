//! Native system-agent transports. Authentication policy and host identity
//! selection belong to ssh_session; this adapter owns no SSH connection.

use std::future::Future;
use std::time::Duration;

use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};

pub(crate) type Connection = AgentClient<Box<dyn AgentStream + Send + Unpin>>;
type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Endpoint {
    OpenSsh,
    Pageant,
    Environment,
}

#[cfg(windows)]
pub(crate) const ENDPOINTS: &[Endpoint] = &[Endpoint::OpenSsh, Endpoint::Pageant];
#[cfg(unix)]
pub(crate) const ENDPOINTS: &[Endpoint] = &[Endpoint::Environment];
#[cfg(not(any(windows, unix)))]
pub(crate) const ENDPOINTS: &[Endpoint] = &[];

pub(crate) async fn connect(endpoint: Endpoint) -> Result<Connection, Error> {
    match endpoint {
        #[cfg(windows)]
        Endpoint::OpenSsh => connect_pipe(r"\\.\pipe\openssh-ssh-agent").await,
        #[cfg(windows)]
        Endpoint::Pageant => Ok(AgentClient::connect_pageant().await?.dynamic()),
        #[cfg(unix)]
        Endpoint::Environment => Ok(AgentClient::connect_env().await?.dynamic()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "SSH agent endpoint is unavailable on this platform",
        )
        .into()),
    }
}

/// The same deadline covers opening a busy native endpoint and asking it for
/// identities. Signing has a separate, interactive budget in the SSH layer.
pub(crate) async fn discover_connection(
    connection: impl Future<Output = Result<Connection, Error>>,
    budget: Duration,
) -> Result<(Connection, Vec<AgentIdentity>), Error> {
    tokio::time::timeout(budget, async {
        let mut agent = connection.await?;
        let identities = agent.request_identities().await?;
        Ok((agent, identities))
    })
    .await
    .map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::TimedOut, "SSH agent discovery timed out")
    })?
}

#[cfg(windows)]
async fn connect_pipe(path: impl AsRef<std::ffi::OsStr>) -> Result<Connection, Error> {
    Ok(AgentClient::connect_named_pipe(path).await?.dynamic())
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
