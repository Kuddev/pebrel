//! Source spans for mixed paragraphs. Only embedded objects split a native text
//! run; surrounding Markdown remains untouched in the canonical document.

use super::block_structure::{EditPart, PartKind, StructureNode, add, span};
use super::inline_edit::Marks;
use markdown::mdast::Node;

const MAX_INLINE_RUNS: usize = 128;

#[derive(Clone, Debug)]
pub(super) struct InlineRun {
    pub part: usize,
    pub marks: Marks,
}

pub(super) fn contains_object(node: &Node) -> bool {
    is_object(node) || node.children().is_some_and(|children| children.iter().any(contains_object))
}

fn is_object(node: &Node) -> bool {
    matches!(node, Node::InlineMath(_) | Node::Image(_) | Node::ImageReference(_))
}

pub(super) fn build(
    node: &Node,
    source: &str,
    offset: usize,
    parts: &mut Vec<EditPart>,
) -> Option<StructureNode> {
    let initial = parts.len();
    let heading = if let Node::Heading(heading) = node { Some(heading.depth) } else { None };
    let mut builder = Builder { source, offset, parts, runs: vec![], heading };
    let built = node.children().is_some_and(|children| builder.collect(children, Marks::default()));
    if !built {
        // Do not instantiate a TextView per tiny object in a pathological block.
        // A literal preview and a single source input keep this fallback bounded.
        builder.parts.truncate(initial);
        return Some(StructureNode::Literal(add(
            builder.parts,
            span(node, offset)?,
            PartKind::Source,
            heading,
        )));
    }
    Some(StructureNode::Inline(builder.runs))
}

struct Builder<'a> {
    source: &'a str,
    offset: usize,
    parts: &'a mut Vec<EditPart>,
    runs: Vec<InlineRun>,
    heading: Option<u8>,
}

impl Builder<'_> {
    fn collect(&mut self, nodes: &[Node], marks: Marks) -> bool {
        for node in nodes {
            if !is_object(node) && contains_object(node) {
                let mut inherited = marks;
                match node {
                    Node::Strong(_) => inherited.bold = true,
                    Node::Emphasis(_) => inherited.italic = true,
                    Node::Delete(_) => inherited.strike = true,
                    Node::Link(_) | Node::LinkReference(_) => inherited.link = true,
                    _ => {},
                }
                if let Some(children) = node.children() {
                    if !self.collect(children, inherited) {
                        return false;
                    }
                    continue;
                }
            }
            let Some(range) = span(node, self.offset) else { return false };
            if !is_object(node) {
                if let Some(previous) = self.runs.last() {
                    let part = &mut self.parts[previous.part];
                    if matches!(part.kind, PartKind::Rich)
                        && previous.marks == marks
                        && part.range.end <= range.start
                        && self
                            .source
                            .get(part.range.end..range.start)
                            .is_some_and(|gap| gap.chars().all(char::is_whitespace))
                    {
                        part.range.end = range.end;
                        continue;
                    }
                }
            }
            if self.runs.len() == MAX_INLINE_RUNS {
                return false;
            }
            let kind = if is_object(node) { PartKind::Source } else { PartKind::Rich };
            let part = add(self.parts, range, kind, self.heading);
            self.runs.push(InlineRun { part, marks });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpui_shell::file_editor::block_structure::BlockStructure;

    fn parse(source: &str) -> BlockStructure {
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_text = true;
        let Node::Root(root) = markdown::to_mdast(source, &options).unwrap() else { panic!() };
        BlockStructure::from_node(&root.children[0], source, 0).unwrap()
    }

    #[test]
    fn identical_objects_have_distinct_source_spans_and_keep_nested_marks() {
        let source = "前 **bold $x$ after** ![pic](a.png) $x$ 后";
        let structure = parse(source);
        let StructureNode::Inline(runs) = &structure.root else { panic!() };
        let formulas: Vec<_> = runs
            .iter()
            .filter(|run| &source[structure.parts[run.part].range.clone()] == "$x$")
            .collect();
        assert_eq!(formulas.len(), 2);
        assert!(formulas[0].marks.bold);
        assert!(!formulas[1].marks.bold);
        assert_ne!(
            structure.parts[formulas[0].part].range,
            structure.parts[formulas[1].part].range
        );
        assert!(structure.parts.iter().any(|part| &source[part.range.clone()] == "![pic](a.png)"));
    }

    #[test]
    fn preparing_inline_images_keeps_source_offsets_and_uses_a_still_preview() {
        let directory = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(directory.path().join("tiny.gif")).unwrap();
        image::codecs::gif::GifEncoder::new(file)
            .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                2,
                2,
                image::Rgba([0, 0, 0, 255]),
            )))
            .unwrap();
        let source = "Before ![alt](tiny.gif) after";
        let outline = crate::gpui_shell::file_editor::outline::Outline::prepare(
            source,
            Some(directory.path()),
        );
        let structure = outline.structures[0].as_ref().unwrap();
        let image = structure
            .parts
            .iter()
            .find(|part| &source[part.range.clone()] == "![alt](tiny.gif)")
            .unwrap();
        let preview = image.preview.as_deref().unwrap();
        assert!(preview.contains(".png)"), "{preview}");
        assert_eq!(outline.source_ranges[0], 0..source.len());
    }

    #[test]
    fn excessive_objects_have_one_literal_part_and_no_partial_allocation() {
        let source = "$x$ ".repeat(MAX_INLINE_RUNS + 1);
        let structure = parse(&source);
        assert!(matches!(structure.root, StructureNode::Literal(0)));
        assert_eq!(structure.parts.len(), 1);
    }
}
