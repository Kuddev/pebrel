//! Resolve a bare URL wrapped inside one bordered table record.

use super::*;
use nebula_terminal::term::cell::Flags;

const MAX_TABLE_ROWS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TableLink {
    pub fragments: Vec<Match>,
    pub target: String,
    pub complete: bool,
}

struct CellText {
    bounds: Match,
    text: String,
}

fn border(c: char) -> bool {
    matches!(c, '│' | '┃' | '║')
}

fn web_url(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

fn cell<T>(term: &Term<T>, line: Line, left: usize, right: usize) -> Option<CellText> {
    if line < term.topmost_line()
        || line > term.bottommost_line()
        || !border(term.grid()[Point::new(line, Column(left))].c)
        || !border(term.grid()[Point::new(line, Column(right))].c)
    {
        return None;
    }
    let start =
        (left + 1..right).find(|&col| term.grid()[Point::new(line, Column(col))].c != ' ')?;
    let end = (start..right).rfind(|&col| term.grid()[Point::new(line, Column(col))].c != ' ')?;
    let mut text = String::new();
    for col in start..=end {
        let value = &term.grid()[Point::new(line, Column(col))];
        if value.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        if value.c.is_whitespace()
            || value.c.is_control()
            || matches!(
                value.c,
                '│' | '┃'
                    | '║'
                    | '─'
                    | '━'
                    | '═'
                    | '┼'
                    | '┬'
                    | '┴'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '！'
                    | '？'
                    | '、'
            )
        {
            return None;
        }
        text.push(value.c);
    }
    Some(CellText { bounds: Point::new(line, Column(start))..=Point::new(line, Column(end)), text })
}

fn continuation<T>(term: &Term<T>, line: Line, left: usize) -> bool {
    // 截图中的续行没有新的记录标题；右侧说明列仍可独立换行。
    (0..left).all(|col| {
        let c = term.grid()[Point::new(line, Column(col))].c;
        c == ' ' || border(c)
    })
}

pub(super) fn at<T>(term: &Term<T>, point: Point) -> Option<TableLink> {
    let left = (0..point.column.0)
        .rfind(|&col| border(term.grid()[Point::new(point.line, Column(col))].c))?;
    let right = (point.column.0 + 1..term.columns())
        .find(|&col| border(term.grid()[Point::new(point.line, Column(col))].c))?;
    let mut first = cell(term, point.line, left, right)?;
    let anchor = first.bounds.start().column;
    if anchor.0 > left + 2 {
        return None;
    }
    for _ in 0..MAX_TABLE_ROWS {
        if web_url(&first.text) {
            break;
        }
        let line = first.bounds.start().line;
        if line <= term.topmost_line() || !continuation(term, line, left) {
            return None;
        }
        let previous = cell(term, line - 1, left, right)?;
        if previous.bounds.start().column != anchor || previous.bounds.end().column.0 + 2 < right {
            return None;
        }
        first = previous;
    }
    if !web_url(&first.text) {
        return None;
    }
    let mut link =
        TableLink { fragments: vec![first.bounds.clone()], target: first.text, complete: true };
    let mut last = first.bounds;
    loop {
        let next_line = last.end().line + 1;
        if last.end().column.0 + 2 < right
            || next_line > term.bottommost_line()
            || !continuation(term, next_line, left)
        {
            break;
        }
        let Some(next) = cell(term, next_line, left, right) else { break };
        if next.bounds.start().column != anchor || web_url(&next.text) {
            break;
        }
        // 到限额仍有续行就放弃重建，不能把一个看似合法的前缀当作完整目标。
        if link.fragments.len() == MAX_TABLE_ROWS || link.target.len() + next.text.len() > 65_536 {
            link.complete = false;
            break;
        }
        link.target.push_str(&next.text);
        last = next.bounds.clone();
        link.fragments.push(next.bounds);
    }
    (link.fragments.len() > 1 && link.fragments.iter().any(|range| range.contains(&point)))
        .then_some(link)
}
