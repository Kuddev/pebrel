use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{SessionError, SharedSession};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

async fn reply(socket: &mut TcpStream, code: u8) -> io::Result<()> {
    socket.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

async fn request(socket: &mut TcpStream) -> Result<Option<(String, u16)>, SessionError> {
    let mut greeting = [0; 2];
    socket.read_exact(&mut greeting).await?;
    if greeting[0] != 5 || greeting[1] == 0 {
        return Ok(None);
    }
    let mut methods = [0; 255];
    socket.read_exact(&mut methods[..usize::from(greeting[1])]).await?;
    if !methods[..usize::from(greeting[1])].contains(&0) {
        socket.write_all(&[5, 255]).await?;
        return Ok(None);
    }
    socket.write_all(&[5, 0]).await?;
    let mut header = [0; 4];
    socket.read_exact(&mut header).await?;
    if header[0] != 5 || header[2] != 0 {
        reply(socket, 1).await?;
        return Ok(None);
    }
    if header[1] != 1 {
        reply(socket, 7).await?; // Only CONNECT; BIND and UDP ASSOCIATE are unavailable.
        return Ok(None);
    }
    let host = match header[3] {
        1 => {
            let mut address = [0; 4];
            socket.read_exact(&mut address).await?;
            IpAddr::V4(Ipv4Addr::from(address)).to_string()
        },
        4 => {
            let mut address = [0; 16];
            socket.read_exact(&mut address).await?;
            IpAddr::V6(Ipv6Addr::from(address)).to_string()
        },
        3 => {
            let count = socket.read_u8().await?;
            let mut address = [0; 255];
            let address = &mut address[..usize::from(count)];
            socket.read_exact(address).await?;
            let Ok(host) = std::str::from_utf8(address) else {
                reply(socket, 4).await?;
                return Ok(None);
            };
            if host.is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace()) {
                reply(socket, 4).await?;
                return Ok(None);
            }
            host.to_owned()
        },
        _ => {
            reply(socket, 8).await?;
            return Ok(None);
        },
    };
    let port = socket.read_u16().await?;
    if port == 0 {
        reply(socket, 4).await?;
        return Ok(None);
    }
    Ok(Some((host, port)))
}

pub(super) async fn serve(
    mut socket: TcpStream,
    peer: SocketAddr,
    session: SharedSession,
) -> Result<(), SessionError> {
    // One budget covers negotiation and channel creation. DNS stays on the SSH host.
    let channel = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let Some((host, port)) = request(&mut socket).await? else {
            return Ok::<_, SessionError>(None);
        };
        match session
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                peer.ip().to_string(),
                u32::from(peer.port()),
            )
            .await
        {
            Ok(channel) => {
                reply(&mut socket, 0).await?;
                Ok(Some(channel))
            },
            Err(error) => {
                reply(&mut socket, 1).await?;
                Err(error.into())
            },
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "SOCKS5 handshake timed out"))??;
    if let Some(channel) = channel {
        tokio::io::copy_bidirectional(&mut socket, &mut channel.into_stream()).await?;
    }
    Ok(())
}
