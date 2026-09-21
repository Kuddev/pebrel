//! Source-backed editable parts. Containers own layout, while each input changes
//! only its content span; pipes, list markers and fences never enter that input.

pub(super) use super::block_inline::InlineRun;
use markdown::mdast::{AlignKind, Node};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) enum PartKind {
    Rich,
    Code(String),
    Math,
    Source,
}

#[derive(Clone, Debug)]
pub(super) struct EditPart {
    pub range: Range<usize>,
    pub kind: PartKind,
    pub heading: Option<u8>,
    pub preview: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ListItem {
    pub range: Range<usize>,
    pub prefix: Range<usize>,
    pub marker: String,
    pub check: Option<Range<usize>>,
    pub children: Vec<StructureNode>,
}

#[derive(Clone, Debug)]
pub(super) enum StructureNode {
    Text(usize),
    Inline(Vec<InlineRun>),
    Literal(usize),
    Code { part: usize, span: Range<usize>, language: String },
    Math { part: usize, span: Range<usize> },
    List(Vec<ListItem>),
    Quote(Vec<StructureNode>),
    Table { rows: Vec<Vec<usize>>, align: Vec<AlignKind>, span: Range<usize> },
}

#[derive(Clone, Debug)]
pub(super) struct BlockStructure {
    pub root: StructureNode,
    pub parts: Vec<EditPart>,
}

impl BlockStructure {
    pub fn from_node(node: &Node, source: &str, offset: usize) -> Option<Self> {
        if !matches!(
            node,
            Node::List(_) | Node::Table(_) | Node::Code(_) | Node::Math(_) | Node::Blockquote(_)
        ) && !(matches!(node, Node::Paragraph(_) | Node::Heading(_))
            && super::block_inline::contains_object(node))
        {
            return None;
        }
        let mut parts = Vec::new();
        let root = build(node, source, offset, &mut parts)?;
        Some(Self { root, parts })
    }

    pub fn part_at(&self, offset: usize) -> Option<usize> {
        self.parts
            .iter()
            .position(|part| part.range.contains(&offset))
            .or_else(|| self.parts.iter().position(|part| part.range.end == offset))
            .or_else(|| self.parts.iter().position(|part| part.range.start >= offset))
            .or_else(|| self.parts.len().checked_sub(1))
    }

    pub fn code_part(&self, span: &Range<usize>) -> Option<usize> {
        self.parts.iter().position(|part| {
            matches!(part.kind, PartKind::Code(_))
                && part.range.start >= span.start
                && part.range.end <= span.end
        })
    }

    pub fn replace_part(&mut self, part: usize, length: usize) {
        let old = self.parts[part].range.clone();
        let delta = length as isize - old.len() as isize;
        self.parts[part].range.end = old.start + length;
        self.parts[part].preview = None;
        for (i, edit) in self.parts.iter_mut().enumerate() {
            if i != part {
                shift(&mut edit.range, &old, delta);
            }
        }
        self.root.shift(&old, delta);
    }
}

impl StructureNode {
    fn shift(&mut self, old: &Range<usize>, delta: isize) {
        match self {
            Self::Text(_) | Self::Literal(_) | Self::Inline(_) => {},
            Self::Code { span, .. } | Self::Math { span, .. } | Self::Table { span, .. } => {
                shift_container(span, old, delta);
            },
            Self::List(items) => {
                for item in items {
                    shift_container(&mut item.range, old, delta);
                    shift(&mut item.prefix, old, delta);
                    if let Some(check) = &mut item.check {
                        shift(check, old, delta);
                    }
                    for child in &mut item.children {
                        child.shift(old, delta);
                    }
                }
            },
            Self::Quote(children) => {
                for child in children {
                    child.shift(old, delta);
                }
            },
        }
    }

    pub fn list_item(&self, part: usize) -> Option<&ListItem> {
        match self {
            Self::List(items) => items.iter().find_map(|item| {
                item.children.iter().find_map(|child| child.list_item(part)).or_else(|| {
                    item.children
                        .iter()
                        .any(|child| match child {
                            Self::Text(id) | Self::Literal(id) => *id == part,
                            Self::Inline(runs) => runs.iter().any(|run| run.part == part),
                            _ => false,
                        })
                        .then_some(item)
                })
            }),
            Self::Quote(children) => children.iter().find_map(|child| child.list_item(part)),
            _ => None,
        }
    }

    pub fn table(&self, part: usize) -> Option<(&[Vec<usize>], &Range<usize>)> {
        match self {
            Self::Table { rows, span, .. } if rows.iter().flatten().any(|id| *id == part) => {
                Some((rows, span))
            },
            Self::List(items) => {
                items.iter().flat_map(|item| &item.children).find_map(|node| node.table(part))
            },
            Self::Quote(children) => children.iter().find_map(|node| node.table(part)),
            _ => None,
        }
    }
}

fn shift(range: &mut Range<usize>, old: &Range<usize>, delta: isize) {
    if range.start >= old.end {
        range.start = range.start.saturating_add_signed(delta);
        range.end = range.end.saturating_add_signed(delta);
    } else if range.end > old.start {
        range.end = range.end.saturating_add_signed(delta);
    }
}

fn shift_container(range: &mut Range<usize>, old: &Range<usize>, delta: isize) {
    if range.start <= old.start && range.end >= old.end {
        range.end = range.end.saturating_add_signed(delta);
    } else {
        shift(range, old, delta);
    }
}

pub(super) fn span(node: &Node, offset: usize) -> Option<Range<usize>> {
    let position = node.position()?;
    Some(position.start.offset.checked_sub(offset)?..position.end.offset.checked_sub(offset)?)
}

pub(super) fn add(
    parts: &mut Vec<EditPart>,
    range: Range<usize>,
    kind: PartKind,
    heading: Option<u8>,
) -> usize {
    let index = parts.len();
    parts.push(EditPart { range, kind, heading, preview: None });
    index
}

fn build(
    node: &Node,
    source: &str,
    offset: usize,
    parts: &mut Vec<EditPart>,
) -> Option<StructureNode> {
    let range = span(node, offset)?;
    Some(match node {
        Node::List(list) => {
            let mut items = Vec::new();
            for (index, item) in list.children.iter().enumerate() {
                let Node::ListItem(item_data) = item else { return None };
                let item_range = span(item, offset)?;
                let line = source[item_range.clone()].lines().next().unwrap_or_default();
                let marker_len = line.find(char::is_whitespace).unwrap_or(line.len());
                let after_marker = item_range.start + marker_len + line[marker_len..].len()
                    - line[marker_len..].trim_start().len();
                let check = item_data.checked.and_then(|_| {
                    let start =
                        source.get(after_marker..)?.starts_with('[').then_some(after_marker + 1)?;
                    Some(start..start + 1)
                });
                let content =
                    item_data.children.first().and_then(|node| span(node, offset)).map_or_else(
                        || {
                            check.as_ref().map_or(after_marker, |check| {
                                let after = check.end + 1;
                                after + source[after..item_range.end].len()
                                    - source[after..item_range.end].trim_start().len()
                            })
                        },
                        |range| range.start,
                    );
                let children = if item_data.children.is_empty() {
                    vec![StructureNode::Text(add(parts, content..content, PartKind::Rich, None))]
                } else {
                    item_data
                        .children
                        .iter()
                        .map(|node| build(node, source, offset, parts))
                        .collect::<Option<Vec<_>>>()?
                };
                items.push(ListItem {
                    range: item_range.clone(),
                    prefix: item_range.start..content,
                    marker: if list.ordered {
                        format!("{}.", u64::from(list.start.unwrap_or(1)) + index as u64)
                    } else {
                        "•".to_owned()
                    },
                    check,
                    children,
                });
            }
            StructureNode::List(items)
        },
        Node::Table(table) => {
            let mut rows = Vec::new();
            for row in &table.children {
                let Node::TableRow(row) = row else { return None };
                let mut cells = Vec::new();
                for cell in &row.children {
                    let Node::TableCell(cell_data) = cell else { return None };
                    let cell_range = span(cell, offset)?;
                    let content = if let (Some(first), Some(last)) =
                        (cell_data.children.first(), cell_data.children.last())
                    {
                        span(first, offset)?.start..span(last, offset)?.end
                    } else {
                        let raw = source.get(cell_range.clone())?;
                        let start = cell_range.start + raw.len()
                            - raw.trim_start_matches([' ', '\t', '|']).len();
                        start..start
                    };
                    cells.push(add(parts, content, PartKind::Rich, None));
                }
                rows.push(cells);
            }
            StructureNode::Table { rows, align: table.align.clone(), span: range }
        },
        Node::Blockquote(quote) => StructureNode::Quote(if quote.children.is_empty() {
            vec![StructureNode::Text(add(parts, range.end..range.end, PartKind::Rich, None))]
        } else {
            quote
                .children
                .iter()
                .map(|node| build(node, source, offset, parts))
                .collect::<Option<Vec<_>>>()?
        }),
        Node::Code(code) => {
            let body = fenced_body(source, range.clone()).unwrap_or(range.clone());
            let language = code.lang.clone().unwrap_or_default();
            if matches!(language.as_str(), "math" | "latex" | "tex") {
                let part = add(parts, body, PartKind::Math, None);
                return Some(StructureNode::Math { part, span: range });
            }
            let part = add(parts, body, PartKind::Code(language.clone()), None);
            StructureNode::Code { part, span: range, language }
        },
        Node::Math(_) => {
            let body = fenced_body(source, range.clone()).unwrap_or(range.clone());
            let part = add(parts, body, PartKind::Math, None);
            StructureNode::Math { part, span: range }
        },
        Node::Paragraph(_) | Node::Heading(_) if super::block_inline::contains_object(node) => {
            return super::block_inline::build(node, source, offset, parts);
        },
        Node::Paragraph(_) | Node::Heading(_) => StructureNode::Text(add(
            parts,
            range,
            PartKind::Rich,
            if let Node::Heading(heading) = node { Some(heading.depth) } else { None },
        )),
        _ => StructureNode::Text(add(parts, range, PartKind::Source, None)),
    })
}

fn fenced_body(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let text = source.get(range.clone())?;
    let first_end = text.find('\n')?;
    let first = text[..first_end].trim_start();
    let marker = first.chars().next()?;
    if !matches!(marker, '`' | '~' | '$') {
        return None;
    }
    let count = first.chars().take_while(|ch| *ch == marker).count();
    let last_start = text.rfind('\n')? + 1;
    let last = text[last_start..].trim();
    let closes = last.len() >= count && last.chars().all(|ch| ch == marker);
    let end = if closes {
        text[..last_start - 1].trim_end_matches('\r').len().max(first_end + 1)
    } else {
        text.len()
    };
    Some(range.start + first_end + 1..range.start + end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> BlockStructure {
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_flow = true;
        let Node::Root(root) = markdown::to_mdast(source, &options).unwrap() else { panic!() };
        BlockStructure::from_node(&root.children[0], source, 0).unwrap()
    }

    #[test]
    fn table_cells_leave_alignment_delimiters_and_escaped_pipes_outside_inputs() {
        let source = "| **名** | 值 |\n| :--- | ---: |\n| a\\|b |  |";
        let structure = parse(source);
        let texts: Vec<_> =
            structure.parts.iter().map(|part| &source[part.range.clone()]).collect();
        assert_eq!(texts, ["**名**", "值", "a\\|b", ""]);
    }

    #[test]
    fn nested_lists_keep_marker_checkbox_and_child_identity() {
        let source = "- [x] **One**\n  - Child\n- Two";
        let mut structure = parse(source);
        assert_eq!(&source[structure.parts[0].range.clone()], "**One**");
        assert_eq!(&source[structure.parts[1].range.clone()], "Child");
        assert_eq!(&source[structure.root.list_item(0).unwrap().check.clone().unwrap()], "x");
        let before = structure.parts[2].range.clone();
        structure.replace_part(0, "**中文**".len());
        assert_eq!(structure.parts[2].range, before.start + 3..before.end + 3);
    }

    #[test]
    fn code_and_math_edit_bodies_without_touching_authored_fences() {
        for (source, body) in [
            ("~~~~rust title\nlet n = 1;\n~~~~", "let n = 1;"),
            ("$$\na^2\n$$", "a^2"),
            ("```\n```", ""),
        ] {
            let structure = parse(source);
            assert_eq!(&source[structure.parts[0].range.clone()], body);
        }
    }
}
