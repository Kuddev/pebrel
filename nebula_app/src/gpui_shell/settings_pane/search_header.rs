use super::*;

impl SettingsPane {
    pub(super) fn matching_settings_sections(&self, cx: &App) -> Vec<usize> {
        let query = self.settings_search_input.read(cx).value();
        let mut sections = matching_sections(&query, crate::gpui_shell::config::ui_language(cx));
        if self.keymap_matches_query(&query) && !sections.contains(&7) {
            sections.push(7);
        }
        sections
    }
    pub(super) fn update_settings_search(&mut self, cx: &mut Context<Self>) {
        let searching = !self.settings_search_input.read(cx).value().trim().is_empty();
        if searching {
            self.search_origin_section.get_or_insert(self.active_section);
            let sections = self.matching_settings_sections(cx);
            if let Some(index) = sections.first() {
                self.active_section = *index;
                self.font_picker_open = false;
                self.bg_picker_open = false;
            }
        } else if let Some(origin) = self.search_origin_section.take() {
            self.active_section = origin;
        }
        cx.notify();
    }

    pub(super) fn render_nav_search(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let focused = self.settings_search_input.read(cx).focus_handle(cx).is_focused(window);
        let line =
            if focused { cx.theme().link } else { crate::gpui_shell::theme::settings_hairline(cx) };
        div()
            .mx_1()
            .mb(px(12.0))
            .border_b_1()
            .border_color(line)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.keymap_capture.take().is_some() {
                        this.keymap_capture_preview.clear();
                        cx.notify();
                    }
                }),
            )
            .child(
                Input::new(&self.settings_search_input)
                    .appearance(false)
                    .focus_bordered(false)
                    .w_full()
                    .cleanable(true)
                    .prefix(
                        Icon::new(IconName::Search)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .aria_label(language.pick("在全部设置中搜索", "Search all settings")),
            )
            .into_any_element()
    }
}

pub(super) fn matching_sections(query: &str, language: crate::display::UiLanguage) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return visible_nav_sections().collect();
    }

    let tokens = query.split_whitespace().collect::<Vec<_>>();
    let mut matches = visible_nav_sections()
        .filter_map(|index| {
            let haystack = format!(
                "{} {}",
                SECTION_SEARCH_TERMS[index],
                section_label(index, language).to_lowercase()
            );
            let words = haystack
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            let mut score = 0usize;
            for token in &tokens {
                score = score.checked_add(score_token(token, &haystack, &words)?)?;
            }
            if haystack.contains(&query) {
                score += 1_500;
            }
            Some((index, score))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score.cmp(left_score).then_with(|| left_index.cmp(right_index))
    });
    matches.into_iter().map(|(index, _)| index).collect()
}

fn score_token(token: &str, haystack: &str, words: &[&str]) -> Option<usize> {
    if words.contains(&token) {
        return Some(1_000);
    }
    if words.iter().any(|word| word.starts_with(token)) {
        return Some(850);
    }
    if haystack.contains(token) {
        return Some(700);
    }

    if token.chars().count() >= 2 {
        if let Some(score) = words.iter().filter_map(|word| subsequence_score(token, word)).max() {
            return Some(score);
        }
    }

    if token.len() >= 4
        && token.is_ascii()
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        let allowed = if token.len() >= 8 { 2 } else { 1 };
        return words
            .iter()
            .filter(|word| word.is_ascii())
            .filter_map(|word| {
                let distance =
                    edit_distance_with_limit(token.as_bytes(), word.as_bytes(), allowed)?;
                Some(420usize.saturating_sub(distance * 60))
            })
            .max();
    }
    None
}

fn subsequence_score(needle: &str, word: &str) -> Option<usize> {
    let mut needle = needle.chars();
    let mut next = needle.next()?;
    let mut matched = 0usize;
    let mut span = 0usize;
    for ch in word.chars() {
        span += usize::from(matched > 0);
        if ch == next {
            matched += 1;
            if let Some(ch) = needle.next() {
                next = ch;
            } else {
                return Some(520usize.saturating_sub(span.saturating_sub(matched) * 8));
            }
        }
    }
    None
}

fn edit_distance_with_limit(left: &[u8], right: &[u8], limit: usize) -> Option<usize> {
    if left.len().abs_diff(right.len()) > limit {
        return None;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_byte) in left.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_min = current[0];
        for (right_index, right_byte) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_byte != right_byte));
            row_min = row_min.min(current[right_index + 1]);
        }
        if row_min > limit {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    (previous[right.len()] <= limit).then_some(previous[right.len()])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_filters_navigation_without_an_extra_click() {
        let en = crate::display::UiLanguage::EnUs;
        let zh = crate::display::UiLanguage::ZhCn;
        assert_eq!(matching_sections(" FONT ", en), vec![1]);
        assert_eq!(matching_sections("字体", zh), vec![1]);
        assert_eq!(matching_sections("quick terminal", en), vec![7]);
        assert!(matching_sections("clod backup", en).is_empty());
        assert!(matching_sections("webdav", en).is_empty());
        assert!(matching_sections("123", zh).is_empty());
        assert_eq!(matching_sections("透度", zh).first(), Some(&1));
        assert!(matching_sections("no such setting", en).is_empty());
        assert_eq!(matching_sections("", en), visible_nav_sections().collect::<Vec<_>>());
        assert!(!matching_sections("AI", en).contains(&3));
    }

    #[test]
    fn ai_toast_search_opens_the_terminal_alert_controls() {
        for query in ["AI 消息弹窗", "AI消息通知", "右下角", "ai toast", "notifications"]
        {
            for language in [crate::display::UiLanguage::ZhCn, crate::display::UiLanguage::EnUs] {
                assert_eq!(matching_sections(query, language), vec![2], "{query}");
            }
        }
    }
}
