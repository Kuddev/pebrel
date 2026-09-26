//! 本机输出与可分享布局分离；编码和不可变引用在后台保存边界生成。

use crate::recent_output::{CommandRecord, storage::Archive};
use crate::session::Session;
use std::collections::BTreeMap;
use std::io;

pub(in crate::gpui_shell::workspace) struct LocalSnapshot {
    pub(in crate::gpui_shell::workspace) session: Session,
    pub(in crate::gpui_shell::workspace) outputs: Vec<Vec<(u64, Vec<CommandRecord>)>>,
    pub(in crate::gpui_shell::workspace) window_id: u64,
}

pub(in crate::gpui_shell::workspace) fn prepare_windows(
    mut windows: Vec<(bool, LocalSnapshot)>,
) -> io::Result<(Option<Session>, Archive)> {
    trim_outputs(&mut windows)?;
    let mut sessions = Vec::new();
    let mut panes = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for (active, snapshot) in windows {
        if !seen.insert(snapshot.window_id) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "duplicate output window"));
        }
        let (session, mut archive) = snapshot.prepare()?;
        for id in session.tabs.iter().flat_map(|tab| &tab.output_refs).filter(|id| !id.is_empty()) {
            let records = archive.take_records(id).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing prepared output")
            })?;
            panes.insert(id.clone(), records);
        }
        sessions.push((active, session));
    }
    Ok((crate::session::combine_sessions(sessions), Archive::new(panes)))
}

/// 在生成不可变引用前淘汰全局最旧记录，不让超额输出阻断布局保存。
fn trim_outputs(windows: &mut [(bool, LocalSnapshot)]) -> io::Result<()> {
    let mut bytes = 1024usize;
    let mut oldest = Vec::new();
    for (wi, (_, window)) in windows.iter().enumerate() {
        for (ti, tab) in window.outputs.iter().enumerate() {
            for (pi, (_, records)) in tab.iter().enumerate() {
                crate::recent_output::RecentOutput::validate_records(records)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                // 引用最长 128 字节，额外预留 JSON 结构及两代交接的封装空间。
                bytes += 256;
                for (ri, record) in records.iter().enumerate() {
                    let size = serde_json::to_vec(record)?.len() + 1;
                    bytes += size;
                    oldest.push((record.captured_at, wi, ti, pi, ri, size));
                }
            }
        }
    }
    oldest.sort_unstable();
    let mut removed = std::collections::HashSet::new();
    for (_, wi, ti, pi, ri, size) in oldest {
        if bytes <= crate::recent_output::storage::MAX_STORE_BYTES {
            break;
        }
        removed.insert((wi, ti, pi, ri));
        bytes = bytes.saturating_sub(size);
    }
    for (wi, (_, window)) in windows.iter_mut().enumerate() {
        for (ti, tab) in window.outputs.iter_mut().enumerate() {
            for (pi, (_, records)) in tab.iter_mut().enumerate() {
                let mut ri = 0;
                records.retain(|_| {
                    let keep = !removed.contains(&(wi, ti, pi, ri));
                    ri += 1;
                    keep
                });
            }
        }
    }
    Ok(())
}

impl LocalSnapshot {
    pub(in crate::gpui_shell::workspace) fn prepare(mut self) -> io::Result<(Session, Archive)> {
        use sha2::{Digest, Sha256};

        if self.session.tabs.len() != self.outputs.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "output tab count mismatch"));
        }
        let mut panes = BTreeMap::new();
        let mut seen = std::collections::HashSet::new();
        for (tab, outputs) in self.session.tabs.iter_mut().zip(self.outputs) {
            let count = tab.layout.as_ref().map_or(1, crate::session::LayoutSession::pane_count);
            if count != outputs.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "output pane count mismatch",
                ));
            }
            tab.output_refs.clear();
            for (pane_id, records) in outputs {
                if !seen.insert(pane_id) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate output pane",
                    ));
                }
                crate::recent_output::RecentOutput::validate_records(&records)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if records.is_empty() {
                    tab.output_refs.push(String::new());
                    continue;
                }
                let bytes = serde_json::to_vec(&records)?;
                let digest = Sha256::digest(&bytes);
                let hash = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
                let id = format!("{}-{pane_id}-{hash}", self.window_id);
                if panes.insert(id.clone(), records).is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate output pane",
                    ));
                }
                tab.output_refs.push(id);
            }
        }
        Ok((self.session, Archive::new(panes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::TabSession;

    fn snapshot(records: Vec<CommandRecord>) -> LocalSnapshot {
        LocalSnapshot {
            session: Session::new(
                0,
                vec![
                    TabSession::single("C:/same".into(), None, None),
                    TabSession::single("C:/same".into(), None, None),
                ],
            ),
            outputs: vec![vec![(11, records.clone())], vec![(12, records)]],
            window_id: 7,
        }
    }

    fn records(label: &str) -> Vec<CommandRecord> {
        use nebula_terminal::{
            event::VoidListener,
            grid::Dimensions,
            term::{Config, Term},
            vte::ansi,
        };
        struct Size;
        impl Dimensions for Size {
            fn total_lines(&self) -> usize {
                4
            }
            fn screen_lines(&self) -> usize {
                4
            }
            fn columns(&self) -> usize {
                32
            }
        }
        let mut term = Term::new(Config::default(), &Size, VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(&mut term, label.as_bytes());
        let mut recent = crate::recent_output::RecentOutput::default();
        recent.begin(term.grid());
        let mut records = recent.snapshot(term.grid());
        for record in &mut records {
            record.captured_at = 0;
        }
        records
    }

    #[test]
    fn oversized_combined_output_evicts_old_records_without_losing_pane_ownership() {
        let mut windows = Vec::new();
        for window in 0..96 {
            let mut record = serde_json::to_value(records("retained").remove(0)).unwrap();
            // 合成合法的大记录；不同窗格的内容相同也不能共用身份。
            let row = record["prompt"]["rows"][0].clone();
            record["prompt"]["rows"] = serde_json::Value::Array(vec![row; 32]);
            let mut record: CommandRecord = serde_json::from_value(record).unwrap();
            // 故意与窗口遍历顺序相反，防止按窗格位置而非时间淘汰。
            record.captured_at = 96 - window;
            let mut captured = snapshot(vec![record; 5]);
            captured.window_id = window;
            windows.push((window == 95, captured));
        }
        let (session, mut archive) = prepare_windows(windows).unwrap();
        let path = tempfile::tempdir().unwrap();
        archive.write_to(&path.path().join("output.json")).unwrap();
        assert!(
            std::fs::metadata(path.path().join("output.json")).unwrap().len() <= 16 * 1024 * 1024
        );
        let session = session.unwrap();
        assert!(!session.tabs[0].output_refs[0].is_empty(), "newest window survives");
        assert!(session.tabs.last().unwrap().output_refs[0].is_empty(), "oldest window is evicted");
        let mut count = 0;
        for tab in &session.tabs {
            for reference in &tab.output_refs {
                if !reference.is_empty() {
                    let restored = archive.take_records(reference).expect("exact pane reference");
                    count += restored.len();
                    assert!(!restored.is_empty());
                }
            }
        }
        assert!(count > 0 && count < 960, "must evict records, not reject the save");
    }

    #[test]
    fn combined_windows_keep_active_order_and_exact_output_ownership() {
        let first = snapshot(records("first"));
        let mut second = snapshot(records("second"));
        second.window_id = 8;
        second.session.active_tab = 1;
        let (session, mut archive) = prepare_windows(vec![(false, first), (true, second)]).unwrap();
        let session = session.unwrap();
        assert_eq!(session.active_tab, 3);
        let windows = session.into_update_windows().unwrap();
        assert_eq!(windows.len(), 2);
        for (window, label) in windows.iter().zip(["first", "second"]) {
            for tab in &window.tabs {
                let restored = archive.take_records(&tab.output_refs[0]).unwrap();
                assert_eq!(
                    serde_json::to_value(restored).unwrap(),
                    serde_json::to_value(records(label)).unwrap()
                );
            }
        }
    }

    #[test]
    fn duplicated_window_identity_cannot_overwrite_another_windows_output() {
        assert!(
            prepare_windows(vec![
                (false, snapshot(records("first"))),
                (true, snapshot(records("second"))),
            ])
            .is_err()
        );
        assert!(prepare_windows(Vec::new()).unwrap().0.is_none());
    }

    #[test]
    fn local_snapshot_refs_are_stable_and_distinct_for_same_directory_panes() {
        let records = records("same");
        let (first, mut archive) = snapshot(records.clone()).prepare().unwrap();
        let (again, _) = snapshot(records).prepare().unwrap();
        assert_eq!(first.tabs[0].output_refs, again.tabs[0].output_refs);
        assert_ne!(first.tabs[0].output_refs, first.tabs[1].output_refs);
        assert!(archive.take_records(&first.tabs[0].output_refs[0]).is_some());
        assert!(archive.take_records(&first.tabs[1].output_refs[0]).is_some());
        assert!(!serde_json::to_string(&first).unwrap().contains(&first.tabs[0].output_refs[0]));
    }

    #[test]
    fn changed_output_gets_new_reference_without_mutating_previous_snapshot() {
        let (before, _) = snapshot(records("before")).prepare().unwrap();
        let (after, _) = snapshot(records("after")).prepare().unwrap();
        assert_ne!(before.tabs[0].output_refs, after.tabs[0].output_refs);
    }

    #[test]
    fn empty_output_keeps_leaf_position_without_persisting_a_record() {
        let (session, _) = snapshot(Vec::new()).prepare().unwrap();
        assert_eq!(session.tabs[0].output_refs, [""]);
        assert_eq!(session.tabs[1].output_refs, [""]);
    }

    #[test]
    fn duplicate_empty_panes_are_rejected_before_publication() {
        let mut captured = snapshot(Vec::new());
        captured.outputs[1][0].0 = 11;
        assert!(captured.prepare().is_err());
    }

    #[test]
    fn invalid_record_is_rejected_before_reference_publication() {
        let mut captured = snapshot(records("safe"));
        captured.outputs[0][0].1 = vec![records("safe").remove(0); 6];
        assert!(captured.prepare().is_err());
    }

    #[test]
    fn mismatched_leaf_capture_does_not_shift_other_pane_references() {
        let mut captured = snapshot(records("safe"));
        captured.outputs[0].push((13, records("other")));
        assert!(captured.prepare().is_err());
    }

    #[test]
    fn mismatched_capture_does_not_publish_misattributed_references() {
        let mut snapshot = snapshot(Vec::new());
        snapshot.outputs.pop();
        assert!(snapshot.prepare().is_err());
    }
}
