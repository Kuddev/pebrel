//! A bounded, nonce-scoped startup marker gives pooled channels distinct PTY identities.
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ssh_session) struct PtyIdentity {
    pub tty: String,
    pid: u32,
}

impl PtyIdentity {
    fn parse(value: &[u8]) -> Option<Self> {
        let (tty, pid) = std::str::from_utf8(value).ok()?.split_once(':')?;
        if !tty.starts_with("/dev/")
            || tty.len() > 128
            || !tty.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_-".contains(&b))
        {
            return None;
        }
        let pid = pid.parse::<u32>().ok().filter(|pid| *pid != 0)?;
        Some(Self { tty: tty.into(), pid })
    }

    pub(in crate::ssh_session) fn has_client(&self, processes: &str) -> Option<bool> {
        let mut owned = false;
        let mut client = false;
        for line in processes.lines() {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let tty = fields.next()?;
            let command = fields.next()?;
            if tty != self.tty.trim_start_matches("/dev/") {
                continue;
            }
            owned |= pid == self.pid;
            client |= command == "tmux" || command == "tmux:" || command.ends_with("/tmux");
        }
        owned.then_some(client)
    }
}

pub(in crate::ssh_session) struct IdentityInput {
    prefix: Vec<u8>,
    candidate: Vec<u8>,
    scanned: usize,
    done: bool,
    identity: Arc<Mutex<Option<PtyIdentity>>>,
}

impl IdentityInput {
    pub(in crate::ssh_session) fn new(
        token: &str,
        identity: Arc<Mutex<Option<PtyIdentity>>>,
    ) -> Self {
        Self {
            prefix: format!("\x1ePEBREL_PTY_{token}=").into_bytes(),
            candidate: Vec::new(),
            scanned: 0,
            done: false,
            identity,
        }
    }

    pub(in crate::ssh_session) fn feed<'a>(&mut self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        if self.done {
            return Cow::Borrowed(bytes);
        }
        let mut output = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            if self.done {
                output.push(byte);
                continue;
            }
            self.scanned += 1;
            if self.candidate.len() < self.prefix.len() {
                if byte == self.prefix[self.candidate.len()] {
                    self.candidate.push(byte);
                } else {
                    output.append(&mut self.candidate);
                    if byte == self.prefix[0] {
                        self.candidate.push(byte);
                    } else {
                        output.push(byte);
                    }
                }
            } else if byte == 0x1f {
                if let Some(identity) = PtyIdentity::parse(&self.candidate[self.prefix.len()..]) {
                    *self.identity.lock().unwrap() = Some(identity);
                    self.candidate.clear();
                } else {
                    output.append(&mut self.candidate);
                    output.push(byte);
                }
                self.done = true;
            } else {
                self.candidate.push(byte);
            }
            if self.scanned >= 8192 || self.candidate.len() > self.prefix.len() + 160 {
                output.append(&mut self.candidate);
                self.done = true;
            }
        }
        Cow::Owned(output)
    }

    pub(in crate::ssh_session) fn finish(&mut self) -> Vec<u8> {
        self.done = true;
        std::mem::take(&mut self.candidate)
    }
}
