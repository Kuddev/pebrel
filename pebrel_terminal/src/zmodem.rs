//! ZMODEM session-start detection.
//!
//! Pebrel does not implement ZMODEM file transfer. A remote `rz` (or `sz`)
//! puts the TTY into raw mode and waits for the terminal to answer its
//! hex header; without an answer it keeps the shell wedged until the user
//! types the protocol's cancel sequence by hand. The stream processor uses
//! this sniffer to recognise those headers so the terminal can cancel the
//! session instead.
//!
//! Only the two session-opening hex headers are recognised, and only when
//! their CRC-16 is valid:
//!
//! - `ZRINIT` (type `01`): a receiver (`rz`) is asking for a file upload.
//! - `ZRQINIT` (type `00`): a sender (`sz`) is offering a file download.
//!
//! The wire form is `ZPAD ZPAD ZDLE 'B' <14 hex digits>`. The issue #806
//! capture (ssh inside a Windows ConPTY pane) shows no `ZDLE` (CAN, `0x18`)
//! between the pads and `B`, so the sniffer also accepts the header without it.

/// The kind of transfer a remote program tried to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZmodemOffer {
    /// `rz` sent `ZRINIT`: it waits for the terminal to upload a file.
    Upload,
    /// `sz` sent `ZRQINIT`: it waits for the terminal to receive a file.
    Download,
}

/// The cancel sequence lrzsz itself sends (`canit`): ten CAN bytes abort
/// either side after five, and the backspaces erase any CAN bytes a shell
/// echoes once the transfer program has already exited.
pub const ABORT_SEQUENCE: &str =
    "\x18\x18\x18\x18\x18\x18\x18\x18\x18\x18\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08";

const ZPAD: u8 = b'*';
const ZDLE: u8 = 0x18;
const ZHEX: u8 = b'B';
const HEX_DIGITS: usize = 14;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Pad,
    Pads,
    Zdle,
    Hex(u8),
}

/// Incremental, allocation-free header matcher. State survives chunk
/// boundaries so a header split across PTY reads is still recognised.
#[derive(Debug, Default)]
pub struct ZmodemSniffer {
    state: State,
    digits: [u8; HEX_DIGITS],
}

impl ZmodemSniffer {
    /// Scans `bytes` and returns the last session-opening header it completes.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<ZmodemOffer> {
        let mut offer = None;
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.state == State::Idle {
                // Ordinary output never leaves Idle without a `*`, so skip
                // straight to the next candidate.
                let Some(start) = find_pad(rest) else { break };
                rest = &rest[start..];
            }
            let byte = rest[0];
            rest = &rest[1..];
            if let Some(found) = self.step(byte) {
                offer = Some(found);
            }
        }
        offer
    }

    fn step(&mut self, byte: u8) -> Option<ZmodemOffer> {
        self.state = match (self.state, byte) {
            (State::Idle, ZPAD) => State::Pad,
            (State::Pad, ZPAD) | (State::Pads, ZPAD) => State::Pads,
            (State::Pads, ZDLE) => State::Zdle,
            (State::Pads, ZHEX) | (State::Zdle, ZHEX) => State::Hex(0),
            (State::Hex(count), digit) if digit.is_ascii_hexdigit() => {
                self.digits[usize::from(count)] = digit;
                let count = count + 1;
                if usize::from(count) < HEX_DIGITS {
                    State::Hex(count)
                } else {
                    self.state = State::Idle;
                    return self.header();
                }
            },
            (_, ZPAD) => State::Pad,
            _ => State::Idle,
        };
        None
    }

    fn header(&self) -> Option<ZmodemOffer> {
        let mut decoded = [0u8; HEX_DIGITS / 2];
        for (byte, pair) in decoded.iter_mut().zip(self.digits.chunks_exact(2)) {
            *byte = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
        }
        let (frame, crc) = decoded.split_at(5);
        if crc16(frame) != u16::from_be_bytes([crc[0], crc[1]]) {
            return None;
        }
        match frame[0] {
            0x00 => Some(ZmodemOffer::Download),
            0x01 => Some(ZmodemOffer::Upload),
            _ => None,
        }
    }
}

/// Finds the next `ZPAD`. Whole 16-byte blocks are tested without
/// short-circuiting so the compiler can vectorise the common no-match case.
fn find_pad(bytes: &[u8]) -> Option<usize> {
    let blocks = bytes.chunks_exact(16);
    let clean = blocks.take_while(|block| !block.iter().fold(false, |hit, &b| hit | (b == ZPAD)));
    let skipped = clean.count() * 16;
    bytes[skipped..].iter().position(|&byte| byte == ZPAD).map(|index| skipped + index)
}

fn hex_value(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|value| value as u8)
}

/// CRC-16/XMODEM, the checksum ZMODEM hex headers carry.
fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |mut crc, &byte| {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 { crc << 1 } else { (crc << 1) ^ 0x1021 };
        }
        crc
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte what lrzsz `rz` prints (issue #806 screenshot).
    const RZ: &[u8] = b"rz waiting to receive.**\x18B0100000023be50\r\x8a\x11";
    const SZ: &[u8] = b"rz\r**\x18B00000000000000\r\x8a\x11";

    fn split_feed(bytes: &[u8], split: usize) -> Vec<ZmodemOffer> {
        let mut sniffer = ZmodemSniffer::default();
        [&bytes[..split], &bytes[split..]]
            .into_iter()
            .filter_map(|chunk| sniffer.feed(chunk))
            .collect()
    }

    #[test]
    fn session_headers_are_found_once_at_every_chunk_boundary() {
        for (wire, offer) in [(RZ, ZmodemOffer::Upload), (SZ, ZmodemOffer::Download)] {
            for split in 0..=wire.len() {
                assert_eq!(split_feed(wire, split), [offer], "split at {split}");
            }
            let mut sniffer = ZmodemSniffer::default();
            let found: Vec<_> = wire.chunks(1).filter_map(|byte| sniffer.feed(byte)).collect();
            assert_eq!(found, [offer], "byte-at-a-time");
        }
    }

    #[test]
    fn conpty_rendered_header_without_zdle_is_still_recognised() {
        let mut sniffer = ZmodemSniffer::default();
        assert_eq!(
            sniffer.feed(b"rz waiting to receive.**B0100000023be50\r\n"),
            Some(ZmodemOffer::Upload)
        );
        assert_eq!(sniffer.feed(b"***B00000000000000"), Some(ZmodemOffer::Download));
        assert_eq!(sniffer.feed(b"**B0100000023BE50"), Some(ZmodemOffer::Upload));
    }

    #[test]
    fn lookalike_output_does_not_start_a_cancel() {
        for text in [
            b"**B0100000023be51".as_slice(), // bad CRC
            b"**\x18B0100000023be5",         // truncated header
            b"**\x18B048906000000000",       // digits shifted off the CRC
            b"*\x18B0100000023be50",         // single ZPAD
            b"**\x18A0100000023be50",        // binary header marker
            b"** B0100000023be50",           // separator between pad and type
            b"**B01000000g3be50",            // non-hex digit
            b"**bold** and ***B***",         // markdown-ish noise
            b"",
        ] {
            assert_eq!(ZmodemSniffer::default().feed(text), None, "{}", text.escape_ascii());
        }
        // ZFILE has a valid CRC but is not a session-opening header.
        assert_eq!(ZmodemSniffer::default().feed(b"**\x18B04000000008906"), None);
    }

    #[test]
    fn a_broken_candidate_does_not_hide_the_following_header() {
        let mut sniffer = ZmodemSniffer::default();
        assert_eq!(sniffer.feed(b"**\x18B01000**\x18B0100000023be50"), Some(ZmodemOffer::Upload));
        assert_eq!(sniffer.feed(b"**B01**B0100000023be50"), Some(ZmodemOffer::Upload));
    }

    #[test]
    fn abort_sequence_matches_lrzsz_canit() {
        let bytes = ABORT_SEQUENCE.as_bytes();
        assert_eq!(bytes.len(), 20);
        assert!(bytes[..10].iter().all(|&byte| byte == ZDLE));
        assert!(bytes[10..].iter().all(|&byte| byte == 0x08));
    }

    #[test]
    fn header_after_long_plain_output_is_found_at_every_offset() {
        for offset in 0..48 {
            let mut wire = vec![b'x'; offset];
            wire.extend_from_slice(RZ);
            assert_eq!(ZmodemSniffer::default().feed(&wire), Some(ZmodemOffer::Upload), "{offset}");
        }
    }

    #[test]
    fn crc_matches_lrzsz_headers() {
        assert_eq!(crc16(&[0x01, 0, 0, 0, 0x23]), 0xbe50);
        assert_eq!(crc16(&[0; 5]), 0);
        assert_eq!(crc16(&[0x04, 0, 0, 0, 0]), 0x8906);
    }
}
