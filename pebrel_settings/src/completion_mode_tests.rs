use super::*;

#[test]
fn modes_round_trip_preserve_other_settings_and_reset_to_inline() {
    for mode in CompletionStyleName::ALL {
        let input = "accept=right\nunknown=keep\ncompletion_style=inline\n";
        let saved = apply_updates(input, &[("completion_style", mode.settings_value().into())]);
        assert_eq!(
            RuntimeSettings::from_raw(&RawSettings::from_text(&saved)).completion_style,
            mode
        );
        assert!(saved.contains("unknown=keep"));
        assert!(saved.contains("accept=right"));
        assert_eq!(CompletionStyleName::from_settings(mode.settings_value()), Some(mode));
    }
    assert_eq!(CompletionStyleName::Hybrid.cycle(), CompletionStyleName::Inline);
    assert_eq!(CompletionStyleName::from_settings("ghost"), Some(CompletionStyleName::Inline));
    assert_eq!(CompletionStyleName::from_settings("list"), Some(CompletionStyleName::Popup));
    assert_eq!(
        RuntimeSettings::from_raw(&RawSettings::from_text("completion_style=invalid"))
            .completion_style,
        CompletionStyleName::Inline
    );
}
