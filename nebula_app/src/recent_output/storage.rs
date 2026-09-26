//! 本机 sidecar 编解码边界；调用方拥有保存顺序和清除代次。

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::CommandRecord;

pub(crate) const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRANSACTION_BYTES: usize = 2 * MAX_STORE_BYTES;

pub(crate) fn archive_path() -> std::path::PathBuf {
    crate::display::nebula_data_dir().join("command-output.json")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Archive {
    version: u32,
    panes: BTreeMap<String, Vec<CommandRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous: Option<Box<Archive>>,
}

impl Archive {
    pub(crate) fn new(panes: BTreeMap<String, Vec<CommandRecord>>) -> Self {
        Self { version: 1, panes, previous: None }
    }

    pub(crate) fn take_records(&mut self, id: &str) -> Option<Vec<CommandRecord>> {
        self.panes.remove(id).or_else(|| self.previous.as_mut()?.take_records(id))
    }

    fn validate(&self) -> io::Result<()> {
        if self.version != 1 || self.panes.len() > 4096 {
            return Err(invalid("unsupported or oversized output archive"));
        }
        for (id, records) in &self.panes {
            if id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(invalid("invalid pane reference"));
            }
            super::RecentOutput::validate_records(records).map_err(invalid)?;
        }
        Ok(())
    }

    fn encode(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        if let Some(previous) = &self.previous {
            if previous.previous.is_some() {
                return Err(invalid("nested output transaction"));
            }
            // 两代各自仍受正常上限约束，不能用交接额度保存一份过大的快照。
            previous.encode()?;
            Self::new(self.panes.clone()).encode()?;
        }
        let limit = if self.previous.is_some() { MAX_TRANSACTION_BYTES } else { MAX_STORE_BYTES };
        let mut encoded = LimitedBuffer { bytes: Vec::new(), limit };
        serde_json::to_writer(&mut encoded, self).map_err(io::Error::other)?;
        Ok(encoded.bytes)
    }

    pub(crate) fn write_to(&self, path: &Path) -> io::Result<()> {
        crate::atomic_file::write(path, &self.encode()?)
    }

    /// 从中断的两代交接中只保留已发布会话实际引用的记录。
    fn retain_references(&mut self, references: &[String]) {
        let mut panes = BTreeMap::new();
        for id in references {
            if let Some(records) = self.take_records(id) {
                panes.insert(id.clone(), records);
            }
        }
        *self = Self::new(panes);
    }

    pub(crate) fn read_from(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        if file.metadata()?.len() > MAX_TRANSACTION_BYTES as u64 {
            return Err(invalid("output archive exceeds byte budget"));
        }
        // metadata 之后仍可能增长，读取也必须限流。
        let mut bytes = Vec::new();
        file.take(MAX_TRANSACTION_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_TRANSACTION_BYTES {
            return Err(invalid("output archive exceeds byte budget"));
        }
        let archive: Self = serde_json::from_slice(&bytes).map_err(invalid)?;
        // 普通文件不能借助读入上限突破 16 MiB；交接文件逐代复核。
        if archive.previous.is_none() && bytes.len() > MAX_STORE_BYTES {
            return Err(invalid("output archive exceeds byte budget"));
        }
        archive.encode().map_err(invalid)?;
        Ok(archive)
    }
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(invalid("output archive exceeds byte budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

mod lifecycle;
pub(crate) use lifecycle::OutputStore;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent_output::{RecentOutput, tests::Size};
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::term::{Config, Term};
    use nebula_terminal::vte::ansi;

    pub(super) fn records(command: &str) -> Vec<CommandRecord> {
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, command.as_bytes());
        let mut recent = RecentOutput::default();
        recent.begin(term.grid());
        parser.advance(&mut term, b"\r\nresult\r\n");
        recent.finish(term.grid());
        let mut records = recent.snapshot(term.grid());
        for record in &mut records {
            record.captured_at = 0;
        }
        records
    }

    #[test]
    fn atomic_round_trip_keeps_same_directory_panes_separate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outputs.json");
        let archive = Archive::new(BTreeMap::from([
            ("pane-one".into(), records("C:>echo one")),
            ("pane-two".into(), records("C:>echo two")),
        ]));
        archive.write_to(&path).unwrap();
        let loaded = Archive::read_from(&path).unwrap();
        assert_eq!(loaded.panes.len(), 2);
        assert_eq!(serde_json::to_value(loaded).unwrap(), serde_json::to_value(archive).unwrap());
    }

    #[test]
    fn restore_resolves_exact_identity_once_and_never_falls_back_to_another_pane() {
        let mut archive = Archive::new(BTreeMap::from([
            ("pane-one".into(), records("C:>echo one")),
            ("pane-two".into(), records("C:>echo two")),
        ]));
        assert!(archive.take_records("missing").is_none());
        let one = archive.take_records("pane-one").unwrap();
        assert!(archive.take_records("pane-one").is_none());
        assert_eq!(
            serde_json::to_value(one).unwrap(),
            serde_json::to_value(records("C:>echo one")).unwrap()
        );
        assert!(archive.take_records("pane-two").is_some());
    }

    #[test]
    fn invalid_write_does_not_replace_last_good_archive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outputs.json");
        let mut archive = Archive::new(BTreeMap::from([("pane".into(), records("C:>good"))]));
        archive.write_to(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        archive.version = 999;
        assert!(archive.write_to(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn transaction_cannot_hide_oversized_generation_or_nested_transactions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outputs.json");
        let record = records("C:>safe");
        let bytes = serde_json::to_vec(&record).unwrap().len();
        let oversized = Archive::new(
            (0..(17 * 1024 * 1024 / bytes))
                .map(|index| (format!("pane-{index}"), record.clone()))
                .collect(),
        );
        let mut transaction = Archive::new(BTreeMap::new());
        transaction.previous = Some(Box::new(oversized));
        std::fs::write(&path, serde_json::to_vec(&transaction).unwrap()).unwrap();
        assert!(Archive::read_from(&path).is_err());
        assert!(transaction.write_to(&path).is_err());
        let mut nested = Archive::new(BTreeMap::new());
        nested.previous = Some(Box::new(Archive::new(BTreeMap::new())));
        transaction.previous = Some(Box::new(nested));
        std::fs::write(&path, serde_json::to_vec(&transaction).unwrap()).unwrap();
        assert!(Archive::read_from(&path).is_err());
        let oversized = std::fs::File::create(&path).unwrap();
        oversized.set_len(MAX_TRANSACTION_BYTES as u64 + 1).unwrap();
        drop(oversized);
        assert!(Archive::read_from(&path).is_err());
    }

    #[test]
    fn malformed_version_oversize_and_excess_records_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outputs.json");
        for contents in [b"broken".as_slice(), b"{\"version\":2,\"panes\":{}}"] {
            std::fs::write(&path, contents).unwrap();
            assert!(Archive::read_from(&path).is_err());
        }
        let oversized = std::fs::File::create(&path).unwrap();
        oversized.set_len(MAX_STORE_BYTES as u64 + 1).unwrap();
        drop(oversized);
        assert!(Archive::read_from(&path).is_err());
        let record = records("C:>test").remove(0);
        let archive = Archive::new(BTreeMap::from([("pane".into(), vec![record; 6])]));
        std::fs::write(&path, serde_json::to_vec(&archive).unwrap()).unwrap();
        assert!(Archive::read_from(&path).is_err());
    }
}
