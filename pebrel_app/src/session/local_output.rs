//! 仅自动恢复文档携带输出引用；共享 Session 编码不含本机身份。

use super::Session;

pub(super) fn encode(session: &Session) -> serde_json::Result<String> {
    #[derive(serde::Serialize)]
    struct LocalSession<'a> {
        #[serde(flatten)]
        session: &'a Session,
        local_output_refs: Vec<&'a [String]>,
    }
    serde_json::to_string(&LocalSession {
        session,
        local_output_refs: session.tabs.iter().map(|tab| tab.output_refs.as_slice()).collect(),
    })
}

pub(super) fn parse(data: &str) -> Option<Session> {
    let mut session = super::parse(data)?;
    // 引用损坏仅丢弃历史，不使仍有效的布局与 AI 身份无法恢复。
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    if let Some(refs) = value
        .get("local_output_refs")
        .and_then(|refs| serde_json::from_value::<Vec<Vec<String>>>(refs.clone()).ok())
    {
        let mut seen = std::collections::HashSet::new();
        let valid = refs.len() == session.tabs.len()
            && refs.iter().zip(&session.tabs).all(|(ids, tab)| {
                let leaves = tab.layout.as_ref().map_or(1, super::LayoutSession::pane_count);
                (ids.is_empty() || ids.len() == leaves)
                    && ids.iter().all(|id| {
                        id.is_empty()
                            || (id.len() <= 128
                                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                                && seen.insert(id))
                    })
            });
        if valid {
            for (tab, ids) in session.tabs.iter_mut().zip(refs) {
                tab.output_refs = ids;
            }
        }
    }
    Some(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{TabSession, combine_sessions};

    fn saved(id: &str) -> Session {
        let mut tab = TabSession::single("C:/same-directory".into(), None, None);
        tab.output_refs = vec![id.into()];
        Session::new(0, vec![tab])
    }

    #[test]
    fn local_output_refs_survive_local_save_but_never_workspace_export_or_import() {
        let original = saved("pane-one");
        let encoded = encode(&original).unwrap();
        assert_eq!(parse(&encoded).unwrap().tabs[0].output_refs, ["pane-one"]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.json");
        crate::session::save_to(&path, &original).unwrap();
        let exported = std::fs::read_to_string(&path).unwrap();
        assert!(!exported.contains("pane-one"));
        // 即使输入的是本机自动存档，手动工作区导入仍不能带入引用。
        std::fs::write(&path, encoded).unwrap();
        assert!(crate::session::load_from(&path).unwrap().tabs[0].output_refs.is_empty());
    }

    #[test]
    fn local_output_refs_follow_tabs_when_combining_and_restoring_windows() {
        let combined =
            combine_sessions([(false, saved("pane-one")), (true, saved("pane-two"))]).unwrap();
        let windows = parse(&encode(&combined).unwrap()).unwrap().into_update_windows().unwrap();
        assert_eq!(windows[0].tabs[0].output_refs, ["pane-one"]);
        assert_eq!(windows[1].tabs[0].output_refs, ["pane-two"]);
    }

    #[test]
    fn malformed_local_output_refs_do_not_break_layout_recovery() {
        let mut value = serde_json::to_value(saved("unused")).unwrap();
        for invalid in [
            serde_json::json!([["../escape"]]),
            serde_json::json!([["one", "two"]]),
            serde_json::json!([["same"], ["extra-tab"]]),
            serde_json::json!("broken"),
        ] {
            value["local_output_refs"] = invalid;
            let session = parse(&value.to_string()).unwrap();
            assert_eq!(session.tabs.len(), 1);
            assert!(session.tabs[0].output_refs.is_empty());
        }
    }
}
