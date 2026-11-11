//! Recovery for local `file:` OSC 8 URIs corrupted by UTF-8 `0x9C` (C1 `ST`) collisions.
//!
//! CLI tools that emit unencoded UTF-8 inside OSC 8 hyperlinks and pass output through
//! byte-oriented ANSI parsers can treat `0x9C` (found in characters like `有` `E6 9C 89` or
//! `最` `E6 9C 80`) as C1 `ST`, either truncating the URI with trailing `U+FFFD` or leaving
//! interior `U+FFFD` in a path segment.
//!
//! Hover/render paths use [`repair_corrupted_file_uri_with_text`] purely in memory (zero
//! filesystem I/O). Background click classification falls back to
//! [`repair_corrupted_local_path_on_disk`] when a generic label omits the filename.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

const MAX_RECOVERY_DIR_ENTRIES: usize = 4096;
const MAX_INTERIOR_GAP_CHARS: usize = 8;

/// Fast O(N) check whether `uri` contains `U+FFFD` (raw or `%EF%BF%BD` percent-encoded).
#[inline]
pub fn has_utf8_replacement_corruption(uri: &str) -> bool {
    uri.contains('\u{FFFD}')
        || uri.as_bytes().windows(9).any(|window| window.eq_ignore_ascii_case(b"%ef%bf%bd"))
}

/// Pure in-memory repair of a corrupted local `file:` URI using the visible cell text of the
/// hyperlink. Never performs filesystem I/O.
pub fn repair_corrupted_file_uri_with_text(raw_uri: &str, visible_text: &str) -> Option<String> {
    if !has_utf8_replacement_corruption(raw_uri) {
        return None;
    }
    let (prefix, decoded_path) =
        local_file_uri_prefix_and_path(super::extract_link_target(raw_uri))?;
    if !decoded_path.contains('\u{FFFD}') {
        return None;
    }

    let clean_vis =
        super::trim_target_punctuation(visible_text.trim().trim_start_matches('\u{FFFD}'));
    if clean_vis.is_empty() {
        return None;
    }

    let path_truncated = decoded_path.ends_with('\u{FFFD}');
    let candidates = collect_visible_candidates(clean_vis, &decoded_path);

    let mut repaired = String::with_capacity(decoded_path.len() + 16);
    for part in decoded_path.split_inclusive(['/', '\\']) {
        let has_sep = part.ends_with(['/', '\\']);
        let (seg, sep) =
            if has_sep { (&part[..part.len() - 1], &part[part.len() - 1..]) } else { (part, "") };
        if seg.contains('\u{FFFD}') {
            repaired.push_str(&pick_unique_match(
                seg,
                candidates.iter().copied(),
                !has_sep && path_truncated,
            )?);
        } else {
            repaired.push_str(seg);
        }
        repaired.push_str(sep);
    }

    let encoded_path = repaired.replace('%', "%25");
    Some(format!("{prefix}{encoded_path}"))
}

/// Background-thread filesystem fallback for local paths containing `U+FFFD` whose visible
/// hyperlink label omitted the filename (for example `[Open report](file:///...)`).
pub(super) fn repair_corrupted_local_path_on_disk(path: &Path) -> Option<PathBuf> {
    let lossy = path.to_string_lossy();
    if !lossy.contains('\u{FFFD}') || lossy.starts_with(r"\\") {
        return None;
    }

    let mut current = PathBuf::new();
    let mut repaired_any = false;
    let components: Vec<Component<'_>> = path.components().collect();

    for (index, component) in components.iter().enumerate() {
        match component {
            Component::Normal(os_seg) => {
                let seg = os_seg.to_string_lossy();
                if !seg.contains('\u{FFFD}') || current.join(os_seg).exists() {
                    current.push(os_seg);
                    if !current.exists() {
                        return None;
                    }
                    continue;
                }
                let is_last = index + 1 == components.len();
                let entries = std::fs::read_dir(&current).ok()?;
                let mut names = Vec::new();
                for (count, entry) in entries.enumerate() {
                    if count >= MAX_RECOVERY_DIR_ENTRIES {
                        return None;
                    }
                    if let Ok(entry) = entry
                        && let Ok(name) = entry.file_name().into_string()
                    {
                        names.push(name);
                    }
                }
                let matched = pick_unique_match(&seg, names.iter().map(String::as_str), is_last)?;
                current.push(OsString::from(matched));
                repaired_any = true;
            },
            other => current.push(other.as_os_str()),
        }
    }

    (repaired_any && current.exists()).then_some(current)
}

fn local_file_uri_prefix_and_path(target: &str) -> Option<(&str, String)> {
    let rest = super::strip_scheme(target)?;
    if let Some(after_slashes) = rest.strip_prefix("//") {
        if super::is_drive_prefixed(after_slashes) {
            return Some((&target[..7], super::percent_decode_utf8(after_slashes)));
        }
        let slash = after_slashes.find('/')?;
        let host = &after_slashes[..slash];
        if !super::is_drive_prefixed(host) && !super::host_is_local(host) {
            return None;
        }
        return Some((&target[..7 + slash], super::percent_decode_utf8(&after_slashes[slash..])));
    }
    (rest.starts_with('/') || super::is_drive_prefixed(rest))
        .then(|| (&target[..5], super::percent_decode_utf8(rest)))
}

fn collect_visible_candidates<'a>(clean_vis: &'a str, decoded_path: &str) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = clean_vis
        .split(['/', '\\'])
        .map(super::trim_target_punctuation)
        .filter(|seg| !seg.is_empty() && !seg.contains('\u{FFFD}'))
        .collect();

    // When `0x9C` prematurely ends OSC 8 on a bare-filename label without `/`, the spilled URI
    // tail directly precedes `<stem><corrupted_char><spilled_tail>` inside `clean_vis`.
    if let Some(last_seg) = decoded_path.rsplit(['/', '\\']).next()
        && last_seg.ends_with('\u{FFFD}')
    {
        let stem = last_seg.trim_end_matches('\u{FFFD}');
        if !stem.is_empty() {
            for (pos, _) in clean_vis.match_indices(stem) {
                let cand = super::trim_target_punctuation(&clean_vis[pos..]);
                let Some(after_stem) = cand.strip_prefix(stem) else { continue };
                let spilled =
                    clean_vis[..pos].trim_end_matches(['(', '[', '<', '（', '【', '《', '"', '\'']);
                if !spilled.is_empty()
                    && !cand.contains(['/', '\\', '\u{FFFD}'])
                    && after_stem.ends_with(spilled)
                    && !out.contains(&cand)
                {
                    out.push(cand);
                }
            }
        }
    }
    out
}

fn pick_unique_match<'a>(
    corrupted: &str,
    candidates: impl Iterator<Item = &'a str>,
    allow_trailing_cut: bool,
) -> Option<String> {
    let mut matches: Vec<&str> = Vec::new();
    let mut c1_matches: Vec<&str> = Vec::new();
    for cand in candidates {
        if let Some(has_0x9c) = match_corrupted_segment(corrupted, cand, allow_trailing_cut) {
            if !matches.contains(&cand) {
                matches.push(cand);
            }
            if has_0x9c && !c1_matches.contains(&cand) {
                c1_matches.push(cand);
            }
        }
    }
    if matches.len() == 1 {
        Some(matches[0].to_owned())
    } else if c1_matches.len() == 1 {
        Some(c1_matches[0].to_owned())
    } else {
        None
    }
}

fn match_corrupted_segment(
    corrupted: &str,
    candidate: &str,
    allow_trailing_cut: bool,
) -> Option<bool> {
    if candidate.is_empty()
        || candidate.contains(['/', '\\', '\u{FFFD}'])
        || candidate.chars().any(char::is_control)
    {
        return None;
    }
    let trailing_cut = corrupted.ends_with('\u{FFFD}');
    if trailing_cut && !allow_trailing_cut {
        return None;
    }
    let mut parts = corrupted.split('\u{FFFD}');
    let head = parts.next().unwrap_or("");
    let mut rem = candidate.strip_prefix(head)?;
    let literals: Vec<&str> = parts.filter(|part| !part.is_empty()).collect();
    if head.is_empty() && literals.is_empty() {
        return None;
    }
    let mut all_0x9c = true;

    for (idx, &lit) in literals.iter().enumerate() {
        let is_final_exact = idx + 1 == literals.len() && !trailing_cut;
        let (gap, after) = if is_final_exact {
            (rem.strip_suffix(lit)?, "")
        } else {
            let pos = rem
                .match_indices(lit)
                .map(|(p, _)| p)
                .find(|&p| p > 0 && rem[..p].chars().count() <= MAX_INTERIOR_GAP_CHARS)?;
            (&rem[..pos], &rem[pos + lit.len()..])
        };
        let char_count = gap.chars().count();
        if char_count == 0
            || char_count > MAX_INTERIOR_GAP_CHARS
            || !gap.chars().any(|c| !c.is_ascii())
        {
            return None;
        }
        all_0x9c &= gap.as_bytes().contains(&0x9C);
        rem = after;
    }

    if trailing_cut || literals.is_empty() {
        let ch = rem.chars().next().filter(|c| !c.is_ascii())?;
        all_0x9c &= ch.encode_utf8(&mut [0; 4]).as_bytes().contains(&0x9C);
    }
    Some(all_0x9c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_uri::{
        LinkTargetKind, classify_link_target, classify_link_target_with_cwd,
        classify_osc8_link_target_with_cwd,
    };

    #[test]
    fn repairs_mid_filename_truncated_and_punctuated_osc8_file_uris() {
        assert_eq!(
            repair_corrupted_file_uri_with_text(
                "file:///D:/erpnext/cqboyue/.inputdata/output/批号导入_\u{FFFD}\u{FFFD}终汇总执行包_20261111.xlsx",
                "批号导入_最终汇总执行包_20261111.xlsx。",
            )
            .as_deref(),
            Some("file:///D:/erpnext/cqboyue/.inputdata/output/批号导入_最终汇总执行包_20261111.xlsx")
        );

        let truncated = "file:///D:/erpnext/cqboyue/.inputdata/output/3_批号更新现\u{FFFD}";
        assert_eq!(
            repair_corrupted_file_uri_with_text(
                truncated,
                "记录_39行_可直接导入_v2.xlsx.inputdata/output/3_批号更新现有记录_39行_可直接导入_v2.xlsx，",
            )
            .as_deref(),
            Some("file:///D:/erpnext/cqboyue/.inputdata/output/3_批号更新现有记录_39行_可直接导入_v2.xlsx")
        );
        assert_eq!(
            repair_corrupted_file_uri_with_text(
                truncated,
                "记录_39行_可直接导入.xlsx.inputdata/output/3_批号更新现有记录_39行_可直接导入.xlsx",
            )
            .as_deref(),
            Some("file:///D:/erpnext/cqboyue/.inputdata/output/3_批号更新现有记录_39行_可直接导入.xlsx")
        );
        assert_eq!(
            repair_corrupted_file_uri_with_text(
                "file:///Users/me/Documents/pdf/01-示例企业管理咨询\u{FFFD}",
                "限公司-合同文件.pdf(01-示例企业管理咨询有限公司-合同文件.pdf)，",
            )
            .as_deref(),
            Some("file:///Users/me/Documents/pdf/01-示例企业管理咨询有限公司-合同文件.pdf")
        );

        // Punctuation trimming that shortens the candidate below `stem.len()` must never slice out of bounds.
        assert!(
            repair_corrupted_file_uri_with_text(
                "file:///Users/me/Documents/pdf/01-示例)\u{FFFD}",
                "(前缀01-示例)",
            )
            .is_none()
        );
    }

    #[test]
    fn ignores_non_local_schemes_remote_hosts_and_intact_uris() {
        assert!(
            repair_corrupted_file_uri_with_text("https://example.com/现\u{FFFD}", "现有.xlsx")
                .is_none()
        );
        assert!(
            repair_corrupted_file_uri_with_text("file://fileserver/share/现\u{FFFD}", "现有.xlsx")
                .is_none()
        );
        assert!(
            repair_corrupted_file_uri_with_text("file:///D:/output/ok.xlsx", "ok.xlsx").is_none()
        );
    }

    #[test]
    fn disk_fallback_only_runs_for_confirmed_osc8_file_uris_and_keeps_ordinary_paths_exact() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("批号导入_最终汇总执行包_20261111.xlsx");
        std::fs::write(&target, b"ok").unwrap();
        let slash = dir.path().to_string_lossy().replace('\\', "/");
        let corrupted_uri = format!(
            "[打开文件](file:/{}/批号导入_\u{FFFD}\u{FFFD}终汇总执行包_20261111.xlsx)",
            slash.trim_start_matches('/')
        );

        // Ordinary path entrypoints preserve exact matching even when a single prefix-sibling exists.
        assert!(matches!(
            classify_link_target_with_cwd("批号导入_\u{FFFD}", Some(dir.path())),
            LinkTargetKind::LocalMissing(_)
        ));
        assert!(matches!(
            classify_link_target_with_cwd("./批号导入_\u{FFFD}", Some(dir.path())),
            LinkTargetKind::LocalMissing(_)
        ));
        assert!(matches!(classify_link_target(&corrupted_uri), LinkTargetKind::LocalMissing(_)));
        assert!(matches!(
            classify_osc8_link_target_with_cwd("批号导入_\u{FFFD}", None, Some(dir.path())),
            LinkTargetKind::LocalMissing(_)
        ));

        // Confirmed OSC 8 explicit `file:` URI recovers the unique match, and refuses ambiguity.
        assert_eq!(
            classify_osc8_link_target_with_cwd(&corrupted_uri, None, Some(dir.path())),
            LinkTargetKind::LocalExisting(target)
        );

        let amb_dir = tempfile::tempdir().unwrap();
        std::fs::write(amb_dir.path().join("3_批号更新现有记录.xlsx"), b"1").unwrap();
        std::fs::write(amb_dir.path().join("3_批号更新现有记录_v2.xlsx"), b"2").unwrap();
        let amb_slash = amb_dir.path().to_string_lossy().replace('\\', "/");
        let amb_uri = format!("file:/{}/3_批号更新现\u{FFFD}", amb_slash.trim_start_matches('/'));
        assert!(matches!(
            classify_osc8_link_target_with_cwd(&amb_uri, None, Some(amb_dir.path())),
            LinkTargetKind::LocalMissing(_)
        ));
    }

    #[test]
    fn literal_fffd_filename_takes_priority_before_repaired_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let slash = dir.path().to_string_lossy().replace('\\', "/");
        let base = slash.trim_start_matches('/');

        // 1. Mid-filename `U+FFFD`: literal file wins while present; falls back once absent.
        let literal_mid = dir.path().join("批号导入_\u{FFFD}\u{FFFD}终汇总执行包_20261111.xlsx");
        let repaired_mid = dir.path().join("批号导入_最终汇总执行包_20261111.xlsx");
        std::fs::write(&literal_mid, b"literal").unwrap();
        std::fs::write(&repaired_mid, b"repaired").unwrap();
        let raw_mid_uri =
            format!("file:/{base}/批号导入_\u{FFFD}\u{FFFD}终汇总执行包_20261111.xlsx");
        let cand_mid_uri = repair_corrupted_file_uri_with_text(
            &raw_mid_uri,
            "批号导入_最终汇总执行包_20261111.xlsx",
        )
        .unwrap();
        assert_eq!(
            classify_osc8_link_target_with_cwd(&raw_mid_uri, Some(&cand_mid_uri), Some(dir.path()),),
            LinkTargetKind::LocalExisting(literal_mid.clone())
        );
        std::fs::remove_file(&literal_mid).unwrap();
        assert_eq!(
            classify_osc8_link_target_with_cwd(&raw_mid_uri, Some(&cand_mid_uri), Some(dir.path()),),
            LinkTargetKind::LocalExisting(repaired_mid)
        );

        // 2. Truncated `U+FFFD`: literal wins first; once absent, visible-label candidate resolves
        // `_v2.xlsx` deterministically alongside `.xlsx`, and stays `LocalMissing` if `_v2.xlsx`
        // itself is deleted rather than opening sibling `.xlsx`.
        let literal_trunc = dir.path().join("3_批号更新现\u{FFFD}");
        let v1_file = dir.path().join("3_批号更新现有记录_39行_可直接导入.xlsx");
        let v2_file = dir.path().join("3_批号更新现有记录_39行_可直接导入_v2.xlsx");
        std::fs::write(&literal_trunc, b"literal").unwrap();
        std::fs::write(&v1_file, b"v1").unwrap();
        std::fs::write(&v2_file, b"v2").unwrap();
        let raw_trunc_uri = format!("file:/{base}/3_批号更新现\u{FFFD}");
        let cand_v2_uri = repair_corrupted_file_uri_with_text(
            &raw_trunc_uri,
            "记录_39行_可直接导入_v2.xlsx/output/3_批号更新现有记录_39行_可直接导入_v2.xlsx",
        )
        .unwrap();
        assert_eq!(
            classify_osc8_link_target_with_cwd(
                &raw_trunc_uri,
                Some(&cand_v2_uri),
                Some(dir.path()),
            ),
            LinkTargetKind::LocalExisting(literal_trunc.clone())
        );
        std::fs::remove_file(&literal_trunc).unwrap();
        assert_eq!(
            classify_osc8_link_target_with_cwd(
                &raw_trunc_uri,
                Some(&cand_v2_uri),
                Some(dir.path()),
            ),
            LinkTargetKind::LocalExisting(v2_file.clone())
        );
        std::fs::remove_file(&v2_file).unwrap();
        assert_eq!(
            classify_osc8_link_target_with_cwd(
                &raw_trunc_uri,
                Some(&cand_v2_uri),
                Some(dir.path()),
            ),
            LinkTargetKind::LocalMissing(v2_file)
        );
    }
}
