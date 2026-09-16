use nebula_terminal::grid::Dimensions;
use winit::window::Theme as WinitTheme;

use super::{
    AiLogo, NebulaConfirm, SizeInfo, ai_logo, alt_screen_vertical_padding_bands, compute_cell_size,
    extract_program, nebula_pad_to_cells, percent_decode_lossy, prepare_ai_logo_texture,
    program_icon, remove_ssh_host_from_lists, replays_untrusted_terminal_output,
    restore_ssh_host_to_lists, strip_file_scheme, system_theme_snapshot,
};
use crate::config::UiConfig;
use crate::display::settings::CellWidthMode;

/// 受控字体度量：只有 advance 与 line_height 参与单元格尺寸计算，
/// 其余字段取任意合法值。
fn metrics(average_advance: f64, line_height: f64) -> crossfont::Metrics {
    crossfont::Metrics {
        average_advance,
        line_height,
        descent: -4.0,
        underline_position: -2.0,
        underline_thickness: 1.0,
        strikeout_position: 5.0,
        strikeout_thickness: 1.0,
    }
}

#[test]
fn relaxed_cell_width_rounds_up_the_fraction_compact_floors_it() {
    let config = UiConfig::default();
    // Maple Mono NF CN 这类字体的平均 advance 常落在 .5 以上，紧凑向下
    // 取整因此会少一像素——宽松就是为补这一像素而设。
    let m = metrics(9.6, 20.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 10.0);
}

#[test]
fn a_fraction_below_half_stays_on_the_same_column_width_in_both_modes() {
    let config = UiConfig::default();
    let m = metrics(9.4, 20.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 9.0);
}

#[test]
fn the_exact_half_boundary_rounds_away_from_zero_in_relaxed_mode() {
    let config = UiConfig::default();
    let m = metrics(9.5, 20.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Compact).0, 9.0);
    assert_eq!(compute_cell_size(&config, &m, CellWidthMode::Relaxed).0, 10.0);
}

#[test]
fn both_modes_compute_the_same_cell_height() {
    let config = UiConfig::default();
    // 该偏好只控制列宽；高度必须逐位相同，否则行距会随模式漂移。
    for (advance, line_height) in [(9.6, 20.7), (7.5, 16.5), (12.2, 25.9)] {
        let m = metrics(advance, line_height);
        let compact = compute_cell_size(&config, &m, CellWidthMode::Compact);
        let relaxed = compute_cell_size(&config, &m, CellWidthMode::Relaxed);
        assert_eq!(compact.1, relaxed.1, "line_height {line_height} 的高度在两模式间漂移");
    }
}

#[test]
fn both_modes_share_the_same_minimum_cell_width() {
    let config = UiConfig::default();
    // 退化度量（字体加载异常）不能产出 0 宽单元格——那会让网格除零。
    let m = metrics(0.3, 0.4);
    let compact = compute_cell_size(&config, &m, CellWidthMode::Compact);
    let relaxed = compute_cell_size(&config, &m, CellWidthMode::Relaxed);
    assert_eq!(compact.0, 1.0);
    assert_eq!(relaxed.0, 1.0);
    assert_eq!(compact.1, relaxed.1, "退化度量下高度也不得随模式漂移");
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn file_uri_tooltip_shows_decoded_path() {
    // `ls --hyperlink` percent-encodes CJK names; the tooltip must not.
    assert_eq!(
        strip_file_scheme("file:///D:/%E6%98%9F%E9%9B%B2/read%20me.txt"),
        "D:/星雲/read me.txt"
    );
    // Non-file URIs keep their encoding — it is part of their identity.
    assert_eq!(strip_file_scheme("https://a.b/c%20d"), "https://a.b/c%20d");
    // Malformed escapes and non-UTF-8 decodes survive verbatim.
    assert_eq!(percent_decode_lossy("100%"), "100%");
    assert_eq!(percent_decode_lossy("%zz"), "%zz");
    assert_eq!(percent_decode_lossy("%ff%fe"), "%ff%fe");
}

#[test]
fn log_replay_commands_do_not_receive_terminal_query_answers() {
    for command in [
        "docker logs app",
        "docker compose logs -f api",
        "podman logs app",
        "kubectl logs pod/api",
        "journalctl -f -u nebula",
    ] {
        assert!(replays_untrusted_terminal_output(command), "{command}");
    }
    for command in ["docker run app", "kubectl exec pod -- sh", "cargo test", "nvim"] {
        assert!(!replays_untrusted_terminal_output(command), "{command}");
    }
}

#[test]
fn popup_pad_counts_display_cells_and_drops_straddling_wide_chars() {
    assert_eq!(nebula_pad_to_cells("ab", 4), "ab  ");
    assert_eq!(nebula_pad_to_cells("目录", 4), "目录");
    // 第二个全宽字符放不进 3 格：丢弃并用空格补齐。
    assert_eq!(nebula_pad_to_cells("目录", 3), "目 ");
    assert_eq!(nebula_pad_to_cells("abcd", 3), "abc");
}

#[test]
fn popup_label_elides_from_the_left() {
    assert_eq!(super::suggest_engine::elide_left("short", 10), "short");
    assert_eq!(super::suggest_engine::elide_left("abcdefgh", 5), "…efgh");
}

#[test]
fn system_theme_snapshot_beats_a_stale_window_override() {
    assert_eq!(
        system_theme_snapshot(Some(WinitTheme::Dark), Some(WinitTheme::Light)),
        Some(WinitTheme::Dark)
    );
    assert_eq!(system_theme_snapshot(None, Some(WinitTheme::Light)), Some(WinitTheme::Light));
}

#[test]
fn ssh_delete_undo_restores_saved_and_pinned_order() {
    let mut saved = strings(&["alpha", "target", "omega"]);
    let mut pinned = strings(&["target", "alpha"]);
    let mut hidden = strings(&["already-hidden"]);

    let snapshot =
        remove_ssh_host_from_lists("target", false, &mut saved, &mut pinned, &mut hidden);
    assert_eq!(snapshot, (Some(1), Some(0), false));
    assert_eq!(saved, strings(&["alpha", "omega"]));
    assert_eq!(pinned, strings(&["alpha"]));
    // A Nebula-managed host is deleted, not renamed to "hidden": nothing
    // may linger in the hidden section for it.
    assert_eq!(hidden, strings(&["already-hidden"]));

    restore_ssh_host_to_lists(
        "target",
        snapshot.0,
        snapshot.1,
        snapshot.2,
        &mut saved,
        &mut pinned,
        &mut hidden,
    );
    assert_eq!(saved, strings(&["alpha", "target", "omega"]));
    assert_eq!(pinned, strings(&["target", "alpha"]));
    assert_eq!(hidden, strings(&["already-hidden"]));
}

#[test]
fn ssh_config_only_hide_is_fully_reversible() {
    let mut saved = Vec::new();
    let mut pinned = Vec::new();
    let mut hidden = Vec::new();

    let snapshot =
        remove_ssh_host_from_lists("config-alias", true, &mut saved, &mut pinned, &mut hidden);
    assert_eq!(snapshot, (None, None, false));
    assert_eq!(hidden, strings(&["config-alias"]));

    restore_ssh_host_to_lists(
        "config-alias",
        snapshot.0,
        snapshot.1,
        snapshot.2,
        &mut saved,
        &mut pinned,
        &mut hidden,
    );
    assert!(saved.is_empty());
    assert!(pinned.is_empty());
    assert!(hidden.is_empty());
}

/// A host that exists both as a saved entry and as a `~/.ssh/config`
/// alias must be hidden on top of the saved-list removal, otherwise the
/// config merge resurrects it on the next restart.
#[test]
fn ssh_delete_of_a_config_backed_saved_host_also_hides_the_alias() {
    let mut saved = strings(&["dual"]);
    let mut pinned = Vec::new();
    let mut hidden = Vec::new();

    let snapshot = remove_ssh_host_from_lists("dual", true, &mut saved, &mut pinned, &mut hidden);
    assert_eq!(snapshot, (Some(0), None, false));
    assert!(saved.is_empty());
    assert_eq!(hidden, strings(&["dual"]));

    restore_ssh_host_to_lists(
        "dual",
        snapshot.0,
        snapshot.1,
        snapshot.2,
        &mut saved,
        &mut pinned,
        &mut hidden,
    );
    assert_eq!(saved, strings(&["dual"]));
    assert!(hidden.is_empty());
}

#[test]
fn asymmetric_bottom_reserve_recovers_rows_hidden_by_top_chrome() {
    let size = SizeInfo::new_fully_asymmetric(1000.0, 1000.0, 10.0, 20.0, 0.0, 0.0, 64.0, 16.0);
    assert_eq!(size.screen_lines(), 46);
    assert_eq!(size.padding_y(), 64.0);
    assert_eq!(size.padding_bottom(), 16.0);

    let old_symmetric = SizeInfo::new_asymmetric(1000.0, 1000.0, 10.0, 20.0, 0.0, 0.0, 64.0);
    assert_eq!(old_symmetric.screen_lines(), 43);
}

#[test]
fn alternate_screen_padding_stays_inside_stacked_panes() {
    let window = SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 80.0, 20.0);
    let top = SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 80.0, 324.0);
    let bottom =
        SizeInfo::new_fully_asymmetric(1000.0, 700.0, 10.0, 20.0, 100.0, 20.0, 384.0, 20.0);

    assert_eq!(
        alt_screen_vertical_padding_bands(&window, &top, 56.0, 636.0),
        [Some((56.0, 24.0)), Some((360.0, 16.0))]
    );
    assert_eq!(
        alt_screen_vertical_padding_bands(&window, &bottom, 56.0, 636.0),
        [None, Some((664.0, 28.0))]
    );
}

#[test]
fn missing_font_notice_can_be_dismissed() {
    let confirm =
        NebulaConfirm::InstallRequiredFont { directory: std::path::PathBuf::from("fonts") };

    assert!(confirm.can_dismiss());
}

#[test]
fn tab_drop_ignores_scrolled_out_zero_rows() {
    // Storage indices 0..=2 and 13.. are hidden; only 3..=12 have screen
    // coordinates. A pointer within that window must never be shifted by
    // the hidden rows that the layout keeps for index stability.
    let visible: Vec<_> = (3..=12)
        .map(|index| (index, (0.0, 100.0 + (index - 3) as f32 * 30.0, 200.0, 24.0)))
        .collect();

    assert_eq!(super::tab_drop_index_from_visible_rows(5, 90.0, &visible, 16), 3);
    assert_eq!(super::tab_drop_index_from_visible_rows(5, 130.0, &visible, 16), 4);
    assert_eq!(super::tab_drop_index_from_visible_rows(5, 500.0, &visible, 16), 12);
    assert_eq!(super::tab_drop_index_from_visible_rows(5, 130.0, &[], 16), 5);
}
