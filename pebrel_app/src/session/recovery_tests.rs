//! Saved recovery contracts for every registered interactive CLI.
use super::*;
use crate::ai_agents::AgentKind;

fn expected_resume(kind: AgentKind, id: &str) -> Option<String> {
    use AgentKind::*;
    Some(match kind {
        Claude => format!("claude --resume {id}"),
        Codex => format!("codex resume {id}"),
        Gemini => format!("gemini --resume {id}"),
        Amp => format!("amp threads continue {id}"),
        OpenCode => format!("opencode --session {id}"),
        Copilot => format!("copilot --resume {id}"),
        Cursor => format!("agent --resume={id}"),
        Pi => format!("pi --session {id}"),
        Antigravity => format!("agy --conversation {id}"),
        Grok => format!("grok --resume {id}"),
        CodeBuddy => format!("codebuddy --resume {id}"),
        OhMyPi => format!("omp --resume {id}"),
        Kimi => format!("kimi --session {id}"),
        CommandCode => format!("command-code --resume {id}"),
        Aider | Goose | Droid | Auggie | Hermes | Vibe | Qwen | Cline | Devin | Kiro | Kilo
        | Qoder | Maki | Trae => return None,
    })
}

fn leaf(kind: AgentKind, id: &str) -> LayoutSession {
    LayoutSession::Pane {
        cwd: "/same-project".into(),
        custom_name: None,
        launch: None,
        agent: Some(AgentSession {
            source: kind.slug().into(),
            session_id: Some(id.into()),
            session_file: None,
        }),
    }
}

#[test]
fn registered_clis_keep_native_ids_in_their_own_split_across_saves() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("session.json");
    let upper_id = "2e7a4172-69d4-4075-b6cb-48ef244e4323";
    let lower_id = "5e95c81e-dd38-41b0-97de-3dc49834a447";
    let switched_id = "c594a477-9b34-4811-95fe-bd7b5de26081";
    for kind in AgentKind::ALL {
        let mut tab = TabSession::single("/same-project".into(), None, None);
        tab.layout = Some(LayoutSession::Split {
            axis: SplitAxis::TopBottom,
            ratio_permille: 500,
            first: Box::new(leaf(kind, upper_id)),
            second: Box::new(leaf(kind, lower_id)),
        });
        let mut session = Session::new(0, vec![tab]);
        for expected_upper in [upper_id, switched_id, switched_id] {
            save_to(&path, &session).unwrap();
            session = load_from(&path).unwrap();
            assert_eq!(session.tabs.len(), 1);
            let layout = session.tabs[0].layout.as_mut().unwrap();
            let leaves = layout.leaves();
            assert_eq!(leaves.len(), 2);
            for (pane, id) in leaves.into_iter().zip([expected_upper, lower_id]) {
                let LayoutSession::Pane { agent: Some(saved), .. } = pane else {
                    panic!("{kind:?} lost a split conversation");
                };
                assert_eq!(AgentKind::parse(&saved.source), Some(kind));
                assert_eq!(saved.session_id.as_deref(), Some(id), "{kind:?}");
                assert_eq!(saved.resume_command(), expected_resume(kind, id), "{kind:?}");
            }
            if expected_upper == upper_id {
                let LayoutSession::Split { first, .. } = layout else {
                    panic!("lost split layout");
                };
                **first = leaf(kind, switched_id);
            }
        }
    }
}

#[test]
fn live_recovery_cases_cover_the_registered_cli_entry_points() {
    use std::collections::BTreeMap;
    let cases: BTreeMap<String, Vec<String>> =
        toml::from_str(include_str!("../../../scripts/conformance/agent_recovery/entries.toml"))
            .unwrap();
    assert_eq!(cases.len(), AgentKind::ALL.len());
    for kind in AgentKind::ALL {
        let commands = &cases[kind.slug()];
        assert!((1..=2).contains(&commands.len()), "{kind:?}");
        for command in commands {
            assert_eq!(AgentKind::parse_command(command), Some(kind), "{command}");
        }
    }
}
