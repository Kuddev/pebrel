//! Generate the QR module matrix in-process. The GPUI/Compose adapters draw the
//! matrix directly; no browser, network call, Node process or bitmap generator.

use qrcode::{Color, EcLevel, QrCode};

use crate::crypto::LinkError;

pub struct PairingQr {
    width: usize,
    dark: Vec<bool>,
}

impl PairingQr {
    pub fn encode(invitation: &[u8]) -> Result<Self, LinkError> {
        if invitation.is_empty() || invitation.len() > 2048 {
            return Err(LinkError::Frame);
        }
        let code = QrCode::with_error_correction_level(invitation, EcLevel::M)
            .map_err(|_| LinkError::Frame)?;
        let width = code.width() + 8;
        let mut dark = vec![false; width * width];
        for y in 0..code.width() {
            for x in 0..code.width() {
                dark[(y + 4) * width + x + 4] = code[(x, y)] == Color::Dark;
            }
        }
        Ok(Self { width, dark })
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.width && self.dark[y * self.width + x]
    }

    /// Cold-path opaque black/white RGBA, including the four-module quiet zone.
    /// Always use integer scaling and preserve aspect ratio when rendering.
    pub fn rgba(&self, scale: usize) -> Result<Vec<u8>, LinkError> {
        if !(1..=8).contains(&scale) {
            return Err(LinkError::Frame);
        }
        let side = self.width * scale;
        let mut output = vec![255; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                if self.is_dark(x / scale, y / scale) {
                    let start = (y * side + x) * 4;
                    output[start..start + 3].fill(0);
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_qr_has_quiet_zone_finder_and_bounded_rgba() {
        let code = PairingQr::encode(br#"{"version":2,"device":"host"}"#).unwrap();
        assert!(code.width() > 21);
        for y in 0..code.width() {
            for x in 0..4 {
                assert!(!code.is_dark(x, y));
                assert!(!code.is_dark(y, x));
            }
        }
        assert!(code.is_dark(4, 4));
        assert!(code.is_dark(10, 4));
        assert_eq!(code.rgba(3).unwrap().len(), (code.width() * 3).pow(2) * 4);
        assert!(code.rgba(0).is_err());
        assert!(PairingQr::encode(&vec![b'a'; 2049]).is_err());
    }
}
