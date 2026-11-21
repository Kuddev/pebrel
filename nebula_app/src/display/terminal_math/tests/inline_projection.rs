use super::*;

fn compact_prepared(cells: usize) -> Option<PreparedFormula> {
    Some(PreparedFormula {
        fitted_pixel_size: TEST_FONT_PX,
        display_style: false,
        bleed_top: 0.0,
        bleed_bottom: 0.0,
        box_right: 0.0,
        centered: true,
        compact_cells: Some(cells),
    })
}

#[test]
fn compact_projection_moves_only_render_coordinates() {
    let grid = TextGrid::from_rows(&["pre $x^2$ suffix"]);
    let original = grid.rows.clone();
    let overlays = scan_grid(&grid);
    assert_eq!(overlays.len(), 1);
    let span = overlays[0].spans[0];
    let projection = LineProjection::build(&overlays, &[compact_prepared(2)]);

    assert!(projection.project_cell(Point::new(0, Column(span.start)), 80).is_none());
    let suffix = projection
        .project_cell(Point::new(0, Column(span.end)), 80)
        .expect("suffix remains visible");
    assert_eq!(suffix.column.0, span.start + 2);
    assert_eq!(grid.rows, original, "projection must not mutate terminal cells");
}

#[test]
fn compact_projection_retains_background_under_visual_formula_box() {
    let grid = TextGrid::from_rows(&["pre $x^2$ suffix"]);
    let overlays = scan_grid(&grid);
    let span = overlays[0].spans[0];
    let projection = LineProjection::build(&overlays, &[compact_prepared(2)]);

    let first = projection
        .project_formula_background(Point::new(0, Column(span.start)), 80)
        .expect("first visual formula cell keeps its background");
    let second = projection
        .project_formula_background(Point::new(0, Column(span.start + 1)), 80)
        .expect("second visual formula cell keeps its background");
    assert_eq!(first.column.0, span.start);
    assert_eq!(second.column.0, span.start + 1);
    assert!(
        projection.project_formula_background(Point::new(0, Column(span.start + 2)), 80).is_none(),
        "source cells beyond the compact formula width must not create a background strip"
    );
}

#[test]
fn disabled_projection_keeps_fixed_columns() {
    let grid = TextGrid::from_rows(&["pre $x^2$                         sidebar"]);
    let overlays = scan_grid(&grid);
    let span = overlays[0].spans[0];
    let prepared = [compact_prepared(2)];
    let mut state = TerminalMathState::default();

    state.update_projection(&overlays, &prepared, false);

    let source = state
        .project_formula_background(Point::new(0, Column(span.start)), 80)
        .expect("formula background remains at its TUI column");
    let sidebar_column = grid.rows[0].iter().position(|cell| *cell == Some('s')).unwrap();
    let sidebar = state
        .project_cell(Point::new(0, Column(sidebar_column)), 80)
        .expect("sidebar remains visible");
    assert_eq!(source.column.0, span.start);
    assert_eq!(sidebar.column.0, sidebar_column);
}

#[test]
fn compact_projection_discards_overlapping_streaming_candidate() {
    let grid = TextGrid::from_rows(&["pre $x^2$ suffix"]);
    let outer = scan_grid(&grid).remove(0);
    let outer_span = outer.spans[0];
    let mut inner = outer.clone();
    inner.spans[0].start += 1;
    inner.spans[0].end -= 1;
    let mut state = TerminalMathState::default();
    let retained = state.update_projection_with_survivors(
        &[inner, outer],
        &[compact_prepared(1), compact_prepared(2)],
        true,
    );
    let projection = state.projection_snapshot();

    assert_eq!(projection.spans.len(), 1);
    assert_eq!(projection.spans[0].source_start, outer_span.start);
    assert_eq!(projection.spans[0].source_end, outer_span.end);
    assert_eq!(retained, [false, true], "GPUI must drop the same overlapping candidate");
}

#[test]
fn compact_projection_accumulates_multiple_formula_shifts() {
    let grid = TextGrid::from_rows(&["a $x^2$ b $y^2$ c"]);
    let overlays = scan_grid(&grid);
    assert_eq!(overlays.len(), 2);
    let prepared = [compact_prepared(2), compact_prepared(2)];
    let projection = LineProjection::build(&overlays, &prepared);
    let last = overlays[1].spans[0];
    let source_reduction: usize =
        overlays.iter().map(|overlay| overlay.spans[0].end - overlay.spans[0].start - 2).sum();

    let suffix = projection
        .project_cell(Point::new(0, Column(last.end)), 80)
        .expect("suffix remains visible");
    assert_eq!(suffix.column.0, last.end - source_reduction);
}

#[test]
fn formula_hit_testing_returns_source_boundaries() {
    let grid = TextGrid::from_rows(&["pre $x^2$ suffix"]);
    let overlays = scan_grid(&grid);
    let span = overlays[0].spans[0];
    let projection = LineProjection::build(&overlays, &[compact_prepared(2)]);

    let (left, left_side) =
        projection.source_from_visual(Point::new(0, Column(span.start)), Side::Left);
    let (right, right_side) =
        projection.source_from_visual(Point::new(0, Column(span.start + 1)), Side::Right);
    assert_eq!((left.column.0, left_side), (span.start, Side::Left));
    assert_eq!((right.column.0, right_side), (span.end - 1, Side::Right));
}

#[test]
fn formula_hit_testing_preserves_nonzero_viewport_origin() {
    let grid = TextGrid::from_rows(&["pre $x^2$ suffix"]);
    let overlays = scan_grid(&grid);
    let span = overlays[0].spans[0];
    let mut state = TerminalMathState::default();
    state.update_projection(&overlays, &[compact_prepared(2)], true);
    let viewport_origin = Line(-7);

    let (left, left_side) = state.source_point(
        Point::new(viewport_origin, Column(span.start)),
        Side::Left,
        viewport_origin,
    );
    let (suffix, suffix_side) = state.source_point(
        Point::new(viewport_origin, Column(span.start + 2)),
        Side::Left,
        viewport_origin,
    );

    assert_eq!((left.line, left.column.0, left_side), (viewport_origin, span.start, Side::Left));
    assert_eq!(
        (suffix.line, suffix.column.0, suffix_side),
        (viewport_origin, span.end, Side::Left)
    );
}

#[test]
fn inline_reflow_reads_the_visible_row_with_retained_display_context() {
    use nebula_terminal::term::test::mock_term;

    let row = r"\(x^2\) some prose   | fixed column";
    let mut term = mock_term(&format!("\\[\r\na^2\r\n\\]\r\n{row}\r\nmore text\r\nprompt"));
    term.set_presentation_origin(Some(Line(3)));
    let size = test_size(term.columns() as f32, 3.0);
    let mut state = TerminalMathState::default();
    let overlays =
        scan_visible(&mut state, &term, &size, &[], true, None, Rgb::default(), |_| true);
    let span = overlays[0].inline_span().expect("visible inline formula");
    assert_eq!(span.row, 0);
    state.update_projection(&overlays, &[compact_prepared(2)], true);

    let prose = row.find("some").unwrap();
    let separator = row.find('|').unwrap();
    assert_eq!(
        state.project_cell(Point::new(0, Column(prose)), term.columns()),
        Some(Point::new(0, Column(3))),
        "the suffix follows the compacted formula on the visible row"
    );
    assert_eq!(
        state.project_cell(Point::new(0, Column(separator)), term.columns()),
        Some(Point::new(0, Column(separator))),
        "the same row's fixed separator stays at its source column"
    );
}

#[test]
fn wrapped_inline_formula_stays_on_closing_row_and_compacts_trailing_text() {
    let grid = TextGrid::from_rows(&[
        r"- 如果保留动态衰减矩阵 $\prod_{i=1}^N (I - \hat{\kappa}_i^\top",
        r"  (a_i \odot \hat{\kappa}_i))$，因为一般矩阵乘法 $A_1 A_2 \neq A_2 A_1$，排在后面",
    ]);
    let overlays = scan_grid_with_hints(&grid);
    assert_eq!(overlays.len(), 2);
    assert!(!overlays[0].display);
    assert_eq!(overlays[0].spans.len(), 2);
    let closing_span = overlays[0].inline_span().expect("wrapped inline span");
    assert_eq!(closing_span.row, 1);
    assert_eq!(closing_span.start, 2, "must preserve list continuation indent");

    let size = test_size(grid.columns as f32, grid.rows.len() as f32);
    let bounds = overlays[0].bounds(&size).expect("bounds");
    assert_eq!(bounds.left, 2.0 * size.cell_width());
    assert_eq!(bounds.top, size.cell_height());
    assert_eq!(bounds.bottom, 2.0 * size.cell_height());

    let mut state = TerminalMathState::default();
    let prepared = prepare_overlays(&mut state, &overlays, &size, TEST_FONT_PX, 1.0);
    let compact_cells = prepared[0].and_then(|item| item.compact_cells).expect("compact inline");
    state.update_projection(&overlays, &prepared, true);

    let coverage = CoverageMask::build(&overlays, &prepared);
    assert!(coverage.covers(Point::new(0, Column(overlays[0].spans[0].start))));
    assert!(coverage.covers(Point::new(1, Column(closing_span.start))));
    assert!(!coverage.covers(Point::new(1, Column(0))), "leading indent stays uncovered");

    let suffix = state
        .project_cell(Point::new(1, Column(closing_span.end)), grid.columns)
        .expect("trailing prose stays visible");
    assert_eq!(suffix.column.0, closing_span.start + compact_cells);
}

#[test]
fn wrapped_inline_formula_partially_scrolled_at_bottom_keeps_visible_span() {
    let mut state = TerminalMathState::default();
    let full = grid_at(
        &[
            "                        $x^2$                                   ",
            r"- 如果保留动态衰减矩阵 $\prod_{i=1}^N (I - \hat{\kappa}_i^\top",
            r"  (a_i \odot \hat{\kappa}_i))$，排在后面                         ",
        ],
        10,
        0,
    );
    remember_visible(&mut state, &full);
    assert_eq!(state.formulas.len(), 2);

    // Viewport shows only the first two rows (absolute rows 10..=11); closing
    // row 12 sits just below the bottom of the 2-line viewport.
    let partial = grid_at(
        &[
            "                        $x^2$                                   ",
            r"- 如果保留动态衰减矩阵 $\prod_{i=1}^N (I - \hat{\kappa}_i^\top",
        ],
        10,
        0,
    );
    state.synchronize_grid(&partial);
    let mut overlays = state.visible_overlays(&partial);
    assert_eq!(overlays.len(), 2);
    for overlay in &mut overlays {
        apply_layout_hints(overlay, &partial);
    }
    mark_formula_neighbours(&mut overlays);
    assert!(
        overlays[0].formula_neighbours_below[0] && overlays[1].formula_neighbours_above[0],
        "neighbour detection must use the visible anchor on row 1 instead of off-screen row 2"
    );

    let size = test_size(partial.columns as f32, partial.rows.len() as f32);
    let visible_span = overlays[1].inline_span().expect("visible inline anchor");
    assert_eq!(visible_span.row, 1);
    let bounds = overlays[1].bounds(&size).expect("visible span keeps clip-able bounds");
    assert_eq!(bounds.top, size.cell_height());
    assert_eq!(bounds.bottom, 2.0 * size.cell_height());

    let prepared = prepare_overlays(&mut state, &overlays, &size, TEST_FONT_PX, 1.0);
    let compact_cells =
        prepared[1].and_then(|item| item.compact_cells).expect("partially visible inline formula");
    state.update_projection(&overlays, &prepared, true);
    let plan = plan_overlay_draw(&mut state, &overlays[1], &prepared[1].unwrap(), &size, 1.0)
        .expect("draw plan on visible row");
    assert!(plan.clip_top >= 0.0 && plan.clip_bottom <= 2.0 * size.cell_height());

    let (left, left_side) =
        state.source_point(Point::new(Line(1), Column(visible_span.start)), Side::Left, Line(0));
    let (right, right_side) = state.source_point(
        Point::new(Line(1), Column(visible_span.start + compact_cells - 1)),
        Side::Right,
        Line(0),
    );
    assert_eq!(
        (left.line, left.column.0, left_side),
        (Line(1), visible_span.start, Side::Left),
        "left half of partially scrolled formula on row 1 maps to row 1 opening source column"
    );
    assert_eq!(
        (right.line, right.column.0, right_side),
        (Line(1), visible_span.end - 1, Side::Right),
        "right half of partially scrolled formula on row 1 maps to row 1 end source column"
    );
}

#[test]
fn expanded_inline_formula_clamps_to_slack_without_dropping_suffix_before_barrier() {
    let grid = TextGrid::from_rows(&["| $x$ ok   | value |"]);
    let overlays = scan_grid_with_hints(&grid);
    assert_eq!(overlays.len(), 1);
    let span = overlays[0].inline_span().unwrap();
    assert_eq!((span.start, span.end), (2, 5));
    assert_eq!(overlays[0].reflow_limit, Some(8));
    assert_eq!(overlays[0].reflow_content_end, Some(8));

    // Source is 3 cells (`$x$`), suffix ` ok` occupies 5..8, and padding
    // `   ` occupies 8..11 before `|` at 11. Requesting 6 visual cells must
    // clamp to 3 cells so ` ok` is not pushed into the padding barrier.
    let projection = LineProjection::build(&overlays, &[compact_prepared(6)]);
    for col in 5..8 {
        assert_eq!(
            projection.project_cell(Point::new(0, Column(col)), grid.columns),
            Some(Point::new(0, Column(col))),
            "suffix cell {col} must stay visible and unshifted when no slack precedes barrier"
        );
    }
    assert_eq!(
        projection.project_cell(Point::new(0, Column(11)), grid.columns),
        Some(Point::new(0, Column(11))),
        "table separator must stay aligned"
    );
}

#[test]
fn prose_with_numbers_unrelated_rows_and_formula_operators_keeps_reflow_unbounded() {
    let grid = TextGrid::from_rows(&[
        "unrelated preamble with single spaces aligned before almost every word",
        r"state $S_t$, GPU in 1 ms updates all 4096 words cleanly.",
        r"let $a_1$ hold when $x + y = z$ and $u_t$ converges.",
        "          $$E = mc^2$$",
        "    - nested markdown bullet item",
    ]);
    let overlays = scan_grid_with_hints(&grid);
    assert_eq!(overlays.len(), 5);
    assert_eq!(overlays[0].reflow_limit, None);
    assert_eq!(overlays[1].reflow_limit, None);
    assert_eq!(overlays[2].reflow_limit, None);
    assert_eq!(overlays[3].reflow_limit, None);
}

#[test]
fn multi_row_table_still_finds_padded_column_through_single_space_separator_row() {
    let grid = TextGrid::from_rows(&["Name    Value", "------- -----", r"\(x^2\) col"]);
    let overlays = scan_grid_with_hints(&grid);
    assert_eq!(overlays.len(), 1);
    assert_eq!(overlays[0].reflow_limit, Some(8));
}
