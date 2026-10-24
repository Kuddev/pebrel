//! Native file-copy payloads are distinct from text that happens to contain a path.

use std::io;
use std::path::PathBuf;

pub(crate) const MAX_FILES: usize = 32;
pub(crate) const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

fn invalid_payload() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Unsupported or invalid file clipboard payload")
}

pub(crate) fn decode(bytes: &[u8]) -> io::Result<Vec<PathBuf>> {
    if !(24..=MAX_PAYLOAD_BYTES).contains(&bytes.len()) {
        return Err(invalid_payload());
    }
    let offset = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let wide = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if wide == 0 || offset < 20 || offset % 2 != 0 || offset > bytes.len() - 4 {
        return Err(invalid_payload());
    }
    let mut paths = Vec::new();
    let mut units = Vec::new();
    for pair in bytes[offset..].chunks_exact(2) {
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit != 0 {
            if units.len() >= 32_767 {
                return Err(invalid_payload());
            }
            units.push(unit);
        } else if units.is_empty() {
            return if paths.is_empty() { Err(invalid_payload()) } else { Ok(paths) };
        } else {
            if paths.len() == MAX_FILES {
                return Err(invalid_payload());
            }
            let path = String::from_utf16(&units).map_err(|_| invalid_payload())?;
            paths.push(PathBuf::from(path));
            units.clear();
        }
    }
    Err(invalid_payload()) // A file list ends in a second NUL, not the buffer boundary.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(paths: &[&str]) -> Vec<u8> {
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&20u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        for path in paths {
            bytes.extend(path.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        }
        bytes.extend([0, 0]);
        bytes
    }

    #[test]
    fn explorer_file_list_preserves_unicode_spaces_and_order() {
        let paths = [r"C:\My files\报告 📎.pdf", r"\\server\share\second.txt"];
        assert_eq!(decode(&payload(&paths)).unwrap(), paths.map(PathBuf::from));
    }

    #[test]
    fn text_and_malformed_file_lists_are_not_native_files() {
        assert!(decode(br"C:\looks-like-a-file.pdf").is_err());
        let original = payload(&[r"C:\report.pdf"]);
        for length in 0..original.len() {
            assert!(decode(&original[..length]).is_err(), "accepted prefix of {length} bytes");
        }
        let mut bytes = original.clone();
        bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_err());
        bytes = original.clone();
        bytes[16..20].fill(0); // Legacy ANSI DROPFILES is deliberately unsupported.
        assert!(decode(&bytes).is_err());
        bytes = original;
        bytes[20..22].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn file_count_and_clipboard_allocation_are_bounded() {
        assert_eq!(decode(&payload(&vec!["file"; MAX_FILES])).unwrap().len(), MAX_FILES);
        assert!(decode(&payload(&vec!["file"; MAX_FILES + 1])).is_err());
        assert!(decode(&vec![0; MAX_PAYLOAD_BYTES + 1]).is_err());
        assert!(decode(&payload(&[])).is_err());
    }
}
