//! The same parsed heading index drives source navigation and preview blocks.
//! Split only at root block boundaries; fenced code, lists and quotes remain intact.

use markdown::mdast::Node;
use std::collections::{HashMap, HashSet};

const MAX_PREVIEW_BYTES: usize = 512 * 1024;
const MAX_BLOCK_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Heading {
    pub(super) label: String,
    pub(super) depth: u8,
    pub(super) row: u32,
    pub(super) block: usize,
    pub(super) parent: Option<usize>,
    pub(super) indent: usize,
}

#[derive(Default)]
pub(super) struct Outline {
    pub(super) headings: Vec<Heading>,
    pub(super) blocks: Vec<String>,
    pub(super) limited: bool,
    definitions: HashMap<String, (String, Vec<String>)>,
    references: Vec<Vec<String>>,
    pub(super) source_ranges: Vec<std::ops::Range<usize>>,
    pub(super) structures: Vec<Option<std::sync::Arc<super::block_structure::BlockStructure>>>,
    heading_levels: Vec<Option<u8>>,
}

impl Outline {
    /// An empty paragraph has no AST node, but still needs a caret between the
    /// surrounding blocks. Its source position disappears on the next parse.
    pub(super) fn edit_block_at(&mut self, offset: usize) -> usize {
        if let Some(index) = self
            .source_ranges
            .iter()
            .position(|range| range.contains(&offset) || range.end == offset)
        {
            return index;
        }
        let index = self.source_ranges.partition_point(|range| range.start < offset);
        self.source_ranges.insert(index, offset..offset);
        self.blocks.insert(index, String::new());
        self.references.insert(index, Vec::new());
        self.heading_levels.insert(index, None);
        self.structures.insert(index, None);
        for heading in &mut self.headings {
            if heading.block >= index {
                heading.block += 1;
            }
        }
        index
    }

    pub(super) fn heading_level(&self, block: usize) -> Option<u8> {
        self.heading_levels.get(block).copied().flatten()
    }

    pub(super) fn prepare(source: &str, base: Option<&std::path::Path>) -> Self {
        let mut outline = Self::parse(source);
        // Rendering may substitute cached still images, but editing always uses
        // byte ranges in the original source, never the rewritten preview text.
        for (block, structure) in outline.blocks.iter_mut().zip(&mut outline.structures) {
            if let Some(structure) = structure {
                for part in &mut std::sync::Arc::make_mut(structure).parts {
                    let source = &block[part.range.clone()];
                    let preview = super::images::rewrite_doc_images(source, base);
                    part.preview = (preview != source).then_some(preview);
                }
            } else {
                *block = super::images::rewrite_doc_images(block, base);
            }
        }
        for (definition, _) in outline.definitions.values_mut() {
            *definition = super::images::rewrite_doc_images(definition, base);
        }
        outline
    }

    pub(super) fn block_source(&self, index: usize) -> String {
        let Some(block) = self.blocks.get(index) else { return String::new() };
        let Some(definitions) = self.reference_sources(index) else {
            return literal_preview(block);
        };
        let mut text = block.clone();
        for definition in definitions {
            text.push_str("\n\n");
            text.push_str(definition);
        }
        text
    }

    fn reference_sources(&self, index: usize) -> Option<Vec<&str>> {
        let mut bytes = self.blocks.get(index)?.len();
        if bytes > MAX_BLOCK_BYTES {
            return None;
        }
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut pending: Vec<&str> =
            self.references.get(index).into_iter().flatten().map(String::as_str).collect();
        while let Some(reference) = pending.pop() {
            if !visited.insert(reference) {
                continue;
            }
            if let Some((definition, references)) = self.definitions.get(reference) {
                bytes = bytes.saturating_add(definition.len() + 2);
                if bytes > MAX_BLOCK_BYTES {
                    return None;
                }
                result.push(definition.as_str());
                pending.extend(references.iter().map(String::as_str));
            }
        }
        Some(result)
    }

    pub(super) fn parse(source: &str) -> Self {
        let limited = source.len() > MAX_PREVIEW_BYTES;
        let source = &source[..source.floor_char_boundary(MAX_PREVIEW_BYTES.min(source.len()))];
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_flow = true;
        options.constructs.math_text = true;
        let Ok(Node::Root(root)) = markdown::to_mdast(source, &options) else {
            return Self {
                headings: vec![],
                blocks: vec![source.to_owned()],
                source_ranges: vec![0..source.len()],
                heading_levels: vec![None],
                limited,
                ..Self::default()
            };
        };
        // Resolve reference links/images from any part of the original document.
        let definitions = root
            .children
            .iter()
            .filter_map(|node| {
                if limited && node.position().is_some_and(|p| p.end.offset == source.len()) {
                    return None;
                }
                let key = match node {
                    Node::Definition(definition) => {
                        format!("link:{}", identifier(&definition.identifier))
                    },
                    Node::FootnoteDefinition(definition) => {
                        format!("note:{}", identifier(&definition.identifier))
                    },
                    _ => return None,
                };
                let position = node.position()?;
                let text = source.get(position.start.offset..position.end.offset)?.to_owned();
                let mut references = Vec::new();
                collect_references(node, &mut references);
                Some((key, (text, references)))
            })
            .fold(HashMap::new(), |mut definitions, (key, value)| {
                definitions.entry(key).or_insert(value);
                definitions
            });
        let mut result = Self { definitions, limited, ..Self::default() };
        for node in &root.children {
            if matches!(node, Node::Definition(_) | Node::FootnoteDefinition(_)) {
                continue;
            }
            let Some(position) = node.position() else { continue };
            let Some(text) = source.get(position.start.offset..position.end.offset) else {
                continue;
            };
            collect_headings(node, result.blocks.len(), &mut result.headings);
            result.source_ranges.push(position.start.offset..position.end.offset);
            result.heading_levels.push(if let Node::Heading(heading) = node {
                Some(heading.depth)
            } else {
                None
            });
            let mut references = Vec::new();
            collect_references(node, &mut references);
            result.references.push(references);
            let unfinished =
                limited && node.position().is_some_and(|p| p.end.offset == source.len());
            if unfinished || text.len() > MAX_BLOCK_BYTES {
                result.limited = true;
                result.references.last_mut().unwrap().clear();
                result.blocks.push(literal_preview(text));
                result.structures.push(None);
            } else {
                result.blocks.push(text.to_owned());
                result.structures.push(
                    super::block_structure::BlockStructure::from_node(
                        node,
                        text,
                        position.start.offset,
                    )
                    .map(std::sync::Arc::new),
                );
            }
        }
        link_heading_parents(&mut result.headings);
        let references_limited =
            (0..result.blocks.len()).any(|index| result.reference_sources(index).is_none());
        result.limited |= references_limited;
        result
    }

    pub(super) fn part_source(&self, index: usize, text: &str) -> String {
        let mut source = text.to_owned();
        if let Some(definitions) = self.reference_sources(index) {
            for definition in definitions {
                source.push_str("\n\n");
                source.push_str(definition);
            }
        }
        source
    }

    pub(super) fn visible_headings<'a>(
        &'a self,
        collapsed: &'a HashSet<usize>,
    ) -> impl Iterator<Item = (usize, &'a Heading)> + 'a {
        let mut hidden_below = None;
        self.headings.iter().enumerate().filter(move |(index, heading)| {
            if hidden_below.is_some_and(|depth| heading.depth > depth) {
                return false;
            }
            hidden_below = collapsed.contains(index).then_some(heading.depth);
            true
        })
    }

    pub(super) fn has_children(&self, index: usize) -> bool {
        self.headings.get(index + 1).is_some_and(|next| next.parent == Some(index))
    }

    /// Preview code actions must stay inside the block, not appended definitions.
    pub(super) fn source_span(
        &self,
        block: usize,
        start: usize,
        end: usize,
    ) -> Option<(usize, usize)> {
        self.blocks.get(block)?.get(start..end)?;
        Some((start, end))
    }

    pub(super) fn replace_block(&mut self, block: usize, next: String) {
        if let Some(source) = self.blocks.get_mut(block) {
            *source = next;
        }
    }
}

fn link_heading_parents(headings: &mut [Heading]) {
    let mut stack: Vec<usize> = Vec::new();
    for index in 0..headings.len() {
        while stack.last().is_some_and(|parent| headings[*parent].depth >= headings[index].depth) {
            stack.pop();
        }
        headings[index].parent = stack.last().copied();
        headings[index].indent = stack.len();
        stack.push(index);
    }
}

fn literal_preview(source: &str) -> String {
    let end = source.floor_char_boundary(MAX_BLOCK_BYTES.min(source.len()));
    let source = &source[..end];
    let fence =
        "`".repeat(source.split(|ch| ch != '`').map(str::len).max().unwrap_or(0).max(2) + 1);
    format!("{fence}text\n{source}\n{fence}")
}

fn identifier(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

fn collect_references(node: &Node, output: &mut Vec<String>) {
    match node {
        Node::LinkReference(reference) => {
            output.push(format!("link:{}", identifier(&reference.identifier)))
        },
        Node::ImageReference(reference) => {
            output.push(format!("link:{}", identifier(&reference.identifier)))
        },
        Node::FootnoteReference(reference) => {
            output.push(format!("note:{}", identifier(&reference.identifier)))
        },
        _ => {},
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_references(child, output);
        }
    }
}

fn label(node: &Node, text: &mut String) {
    match node {
        Node::Text(node) => text.push_str(&node.value),
        Node::InlineCode(node) => text.push_str(&node.value),
        Node::InlineMath(node) => text.push_str(&node.value),
        Node::Image(node) => text.push_str(&node.alt),
        Node::ImageReference(node) => text.push_str(&node.alt),
        Node::Break(_) => text.push(' '),
        _ => {
            if let Some(children) = node.children() {
                for child in children {
                    label(child, text);
                }
            }
        },
    }
}

fn collect_headings(node: &Node, block: usize, headings: &mut Vec<Heading>) {
    if let Node::Heading(heading) = node {
        let mut text = String::new();
        label(node, &mut text);
        headings.push(Heading {
            label: text,
            depth: heading.depth,
            row: heading.position.as_ref().map_or(0, |p| p.start.line.saturating_sub(1) as u32),
            block,
            parent: None,
            indent: 0,
        });
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_headings(child, block, headings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_cursor_at_paragraph_end_reuses_the_existing_block() {
        let mut outline = Outline::parse("Paragraph\n\nNext");
        assert_eq!(outline.edit_block_at("Paragraph".len()), 0);
        assert_eq!(outline.blocks.len(), 2);
        assert_eq!(outline.edit_block_at("Paragraph\n".len()), 1);
        assert_eq!(outline.source_ranges[1], 10..10);
    }

    #[test]
    fn authored_headings_and_nested_folds_share_one_tree_without_rewriting_source() {
        let source = "# First\n\n## Sub\n\n### Deep\n\n## Other\n\n# Next\n";
        let outline = Outline::parse(source);
        assert_eq!(outline.headings.iter().map(|h| h.indent).collect::<Vec<_>>(), [0, 1, 2, 1, 0]);
        assert_eq!(outline.blocks[0], "# First");
        assert_eq!(outline.block_source(0), "# First");
        assert_eq!(outline.headings[1].label, "Sub");
        let mut collapsed = HashSet::from([0, 1]);
        assert_eq!(
            outline.visible_headings(&collapsed).map(|(i, _)| i).collect::<Vec<_>>(),
            [0, 4]
        );
        collapsed.remove(&0);
        assert_eq!(
            outline.visible_headings(&collapsed).map(|(i, _)| i).collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
        assert!(outline.has_children(1));
        assert!(!outline.has_children(2));
    }

    #[test]
    fn preview_preserves_setext_inline_markup_and_code_action_offsets() {
        let outline = Outline::parse(
            "Title\n===\n\n## **Bold**\n\n> ### Quote\n>\n> ```rust\n> old\n> ```\n",
        );
        assert_eq!(outline.block_source(0), "Title\n===");
        assert_eq!(outline.block_source(1), "## **Bold**");
        let shown = outline.block_source(2);
        assert!(shown.starts_with("> ### Quote"));
        let displayed = shown.find("```rust").unwrap();
        let raw = outline.blocks[2].find("```rust").unwrap();
        assert_eq!(outline.source_span(2, displayed, displayed + 7), Some((raw, raw + 7)));
        assert_eq!(Outline::parse("# 1. Existing").block_source(0), "# 1. Existing");
    }

    #[test]
    fn headings_keep_hierarchy_source_rows_and_duplicate_targets() {
        let outline = Outline::parse(
            "# 标题 *one*\n\n```md\n# not a heading\n```\n\nTitle\n---\n\n## 标题 `two`\n\n## 标题 `two`\n",
        );
        assert_eq!(
            outline.headings.iter().map(|h| (h.label.as_str(), h.depth, h.row)).collect::<Vec<_>>(),
            [("标题 one", 1, 0), ("Title", 2, 6), ("标题 two", 2, 9), ("标题 two", 2, 11)]
        );
        assert_ne!(outline.headings[2].block, outline.headings[3].block);
        assert!(outline.blocks.iter().any(|text| text.starts_with("```md\n# not")));
    }

    #[test]
    fn nested_headings_keep_their_container_and_references_resolve_across_blocks() {
        let outline = Outline::parse(
            "> ## [linked][ref]\n> body\n\n![image][img]\n\n[ref]: https://example.com\n[img]: images/test.png\n",
        );
        assert_eq!(outline.headings[0].label, "linked");
        assert_eq!(outline.headings[0].block, 0);
        assert!(outline.blocks[0].starts_with("> ##"));
        assert!(outline.block_source(0).contains("[ref]: https://example.com"));
        assert!(outline.block_source(1).contains("[img]: images/test.png"));
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn unrelated_reference_definitions_are_not_copied_into_every_block() {
        let mut source = (0..500).map(|id| format!("paragraph {id}\n\n")).collect::<String>();
        source.push_str("![selected][img-499]\n\n");
        for id in 0..500 {
            source.push_str(&format!("[img-{id}]: local-{id}.png\n"));
        }
        let outline = Outline::parse(&source);
        assert!(outline.block_source(0).len() < 30);
        let referenced = outline.block_source(outline.blocks.len() - 1);
        assert!(referenced.contains("local-499.png"));
        assert!(!referenced.contains("local-0.png"));
        assert!(outline.blocks.iter().map(String::len).sum::<usize>() < source.len());
    }
}

#[cfg(test)]
mod preview_budget_tests {
    use super::*;
    #[test]
    fn large_document_is_bounded_before_ast_construction_and_partial_math_is_literal() {
        let source = "paragraph\n\n".repeat(MAX_PREVIEW_BYTES / 11 + 100);
        let outline = Outline::parse(&source);
        assert!(outline.limited);
        assert!(outline.blocks.iter().map(String::len).sum::<usize>() <= MAX_PREVIEW_BYTES + 128);
        let source = format!("$$\n{}\n$$", "x+".repeat(MAX_BLOCK_BYTES));
        let outline = Outline::parse(&source);
        assert!(outline.limited);
        assert!(outline.blocks[0].starts_with("```text"));
    }

    #[test]
    fn first_reference_definition_keeps_markdown_precedence() {
        let outline = Outline::parse("[label][ref]\n\n[ref]: first.png\n[ref]: second.png\n");
        assert!(outline.block_source(0).contains("first.png"));
        assert!(!outline.block_source(0).contains("second.png"));
    }
}

#[cfg(test)]
mod reference_budget_tests {
    use super::*;
    #[test]
    fn transitive_footnotes_cannot_bypass_the_preview_block_budget() {
        let mut source = String::from("body[^n0]\n\n");
        for index in 0..100 {
            source.push_str(&format!(
                "[^n{index}]: {} [^n{}]\n\n",
                "x".repeat(512),
                (index + 1) % 100
            ));
        }
        let outline = Outline::parse(&source);
        assert!(outline.limited);
        let preview = outline.block_source(0);
        assert!(preview.starts_with("```text"));
        assert!(preview.len() < 128);
    }
}
