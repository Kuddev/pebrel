//! Opt-in fixed-port listener so other local apps or LAN hosts can drive the Runtime API.

use super::server::fresh_token;
use super::*;
use nebula_settings::RuntimeApiExposure;

pub(crate) const REMOTE_PORT: u16 = 47600;
const TOKEN_FILE: &str = "runtime-remote.token";

/// 与本机私有端点不同，远程 token 跨启动保持不变，SDK 不必每次重新发现。
fn persistent_token(path: &std::path::Path) -> Result<String, IoError> {
    if let Ok(saved) = std::fs::read_to_string(path) {
        let saved = saved.trim();
        if saved.len() == 32 && saved.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(saved.to_owned());
        }
    }
    let token = fresh_token();
    crate::atomic_file::write(path, token.as_bytes())?;
    Ok(token)
}

pub(super) fn spawn(exposure: RuntimeApiExposure, sink: EventSink, hub: RuntimeHub) {
    let address = match exposure {
        RuntimeApiExposure::Off => return,
        RuntimeApiExposure::Local => Ipv4Addr::LOCALHOST,
        RuntimeApiExposure::Lan => Ipv4Addr::UNSPECIFIED,
    };
    let token = match persistent_token(&crate::display::nebula_data_dir().join(TOKEN_FILE)) {
        Ok(token) => token,
        Err(error) => {
            warn!("Runtime API remote: cannot persist token: {error}");
            return;
        },
    };
    let listener = match TcpListener::bind((address, REMOTE_PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            warn!("Runtime API remote: cannot bind {address}:{REMOTE_PORT}: {error}");
            return;
        },
    };
    info!("Runtime API remote listener on {address}:{REMOTE_PORT}");
    if let Err(error) = std::thread::Builder::new()
        .name("nebula-runtime-remote".into())
        .spawn(move || serve(listener, token, sink, hub))
    {
        warn!("Runtime API remote: cannot start listener: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_survives_restarts_and_replaces_a_corrupt_file() {
        let path = std::env::temp_dir()
            .join(format!("pebrel-remote-token-{}", std::process::id()))
            .join(TOKEN_FILE);
        let first = persistent_token(&path).unwrap();
        assert_eq!(persistent_token(&path).unwrap(), first);
        std::fs::write(&path, "garbage").unwrap();
        let second = persistent_token(&path).unwrap();
        assert_ne!(second, first);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), second);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
