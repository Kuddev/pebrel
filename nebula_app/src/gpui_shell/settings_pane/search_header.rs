use super::*;
use gpui::{Animation, AnimationExt as _, relative};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Default)]
pub(super) struct SearchFocus {
    focused: bool,
    generation: usize,
    from: f32,
    progress: Rc<Cell<f32>>,
}

impl SearchFocus {
    pub(super) fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.from = self.progress.get();
            self.focused = focused;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    fn underline(&self, color: Hsla, reduce_motion: bool) -> gpui::AnyElement {
        let target = if self.focused { 1.0 } else { 0.0 };
        let line = div().absolute().bottom_0().h(px(1.0)).bg(color);
        if self.from == target || reduce_motion {
            self.progress.set(target);
            return line
                .left(relative((1.0 - target) * 0.5))
                .w(relative(target))
                .into_any_element();
        }
        let from = self.from;
        let progress = self.progress.clone();
        line.with_animation(
            ("settings-search-focus-line", self.generation),
            Animation::new(Duration::from_millis(180)).with_easing(crate::gpui_shell::motion::ease),
            move |line, delta| {
                let value = from + (target - from) * delta;
                progress.set(value);
                line.left(relative((1.0 - value) * 0.5)).w(relative(value))
            },
        )
        .into_any_element()
    }
}

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
        _window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        div()
            .relative()
            .mx_1()
            .mb(px(12.0))
            .pb(px(1.0))
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
                            .size(px(SETTINGS_NAV_ICON_SIZE))
                            .text_color(cx.theme().muted_foreground),
                    )
                    .aria_label(language.pick("在全部设置中搜索", "Search all settings")),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(px(1.0))
                    .bg(crate::gpui_shell::theme::settings_hairline(cx)),
            )
            .child(self.settings_search_focus.underline(cx.theme().link, cx.reduce_motion()))
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
    fn focus_reversal_starts_at_the_displayed_line_and_reduced_motion_finishes_immediately() {
        let mut focus = SearchFocus::default();
        focus.set_focused(true);
        focus.progress.set(0.4);
        focus.set_focused(false);
        assert_eq!(focus.from, 0.4);
        assert_eq!(focus.generation, 2);
        let _ = focus.underline(gpui::hsla(0.0, 0.0, 0.0, 1.0), true);
        assert_eq!(focus.progress.get(), 0.0);
        focus.set_focused(true);
        let _ = focus.underline(gpui::hsla(0.0, 0.0, 0.0, 1.0), true);
        assert_eq!(focus.progress.get(), 1.0);
    }

    #[test]
    fn search_filters_navigation_without_an_extra_click() {
        let en = crate::display::UiLanguage::EnUs;
        let zh = crate::display::UiLanguage::ZhCn;
        assert_eq!(matching_sections(" FONT ", en), vec![1]);
        assert_eq!(matching_sections("字体", zh), vec![1]);
        assert_eq!(matching_sections("quick terminal", en), vec![7]);
        assert_eq!(matching_sections("clod backup", en), vec![9]);
        assert_eq!(matching_sections("webdav", en), vec![9]);
        assert_eq!(matching_sections("123", zh), vec![9]);
        assert_eq!(matching_sections("手机", zh), vec![MOBILE_SECTION]);
        assert_eq!(matching_sections("pairing", en), vec![MOBILE_SECTION]);
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

    #[test]
    fn ctrl_wheel_zoom_search_opens_the_terminal_appearance_controls() {
        for query in ["滚轮", "ctrl wheel", "zoom"] {
            for language in [crate::display::UiLanguage::ZhCn, crate::display::UiLanguage::EnUs] {
                assert_eq!(matching_sections(query, language), vec![1], "{query}");
            }
        }
    }

    #[test]
    fn shader_and_media_search_find_the_appearance_controls() {
        for query in
            ["WGSL", "shader", "terminal effects", "终端效果", "着色器", "视频", "GIF", "动图"]
        {
            for language in [crate::display::UiLanguage::ZhCn, crate::display::UiLanguage::EnUs] {
                assert_eq!(matching_sections(query, language).first(), Some(&1), "{query}");
            }
        }
    }
}
