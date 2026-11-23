//! 作者图片复用 GPUI 的异步图片加载与缓存，不在绘制回调里访问网络。

use super::*;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

fn avatar_url(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let (account, domain) = email.rsplit_once('@')?;
    if account.is_empty() || domain.is_empty() {
        return None;
    }
    if domain == "users.noreply.github.com" {
        if let Some((id, _)) = account.split_once('+') {
            if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Some(format!("https://avatars.githubusercontent.com/u/{id}?s=48"));
            }
        } else if account.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') {
            return Some(format!("https://github.com/{account}.png?size=48"));
        }
    }
    // Gravatar 接受规范化邮箱的 SHA-256；原始邮箱不进入请求 URL。
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(email.as_bytes()) {
        let _ = write!(&mut hash, "{byte:02x}");
    }
    Some(format!("https://www.gravatar.com/avatar/{hash}?s=48&d=404"))
}

pub(super) fn author_avatar(
    commit: &crate::display::side_panel::GitCommit,
    color: gpui::Hsla,
    surface: gpui::Hsla,
) -> gpui::AnyElement {
    let initials: String = commit
        .author
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    let initials: SharedString = if initials.is_empty() { "·".into() } else { initials.into() };
    let placeholder = move || {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(surface)
            .text_color(color)
            .text_size(px(8.0))
            .font_weight(FontWeight::MEDIUM)
            .child(initials.clone())
            .into_any_element()
    };
    let image = match avatar_url(&commit.author_email) {
        Some(url) => img(url)
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_loading(placeholder.clone())
            .with_fallback(placeholder)
            .into_any_element(),
        None => placeholder(),
    };
    div()
        .id(SharedString::from(format!("git-author-{}", commit.full_hash)))
        .size_full()
        .rounded_full()
        .border_1()
        .border_color(color)
        .bg(surface)
        .overflow_hidden()
        .tooltip({
            let author = commit.author.clone();
            move |window, cx| Tooltip::new(author.clone()).build(window, cx)
        })
        .child(image)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::avatar_url;

    #[test]
    fn github_noreply_uses_account_id_and_other_emails_use_a_normalized_hash() {
        assert_eq!(
            avatar_url("42+alice@users.noreply.github.com").as_deref(),
            Some("https://avatars.githubusercontent.com/u/42?s=48")
        );
        assert_eq!(avatar_url(" ALICE@example.com "), avatar_url("alice@example.com"));
        assert!(!avatar_url("alice@example.com").unwrap().contains("alice@example.com"));
        assert_eq!(avatar_url(""), None);
    }
}
