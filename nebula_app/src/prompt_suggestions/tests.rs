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
fn long_answer_sequences_keep_the_latest_user_and_recent_answers_in_order() {
    let mut messages = vec![
        Message { role: "user".into(), text: "old request".into() },
        Message { role: "user".into(), text: "先测中文喵".into() },
    ];
    messages.extend((0..12).map(|index| Message {
        role: "assistant".into(),
        text: format!("answer {index}: {} end {index}", "长".repeat(5000)),
    }));
    messages.push(Message { role: "tool".into(), text: "private tool output".into() });
    let bounded = context(&messages);
    assert_eq!(bounded[0], messages[1]);
    assert!(bounded.last().unwrap().text.ends_with("end 11"));
    assert!(bounded.len() <= 8);
    assert!(bounded.iter().map(|message| message.text.chars().count()).sum::<usize>() <= 12000);
    assert_eq!(bounded.iter().filter(|message| message.role == "user").count(), 1);
    for pair in bounded.windows(2).filter(|pair| pair[0].role == "assistant") {
        let index = |message: &Message| {
            message
                .text
                .split_whitespace()
                .nth(1)
                .unwrap()
                .trim_end_matches(':')
                .parse::<u32>()
                .unwrap()
        };
        assert!(index(&pair[0]) < index(&pair[1]));
    }
    assert_eq!(context(&bounded), bounded, "transport clipping must not evict the retained user");
}

#[test]
fn abstention_is_distinct_from_invalid_json() {
    assert_eq!(decode(r#"{"suggestion":null}"#), Ok(None));
    assert_eq!(decode("{}"), Err(()));
}
