//! Bounded inline reflow and the agent-answer shapes that motivated it.

use super::*;

fn compact(cells: usize) -> Option<PreparedFormula> {
    Some(PreparedFormula {
        fitted_pixel_size: 20.0,
        display_style: false,
        bleed_top: 0.0,
        bleed_bottom: 0.0,
        box_right: 0.0,
        centered: true,
        compact_cells: Some(cells),
    })
}

fn column_of(row: &str, needle: &str) -> usize {
    row[..row.find(needle).expect("needle in row")].chars().count()
}

fn projected(row: &str, cells: &[usize]) -> (Vec<FormulaOverlay>, LineProjection) {
    let overlays = scan_grid_with_hints(&TextGrid::from_rows(&[row]));
    assert_eq!(overlays.len(), cells.len(), "{row}");
    let prepared: Vec<_> = cells.iter().map(|&cells| compact(cells)).collect();
    let projection = LineProjection::build(&overlays, &prepared);
    (overlays, projection)
}

fn visual(projection: &LineProjection, column: usize) -> Option<usize> {
    projection.project_cell(Point::new(0, Column(column)), 80).map(|point| point.column.0)
}

#[test]
fn prose_after_a_formula_follows_it_up_to_a_table_border() {
    let row = "│ $y=C_1e^{r_1x}+C_2e^{r_2x}$ end │ next";
    let (overlays, projection) = projected(row, &[12]);
    let span = overlays[0].spans[0];
    let shrink = span.end - span.start - 12;

    let end = column_of(row, "end");
    assert_eq!(visual(&projection, end), Some(end - shrink), "prose closes the gap");
    let border = row.char_indices().filter(|(_, c)| *c == '│').nth(1).unwrap().0;
    let border = row[..border].chars().count();
    assert_eq!(overlays[0].reflow_limit, Some(border));
    assert_eq!(visual(&projection, border), Some(border), "table border keeps its column");
    let next = column_of(row, "next");
    assert_eq!(visual(&projection, next), Some(next), "the next cell is not shifted");
}

#[test]
fn a_wide_gap_marks_an_independently_positioned_column() {
    let row = "pre $x^2$ suffix      sidebar";
    let (overlays, projection) = projected(row, &[2]);
    let suffix = column_of(row, "suffix");
    let sidebar = column_of(row, "sidebar");
    assert_eq!(overlays[0].reflow_limit, Some(sidebar));
    assert!(visual(&projection, suffix).unwrap() < suffix);
    assert_eq!(visual(&projection, sidebar), Some(sidebar));
}

#[test]
fn single_spaces_and_wide_glyphs_stay_in_the_prose_run() {
    // `TextGrid::from_rows` gives a CJK glyph one cell; real wide-glyph spacers
    // are `None` cells and must not count as blanks either.
    let mut grid = TextGrid::from_rows(&["$x_0$ 处取极大值 tail"]);
    grid.rows[0].insert(7, None);
    grid.rows[0].truncate(grid.columns);
    let overlays = scan_grid_with_hints(&grid);
    assert_eq!(overlays[0].reflow_limit, None);
}

#[test]
fn formulas_on_both_sides_of_a_border_shift_independently() {
    let row = "a $x^2$ b | $y^2$ c";
    let (overlays, projection) = projected(row, &[2, 2]);
    let border = column_of(row, "|");
    let second = overlays[1].spans[0];
    assert_eq!(visual(&projection, border), Some(border));
    let c = column_of(row, "c");
    let own_shrink = second.end - second.start - 2;
    assert_eq!(visual(&projection, c), Some(c - own_shrink), "only the second formula counts");
}

#[test]
fn a_formula_wider_than_its_source_never_covers_the_stop_column() {
    let row = "$x$ ab│z";
    let (overlays, projection) = projected(row, &[6]);
    let border = column_of(row, "│");
    assert_eq!(overlays[0].reflow_limit, Some(border));
    assert_eq!(visual(&projection, column_of(row, "a")), None, "pushed onto the border");
    assert_eq!(visual(&projection, border), Some(border));
    assert_eq!(visual(&projection, border + 1), Some(border + 1));
}

#[test]
fn hit_testing_maps_vacated_cells_before_the_stop_column() {
    let row = "$x^2$ ab   side";
    let (_, projection) = projected(row, &[2]);
    let side = column_of(row, "side");
    let (source, _) = projection.source_from_visual(Point::new(0, Column(side)), Side::Left);
    assert_eq!(source.column.0, side, "the fixed column is its own source");
    let (vacated, _) = projection.source_from_visual(Point::new(0, Column(side - 1)), Side::Left);
    assert!(vacated.column.0 < side, "a vacated cell never selects the fixed column");
    let b = column_of(row, "b");
    let visual_b = visual(&projection, b).unwrap();
    let (back, _) = projection.source_from_visual(Point::new(0, Column(visual_b)), Side::Left);
    assert_eq!(back.column.0, b, "shifted prose round-trips");
}

#[test]
fn unbounded_prose_keeps_the_existing_projection() {
    // No stop column: the projection is identical to the unbounded one.
    let (overlays, projection) = projected("pre $x^2$ suffix", &[2]);
    assert_eq!(overlays[0].reflow_limit, None);
    let suffix = column_of("pre $x^2$ suffix", "suffix");
    assert_eq!(visual(&projection, suffix), Some(suffix - 3));
}

#[test]
fn agent_answer_shapes_from_the_report_are_recognized() {
    let row =
        "- 通解里有两个任意常数 $C_1$ 、 $C_2$ ，题目只告诉了 $f(x_0)>0$ 和 $f^{\\prime}(x_0)=0$";
    let found: Vec<_> = scan_grid(&TextGrid::from_rows(&[row]))
        .into_iter()
        .map(|formula| formula.source.to_string())
        .collect();
    assert_eq!(found, ["C_1", "C_2", "f(x_0)>0", r"f^{\prime}(x_0)=0"]);

    for (row, source) in [
        ("| 两个不等实根 | $y=C_1e^{r_1x}+C_2e^{r_2x}$ |", "y=C_1e^{r_1x}+C_2e^{r_2x}"),
        ("| 二重根 | $y=(C_1+C_2x)e^{rx}$ |", "y=(C_1+C_2x)e^{rx}"),
        ("所以 $(0,f(0))$ 是拐点", "(0,f(0))"),
        ("点 $(x_0,f(x_0))$ 处", "(x_0,f(x_0))"),
        ("初值 $X_0$ 与 $C_{12}$", "X_0"),
    ] {
        let found = scan_grid(&TextGrid::from_rows(&[row]));
        assert!(!found.is_empty(), "{row}");
        assert_eq!(found[0].source.as_ref(), source);
    }
}

#[test]
fn widened_evidence_still_rejects_shell_and_prose() {
    for row in [
        "echo $HOME $USER",
        "env $LONG_VARIABLE$",
        "two letters $AB$ and $A1$",
        "literal $hello$ text",
        "config $PATH=/tmp$",
        "pair $(key,value)$ here",
        "call $(foo, bar(baz))$ here",
        "brace $a{b}$ only",
        "empty $^{}$ base",
    ] {
        assert!(scan_grid(&TextGrid::from_rows(&[row])).is_empty(), "{row}");
    }
}
