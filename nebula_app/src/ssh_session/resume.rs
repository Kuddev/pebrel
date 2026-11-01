//! Pane-owned remote recovery. Probes use separate exec channels, never interactive input.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{SharedSession, SshEventHost, exec};

mod identity;
pub(super) use identity::{IdentityInput, PtyIdentity};

const BUDGET: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_secs(2);
const CLIENT_FORMAT: &str =
    "#{client_tty}\t#{session_id}\t#{session_created}:#{pid}\t#{window_id}\t#{pane_id}";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    executable: String,
    socket: String,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self { executable: "tmux".into(), socket: String::new() }
    }
}

impl Endpoint {
    fn command(&self) -> String {
        let mut command = format!("{} -u", quote(&self.executable));
        if !self.socket.is_empty() {
            command.push_str(&format!(" -S {}", quote(&self.socket)));
        }
        command
    }

    fn parse(text: &str) -> Vec<Self> {
        let mut endpoints = Vec::new();
        for line in text.lines().take(64) {
            let Some((executable, socket)) = line.split_once('\t') else { continue };
            if valid_path(executable) && valid_path(socket) {
                let endpoint = Self { executable: executable.into(), socket: socket.into() };
                if !endpoints.contains(&endpoint) {
                    endpoints.push(endpoint);
                }
            }
            if endpoints.len() == 32 {
                break;
            }
        }
        endpoints
    }
}

/// Validated metadata from this pane's PTY; never serialized or shared by host alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeTarget {
    endpoint: Endpoint,
    session: String,
    identity: String,
    window: String,
    pane: String,
}

impl ResumeTarget {
    fn parse(output: &str, tty: &str, endpoint: &Endpoint) -> Option<Self> {
        let mut matching = output.lines().filter_map(|line| {
            let fields: Vec<_> = line.trim_end_matches('\r').split('\t').collect();
            if fields.len() != 5 || fields[0] != tty {
                return None;
            }
            let (created, pid) = fields[2].split_once(':')?;
            if !id(fields[1], '$')
                || !digits(created)
                || !digits(pid)
                || !id(fields[3], '@')
                || !id(fields[4], '%')
            {
                return None;
            }
            Some(Self {
                endpoint: endpoint.clone(),
                session: fields[1].into(),
                identity: fields[2].into(),
                window: fields[3].into(),
                pane: fields[4].into(),
            })
        });
        let target = matching.next()?;
        matching.next().is_none().then_some(target)
    }

    fn attach(&self) -> String {
        let command = self.endpoint.command();
        let session = quote(&self.session);
        let window = quote(&format!("{}:{}", self.session, self.window));
        let pane = quote(&format!("{}:{}.{}", self.session, self.window, self.pane));
        format!(
            "if [ \"$({command} display-message -p -t {session} '#{{session_created}}:#{{pid}}' 2>/dev/null)\" = {} ] && \
             {command} select-window -t {window} 2>/dev/null && \
             {command} select-pane -t {pane} 2>/dev/null; then \
             {command} attach-session -t {session}; fi; ",
            quote(&self.identity)
        )
    }
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn id(value: &str, prefix: char) -> bool {
    value.strip_prefix(prefix).is_some_and(digits)
}

fn valid_path(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn posix(script: &str) -> String {
    format!("sh -c {}", quote(script))
}

pub(super) const PROBE_REPLY: &str = "PEBREL_POSIX_2:'\"$\\ spaced";

pub(super) fn probe_command() -> String {
    // Finding sh does not establish how the SSH server's outer shell passes
    // nested quotes. Exercise the same quoting and expansion as startup.
    posix(&format!("pebrel_probe={}; printf '%s' \"$pebrel_probe\"", quote(PROBE_REPLY)))
}

pub(super) async fn supported(session: &SharedSession) -> bool {
    query(session, &probe_command()).await.as_deref() == Some(PROBE_REPLY)
}

pub(super) fn startup(
    token: &str,
    bootstrap: Option<&str>,
    target: Option<&ResumeTarget>,
    cwd: Option<&str>,
) -> String {
    // tty prints "not a tty" when startup has no terminal, as reported for JumpServer direct
    // connections. The filter shows malformed markers verbatim, so send one only on success.
    let mut script = format!(
        "if pebrel_tty=$(tty 2>/dev/null); then \
         printf '\\036PEBREL_PTY_{token}=%s:%s\\037' \"$pebrel_tty\" \"$$\"; fi; "
    );
    if let Some(target) = target {
        script.push_str(&target.attach());
    }
    if let Some(path) = cwd.filter(|path| valid_path(path)) {
        script.push_str(&format!("cd {} 2>/dev/null || :; ", quote(path)));
    }
    script.push_str(&format!(
        "export NEBULA_REMOTE_HOOK_TOKEN={} NEBULA_PANE_REMOTE=1; ",
        quote(token)
    ));
    script.push_str(bootstrap.unwrap_or("exec \"${SHELL:-/bin/sh}\" -l"));
    posix(&script)
}

async fn query(session: &SharedSession, command: &str) -> Option<String> {
    tokio::time::timeout(BUDGET, async {
        let channel = session.channel_open_session().await.ok()?;
        let bytes = exec::capture_private(channel, command).await.ok()?;
        String::from_utf8(bytes).ok()
    })
    .await
    .ok()
    .flatten()
}

async fn client_at(
    session: &SharedSession,
    identity: &PtyIdentity,
    endpoint: &Endpoint,
) -> Option<ResumeTarget> {
    let command = format!("{} list-clients -F {}", endpoint.command(), quote(CLIENT_FORMAT));
    let output = query(session, &posix(&command)).await?;
    ResumeTarget::parse(&output, &identity.tty, endpoint)
}

async fn checkpoint(
    session: &SharedSession,
    identity: &PtyIdentity,
    endpoint: &Endpoint,
) -> Option<Option<ResumeTarget>> {
    // The PID and tty are learned on this exact channel, including when the transport is pooled.
    let command = posix(&format!(
        "ps -t {} -o pid= -o tty= -o comm=",
        quote(identity.tty.trim_start_matches("/dev/"))
    ));
    let processes = query(session, &command).await?;
    if !identity.has_client(&processes)? {
        return Some(None);
    }
    if let Some(target) = client_at(session, identity, endpoint).await {
        return Some(Some(target));
    }
    let discovered = query(session, &posix(include_str!("resume/discover.sh"))).await?;
    let mut found = None;
    for candidate in Endpoint::parse(&discovered).into_iter().filter(|value| value != endpoint) {
        if let Some(target) = client_at(session, identity, &candidate).await {
            if found.is_some() {
                return None;
            }
            found = Some(target);
        }
    }
    found.map(Some)
}

pub(super) async fn monitor<H: SshEventHost>(
    session: &SharedSession,
    identity: Arc<Mutex<Option<PtyIdentity>>>,
    initial: Option<&ResumeTarget>,
    events: &H,
) -> std::convert::Infallible {
    let mut endpoint = initial.map(|target| target.endpoint.clone()).unwrap_or_default();
    loop {
        let current = identity.lock().ok().and_then(|value| value.clone());
        if let Some(identity) = current
            && let Ok(Some(target)) =
                tokio::time::timeout(BUDGET, checkpoint(session, &identity, &endpoint)).await
        {
            if let Some(target) = &target {
                endpoint = target.endpoint.clone();
            }
            events.ssh_resume_target(target);
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod acceptance;
