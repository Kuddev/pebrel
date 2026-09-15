//! `docs/math-rendering-test.md` 语料守护测试。独立成文件，避免撑破
//! `markdown_view.rs` 的行数预算（同 `gitignore_tests.rs` 的先例）。

use crate::markdown::{FragmentContent, FormattedTextLine, MathSource};
use crate::math::{DEFAULT_LIMITS, compile_formula};

#[test]
fn math_rendering_fixture_uses_native_layout_for_every_formula() {
    let raw = include_str!("../../../docs/math-rendering-test.md");
    let document = crate::markdown::parse_markdown(raw);
    // 语料中的 `<!-- pebrel-test: source-fallback -->` / `preserve-source`
    // 负例（\pu、故意坏输入等）按合同不进排版引擎；取公式源码前最近的
    // pebrel-test 标记判定期望，与 scientific_corpus 语料测试同源。
    let expects_native = |source: &MathSource| -> bool {
        let marker_end = match &source.source {
            crate::markdown::TextRef::Source { range, .. } => range.as_usize().start,
            crate::markdown::TextRef::Generated(_) => return true,
        };
        match raw[..marker_end].rfind("<!-- pebrel-test:") {
            Some(index) => {
                let marker = &raw[index..marker_end];
                !marker.contains("source-fallback") && !marker.contains("preserve-source")
            },
            None => true,
        }
    };
    let mut formula_count = 0usize;
    let mut check = |source: &MathSource, display: bool| {
        if !expects_native(source) {
            return;
        }
        let layout = compile_formula(source.as_str(), display, 18.0, 1.0, DEFAULT_LIMITS)
            .unwrap_or_else(|error| panic!("fixture compile failed for {:?}: {error:?}", source));
        assert!(layout.metrics.width.is_finite() && layout.metrics.width > 0.0);
        formula_count += 1;
    };

    for line in &document.lines {
        match line {
            FormattedTextLine::DisplayMath(source) => check(source, true),
            FormattedTextLine::Line(inline)
            | FormattedTextLine::Heading(crate::markdown::FormattedTextHeader {
                text: inline,
                ..
            }) => {
                for fragment in inline {
                    if let FragmentContent::Math(source) = &fragment.content {
                        check(source, false);
                    }
                }
            },
            _ => {},
        }
    }

    assert!(formula_count >= 25, "fixture should cover many formulas");
}
