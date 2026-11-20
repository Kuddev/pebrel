//! 短码只引导取得完整邀请；不派生 Noise PSK，也不跳过两端校验与桌面批准。

use super::{HostState, authentication, invalid};
use std::io;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub(super) struct PairingCode {
    code: Zeroizing<String>,
    invitation: Zeroizing<String>,
    grant: String,
    attempts: u8,
    consumed: bool,
}

impl PairingCode {
    pub fn new(invitation: &str, grant: &str) -> io::Result<Self> {
        let number = loop {
            let mut bytes = [0; 4];
            getrandom::fill(&mut bytes).map_err(io::Error::other)?;
            let value = u32::from_le_bytes(bytes);
            // 拒绝取模余数，8 位十进制空间中的每个值等概率。
            if value < 4_200_000_000 {
                break value % 100_000_000;
            }
        };
        Ok(Self {
            code: Zeroizing::new(format!("{number:08}")),
            invitation: Zeroizing::new(invitation.to_owned()),
            grant: grant.to_owned(),
            attempts: 0,
            consumed: false,
        })
    }

    pub fn code(&self, host: &HostState, now: u64) -> Option<&str> {
        (!self.consumed && self.attempts < 5 && host.invitation_valid(&self.grant, now))
            .then_some(self.code.as_str())
    }

    pub fn redeem(&mut self, code: &str, host: &HostState, now: u64) -> io::Result<String> {
        if self.code(host, now).is_none() {
            return Err(authentication());
        }
        self.attempts += 1;
        if code.len() != 8 || !bool::from(code.as_bytes().ct_eq(self.code.as_bytes())) {
            return Err(authentication());
        }
        let value: serde_json::Value =
            serde_json::from_str(&self.invitation).map_err(|_| invalid())?;
        if value["secure"]["grant"].as_str() != Some(&self.grant) {
            return Err(invalid());
        }
        self.consumed = true;
        Ok(self.invitation.to_string())
    }
}
