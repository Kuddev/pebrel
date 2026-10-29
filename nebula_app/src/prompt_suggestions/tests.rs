use super::*;

#[test]
fn prediction_prompt_preserves_the_pinned_codex_source_verbatim() {
    use sha2::{Digest, Sha256};

    let source = include_bytes!("../../res/prompts/codex_prompt_suggestion.txt");
    let hash: String = Sha256::digest(source).iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hash, "3728870393e8d9776f983db00b1b4bea094b541d56478807d204f7e650232f4e");
    assert!(INSTRUCTIONS.starts_with(std::str::from_utf8(source).unwrap()));
    assert_eq!(
        &INSTRUCTIONS[source.len()..],
        "\n\nReturn only JSON matching this schema: {\"type\":\"object\",\"properties\":{\"suggestion\":{\"type\":[\"string\",\"null\"]}},\"required\":[\"suggestion\"],\"additionalProperties\":false}."
    );
}

fn identity() -> Identity {
    Identity {
        agent: "codex".into(),
        session: "s1".into(),
        turn: "t1".into(),
        input_revision: 4,
        provider: "completion-provider".into(),
        language: "zh-CN".into(),
    }
}

#[test]
fn late_result_cannot_restore_dismissed_or_replaced_candidate() {
    let mut state = State::default();
    let first = identity();
    let generation = state.begin(first.clone()).unwrap();
    state.invalidate();
    state.finish(generation, &first, Some("旧结果".into()));
    assert_eq!(state.candidate(&first, true), None);
    assert!(state.begin(first.clone()).is_none());
    let mut second = first.clone();
    second.turn = "t2".into();
    let next = state.begin(second.clone()).unwrap();
    state.finish(next, &second, Some("补一下边界测试".into()));
    state.finish(generation, &first, Some("迟到结果".into()));
    assert_eq!(state.candidate(&second, true), Some("补一下边界测试"));
}

#[test]
fn accepting_needs_same_owner_input_and_settings_and_is_consumed_once() {
    let mut state = State::default();
    let original = identity();
    let generation = state.begin(original.clone()).unwrap();
    state.finish(generation, &original, Some("可以，跑一下测试".into()));
    assert_eq!(state.take(&original, false), None);
    for field in 0..6 {
        let mut changed = original.clone();
        match field {
            0 => changed.agent = "claude".into(),
            1 => changed.session = "s2".into(),
            2 => changed.turn = "t2".into(),
            3 => changed.input_revision += 1,
            4 => changed.provider = "another".into(),
            _ => changed.language = "en".into(),
        }
        assert_eq!(state.take(&changed, true), None);
    }
    assert_eq!(state.take(&original, true).as_deref(), Some("可以，跑一下测试"));
    assert_eq!(state.take(&original, true), None);
}

#[test]
fn output_is_a_bounded_single_user_message_or_abstention() {
    for text in [
        "null",
        "{}",
        r#"{"suggestion":null}"#,
        r#"{"suggestion":3}"#,
        r#"{"suggestion":"好","explanation":"extra"}"#,
        r#"{"suggestion":"a\nb"}"#,
        r#"{"suggestion":"\u001b[31m"}"#,
        r#"{"suggestion":"a\u2028b"}"#,
    ] {
        assert_eq!(parse(text), None, "{text}");
    }
    assert_eq!(
        parse(r#"{"suggestion":"  先把中文输入测完  "}"#).as_deref(),
        Some("先把中文输入测完")
    );
    for size in [240, 241] {
        let text = "中".repeat(size);
        assert_eq!(
            parse(&serde_json::json!({"suggestion":text}).to_string()),
            (size == 240).then_some(text)
        );
    }
}

#[test]
fn context_preserves_user_voice_without_tools_or_unbounded_history() {
    let messages = vec![
        Message { role: "tool".into(), text: "private tool output".into() },
        Message { role: "user".into(), text: "先把光标弄好，别动别的".into() },
        Message { role: "assistant".into(), text: "中".repeat(20_000) },
        Message { role: "user".into(), text: "可以，继续测".into() },
    ];
    let bounded = context(&messages);
    assert_eq!(bounded.len(), 3);
    assert_eq!(bounded[0].text, messages[1].text);
    assert_eq!(bounded[2].text, messages[3].text);
    assert_eq!(bounded[1].text.chars().count(), 3000);
    assert!(context(&vec![messages[1].clone(); 20]).len() <= 8);
}

#[test]
fn excerpts_keep_initial_request_and_final_correction_on_utf8_boundaries() {
    let original = format!("先测中文{}最后改成仅侧边 tab", "🦀".repeat(20_000));
    let bounded = context(&[Message { role: "user".into(), text: original.clone() }]);
    assert!(bounded[0].text.starts_with("先测中文"));
    assert!(bounded[0].text.ends_with("最后改成仅侧边 tab"));
    assert_eq!(bounded[0].text.chars().count(), 3000);
    for limit in 0..40 {
        let text = excerpt(&original, limit);
        assert_eq!(text.chars().count(), limit);
    }
}

#[test]
fn voice_evidence_survives_long_answers_without_learning_the_assistants_style() {
    let mut messages = vec![Message { role: "user".into(), text: "先弄好光标".into() }];
    messages.extend((0..12).map(|_| Message {
        role: "assistant".into(),
        text: "The implementation has been validated in accordance with the plan.".into(),
    }));
    messages.push(Message { role: "user".into(), text: "继续测".into() });
    assert!(!context(&messages).iter().any(|message| message.text == "先弄好光标"));
    assert_eq!(voice_examples(&messages), ["先弄好光标", "继续测"]);
    messages.extend(
        (0..20)
            .map(|i| Message { role: "user".into(), text: format!("{i}{}", "中".repeat(4000)) }),
    );
    let voice = voice_examples(&messages);
    assert!(voice.len() <= 6);
    assert!(voice.iter().map(|text| text.chars().count()).sum::<usize>() <= 1600);
    assert!(voice.iter().all(|text| text.chars().count() <= 360));
    assert_eq!(decode(r#"{"suggestion":null}"#), Ok(None));
    assert_eq!(decode("{}"), Err(()));
}
