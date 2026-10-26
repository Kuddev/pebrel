//! 纯字符画在原占位内适配比例；正文网格、PTY 尺寸和源文本保持不变。

use std::ops::Range;

use nebula_terminal::render::RenderSnapshot;

mod paint;
pub(super) use paint::paint_overflow;

const BLOCKED: u8 = 15;
const DENSE: u8 = 16;
const VISITED: u8 = 128;
const MAX_CELLS: usize = 262_144;
const MAX_REGIONS: usize = 32;

#[derive(Clone, Copy, Debug)]
struct Area {
    top: i64,
    bottom: i64,
    left: usize,
    right: usize,
    kind: u8,
}

impl Area {
    fn intersects_rows(self, top: i64, rows: usize) -> bool {
        self.top < top + rows as i64 && self.bottom > top
    }
}

#[derive(Clone, Copy)]
struct Region {
    area: Area,
    source_top: f32,
    visual_top: f32,
    padding: f32,
    ratio: f32,
}

impl Region {
    fn contains(self, row: i64, col: usize) -> bool {
        let row = row as f32 - self.source_top;
        row >= 0.0
            && row < (self.area.bottom - self.area.top) as f32
            && (self.area.left..self.area.right).contains(&col)
    }
}

/// 坐标单位与 native-result projection 一样，都是行/列，不混用物理像素。
#[derive(Clone, Default)]
pub(super) struct Frame {
    regions: Vec<Region>,
}

impl Frame {
    fn region(&self, row: i64, col: usize) -> Option<usize> {
        self.regions.iter().position(|region| region.contains(row, col))
    }

    pub fn same_region(&self, row: usize, a: usize, b: usize) -> bool {
        self.region(row as i64, a) == self.region(row as i64, b)
    }

    pub fn cell(&self, row: usize, col: usize, normal_y: f32) -> (f32, f32) {
        self.cell_at(row as i64, col, normal_y)
    }

    pub fn cell_at(&self, row: i64, col: usize, normal_y: f32) -> (f32, f32) {
        let Some(index) = self.region(row, col) else { return (normal_y, 1.0) };
        let region = self.regions[index];
        (
            region.visual_top + region.padding + (row as f32 - region.source_top) * region.ratio,
            region.ratio,
        )
    }

    pub fn mapped_source_row(&self, col: f32, visual_y: f32) -> Option<f32> {
        for region in &self.regions {
            let rows = (region.area.bottom - region.area.top) as f32;
            if col >= region.area.left as f32
                && col < region.area.right as f32
                && visual_y >= region.visual_top
                && visual_y < region.visual_top + rows
            {
                // 上下留白归对应边缘源行，拖选离开图案时仍有确定且连续的边界。
                return Some(
                    region.source_top
                        + ((visual_y - region.visual_top - region.padding) / region.ratio)
                            .clamp(0.0, rows - 0.0001),
                );
            }
        }
        None
    }

    pub fn source_bounds(&self) -> Range<i64> {
        let start = self.regions.iter().map(|r| r.source_top as i64).min().unwrap_or(0);
        let end = self
            .regions
            .iter()
            .map(|r| r.source_top as i64 + r.area.bottom - r.area.top)
            .max()
            .unwrap_or(0);
        start..end
    }

    pub fn shift_source(&mut self, offset: i64) {
        for region in &mut self.regions {
            region.source_top += offset as f32;
        }
    }

    pub fn spans(
        &self,
        row: i64,
        columns: Range<usize>,
        normal_y: f32,
        mut paint: impl FnMut(Range<usize>, f32, f32),
    ) {
        let mut start = columns.start;
        while start < columns.end {
            let end = if let Some(index) = self.region(row, start) {
                columns.end.min(self.regions[index].area.right)
            } else {
                self.regions
                    .iter()
                    .filter(|region| {
                        region.area.left > start && region.contains(row, region.area.left)
                    })
                    .map(|region| region.area.left)
                    .min()
                    .unwrap_or(columns.end)
                    .min(columns.end)
            };
            let (y, height) = self.cell_at(row, start, normal_y);
            paint(start..end, y, height);
            start = end;
        }
    }
}

#[derive(Default)]
pub(super) struct Layout {
    identity: Option<(usize, u64, bool, u16)>,
    cached: Vec<Area>,
    pub frame: Frame,
}

fn geometry(ch: char) -> u8 {
    match ch {
        '\u{2801}'..='\u{28ff}' => {
            1 | if (ch as u32 - 0x2800).count_ones() >= 5 { DENSE } else { 0 }
        },
        '\u{2580}'..='\u{259f}' => 2 | DENSE,
        _ => BLOCKED,
    }
}

fn kind(value: u8) -> u8 {
    value & 15
}

impl Layout {
    pub fn capture_rows(
        &self,
        identity: (usize, u64, bool, u16),
        top: i64,
        rows: usize,
        retained: Range<i64>,
        visible: &RenderSnapshot,
    ) -> Option<Range<i64>> {
        let mut range = top..top + rows as i64;
        for area in self
            .cached
            .iter()
            .filter(|a| self.identity == Some(identity) && a.intersects_rows(top, rows))
        {
            range.start = range.start.min(area.top - 1);
            range.end = range.end.max(area.bottom + 1);
        }
        // 新图案可能正好贴着视口边缘；有界读取相邻源行，区分完整图案与被裁掉的片段。
        let mut edge = |row: u16| {
            if row < 3 {
                range.start = range.start.min(top - 32);
            }
            if usize::from(row) >= rows.saturating_sub(3) {
                range.end = range.end.max(top + rows as i64 + 32);
            }
        };
        for glyph in &visible.box_glyphs {
            if !glyph.wide && kind(geometry(glyph.ch)) != BLOCKED {
                edge(glyph.row);
            }
        }
        for segment in &visible.segments {
            if !segment.wide
                && segment
                    .cells
                    .windows(8)
                    .any(|run| run.iter().all(|cell| matches!(cell.text.as_str(), "/" | "\\")))
            {
                edge(segment.row);
            }
        }
        range.start = range.start.max(retained.start);
        range.end = range.end.min(retained.end);
        let length = range.end.saturating_sub(range.start) as usize;
        ((range.start < top || range.end > top + rows as i64)
            && length > 0
            && length <= u16::MAX as usize
            && length.saturating_mul(usize::from(identity.3)) <= MAX_CELLS)
            .then_some(range)
    }

    pub fn update(
        &mut self,
        snap: &RenderSnapshot,
        identity: (usize, u64, bool, u16),
        viewport_top: i64,
        width: f32,
        height: f32,
        disabled: bool,
        project_row: impl Fn(f32) -> f32,
        allowed: impl Fn(usize, usize) -> bool,
    ) {
        self.frame.regions.clear();
        if self.identity != Some(identity) {
            self.cached.clear();
            self.identity = Some(identity);
        }
        let cols = usize::from(snap.cols);
        let rows = usize::from(snap.rows);
        if disabled
            || width <= 0.0
            || height <= width * 2.0
            || rows.saturating_mul(cols) > MAX_CELLS
        {
            self.cached.clear();
            return;
        }
        let (mut count, mut first, mut last) = (0, u16::MAX, 0);
        let mut candidate = |row| {
            count += 1;
            first = first.min(row);
            last = last.max(row);
        };
        for glyph in &snap.box_glyphs {
            if !glyph.wide && kind(geometry(glyph.ch)) != BLOCKED {
                candidate(glyph.row);
            }
        }
        for segment in &snap.segments {
            if !segment.wide {
                for cell in &segment.cells {
                    if matches!(cell.text.as_str(), "/" | "\\") {
                        candidate(segment.row);
                    }
                }
            }
        }
        // 普通输出、单行进度条和路径不分配候选网格；已知图案滚出一部分时保留其坐标。
        if count == 0
            || ((count < 32 || last.saturating_sub(first) < 3)
                && !self.cached.iter().any(|area| area.intersects_rows(viewport_top, rows)))
        {
            self.cached.retain(|area| !area.intersects_rows(viewport_top, rows));
            return;
        }
        let mut cells = vec![0u8; rows * cols];
        for glyph in &snap.box_glyphs {
            let row = usize::from(glyph.row);
            let col = usize::from(glyph.col);
            if row < rows && col < cols {
                cells[row * cols + col] = if glyph.wide {
                    BLOCKED
                } else if glyph.ch == '\u{2800}' {
                    0
                } else {
                    geometry(glyph.ch)
                };
            }
        }
        for segment in &snap.segments {
            let row = usize::from(segment.row);
            if row >= rows {
                continue;
            }
            for cell in &segment.cells {
                let col = usize::from(cell.col);
                if col >= cols {
                    continue;
                }
                cells[row * cols + col] = if !segment.wide
                    && !cell.italic
                    && !cell.underline
                    && !cell.strikethrough
                    && matches!(cell.text.as_str(), "/" | "\\")
                {
                    3
                } else {
                    BLOCKED
                };
                if segment.wide && col + 1 < cols {
                    cells[row * cols + col + 1] = BLOCKED;
                }
            }
        }
        // 路径或正文里的孤立斜线不参与连通，避免把 Logo 下面的提示符并进图案。
        for row in 0..rows {
            let mut col = 0;
            while col < cols {
                if cells[row * cols + col] != 3 {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < cols && cells[row * cols + col] == 3 {
                    col += 1;
                }
                if col - start < 8 {
                    cells[row * cols + start..row * cols + col].fill(BLOCKED);
                }
            }
        }
        // 带背景、图像或公式的区域具有其他呈现语义，不把它们当作空白画布。
        for run in &snap.bg_runs {
            if usize::from(run.row) < rows {
                for col in usize::from(run.start)..usize::from(run.end).min(cols) {
                    cells[usize::from(run.row) * cols + col] = BLOCKED;
                }
            }
        }
        for run in &snap.image_runs {
            if usize::from(run.row) < rows {
                for col in usize::from(run.column)
                    ..(usize::from(run.column) + usize::from(run.columns)).min(cols)
                {
                    cells[usize::from(run.row) * cols + col] = BLOCKED;
                }
            }
        }
        let valid = |area: Area, cells: &[u8]| {
            if let Some(cursor) = &snap.cursor
                && (area.top..area.bottom).contains(&(viewport_top + i64::from(cursor.row)))
                && (area.left..area.right).contains(&usize::from(cursor.col))
            {
                return false;
            }
            let mut found = false;
            for absolute in area.top.max(viewport_top)..area.bottom.min(viewport_top + rows as i64)
            {
                let row = (absolute - viewport_top) as usize;
                if (project_row(row as f32 + 1.0) - project_row(row as f32) - 1.0).abs() > 0.001 {
                    return false;
                }
                for col in area.left..area.right {
                    let cell = kind(cells[row * cols + col]);
                    if (cell != 0 && cell != area.kind) || !allowed(row, col) {
                        return false;
                    }
                    found |= cell == area.kind;
                }
            }
            found
        };
        // 边界缓存只用于请求完整源行；每帧都以当前内容重新确认形状，擦除不能留下旧边界。
        self.cached.retain(|area| !area.intersects_rows(viewport_top, rows));
        let mut pending = Vec::new();
        for seed in 0..cells.len() {
            let group = kind(cells[seed]);
            if group == 0 || group == BLOCKED || cells[seed] & VISITED != 0 {
                continue;
            }
            let mut area = Area {
                top: (seed / cols) as i64,
                bottom: (seed / cols + 1) as i64,
                left: seed % cols,
                right: seed % cols + 1,
                kind: group,
            };
            let mut count = 0;
            let mut dense = 0;
            pending.push(seed);
            cells[seed] |= VISITED;
            while let Some(index) = pending.pop() {
                let (row, col) = (index / cols, index % cols);
                count += 1;
                dense += usize::from(cells[index] & DENSE != 0);
                area.top = area.top.min(row as i64);
                area.bottom = area.bottom.max(row as i64 + 1);
                area.left = area.left.min(col);
                area.right = area.right.max(col + 1);
                // 允许图案内部两列/两行空隙，整体空白仍须通过矩形排他性检查。
                for y in row.saturating_sub(3)..=(row + 3).min(rows - 1) {
                    for x in col.saturating_sub(3)..=(col + 3).min(cols - 1) {
                        let next = y * cols + x;
                        if kind(cells[next]) == group && cells[next] & VISITED == 0 {
                            cells[next] |= VISITED;
                            pending.push(next);
                        }
                    }
                }
            }
            let complete = (area.top > 0 || viewport_top == 0) && area.bottom < rows as i64;
            area.top += viewport_top;
            area.bottom += viewport_top;
            if !complete
                || count < 32
                || area.bottom - area.top < 4
                || area.right - area.left < 8
                || (group != 3 && dense < 8)
                || !valid(area, &cells)
            {
                continue;
            }
            if group == 3
                && (area.top..area.bottom).any(|absolute| {
                    let row = (absolute - viewport_top) as usize;
                    let amount = (area.left..area.right)
                        .filter(|&col| kind(cells[row * cols + col]) == 3)
                        .count();
                    amount != 0 && amount < 8
                })
            {
                continue;
            }
            self.cached.push(area);
        }
        if self.cached.len() > MAX_REGIONS {
            self.cached.drain(..self.cached.len() - MAX_REGIONS);
        }
        let ratio = width * 2.0 / height;
        for area in self.cached.iter().filter(|area| area.intersects_rows(viewport_top, rows)) {
            let source_top = (area.top - viewport_top) as f32;
            self.frame.regions.push(Region {
                area: *area,
                source_top,
                visual_top: project_row(source_top),
                padding: (area.bottom - area.top) as f32 * (1.0 - ratio) * 0.5,
                ratio,
            });
        }
    }
}

#[cfg(test)]
mod tests;
