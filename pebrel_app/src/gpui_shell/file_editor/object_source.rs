//! 对象草稿只序列化当前源区间；引用图片改成独立图片时不影响其他引用。

use super::block_structure::PartKind;
use super::outline::Outline;
use crate::i18n::Message;
use markdown::mdast::Node;
use std::path::Path;

#[derive(Clone)]
pub(super) enum ObjectSource {
    Math { fence: String, display: bool },
    Image { title: Option<String> },
}

pub(super) struct ObjectValue {
    pub syntax: ObjectSource,
    pub text: String,
    pub alt: Option<String>,
}

pub(super) struct ObjectDraft {
    pub replacement: String,
    pub preview: String,
    pub image: Option<gpui::Resource>,
}

impl ObjectValue {
    pub(super) fn parse(
        kind: &PartKind,
        source: &str,
        outline: &Outline,
        block: usize,
    ) -> Option<Self> {
        if matches!(kind, PartKind::Math) {
            return Some(Self {
                syntax: ObjectSource::Math { fence: String::new(), display: true },
                text: source.to_owned(),
                alt: None,
            });
        }
        let mut options = markdown::ParseOptions::gfm();
        options.constructs.math_text = true;
        let Node::Root(root) =
            markdown::to_mdast(&outline.part_source(block, source), &options).ok()?
        else {
            return None;
        };
        let Node::Paragraph(paragraph) = root.children.first()? else { return None };
        if paragraph.children.len() != 1 {
            return None;
        }
        Some(match &paragraph.children[0] {
            Node::InlineMath(math) => {
                let count = source.bytes().take_while(|byte| *byte == b'$').count();
                Self {
                    syntax: ObjectSource::Math { fence: "$".repeat(count), display: false },
                    text: math.value.clone(),
                    alt: None,
                }
            },
            Node::Image(image) => Self {
                syntax: ObjectSource::Image { title: image.title.clone() },
                text: image.url.clone(),
                alt: Some(image.alt.clone()),
            },
            Node::ImageReference(image) => {
                let (url, title) = outline.image_definition(&image.identifier)?;
                Self {
                    syntax: ObjectSource::Image { title: title.clone() },
                    text: url.clone(),
                    alt: Some(image.alt.clone()),
                }
            },
            _ => return None,
        })
    }
}

impl ObjectSource {
    pub(super) fn draft(
        &self,
        text: &str,
        alt: &str,
        base: Option<&Path>,
    ) -> Result<ObjectDraft, Message> {
        match self {
            Self::Math { fence, display } => {
                if text.trim().is_empty() || text.len() > 4096 {
                    return Err(Message::EditorObjectInvalidMath);
                }
                crate::math::parser::parse_formula_source(
                    text,
                    *display,
                    crate::math::DEFAULT_LIMITS,
                )
                .map_err(|_| Message::EditorObjectInvalidMath)?;
                let replacement =
                    if *display { text.to_owned() } else { format!("{fence}{text}{fence}") };
                let preview =
                    if *display { format!("$$\n{text}\n$$") } else { replacement.clone() };
                // 定界符仍属于文档语法；有效 TeX 也不能关闭当前对象并注入相邻段落。
                let mut options = markdown::ParseOptions::gfm();
                options.constructs.math_flow = true;
                options.constructs.math_text = true;
                let parsed = markdown::to_mdast(&preview, &options).ok();
                let valid = matches!(parsed, Some(Node::Root(ref root)) if root.children.len() == 1
                    && if *display { matches!(root.children[0], Node::Math(_)) }
                    else { matches!(&root.children[0], Node::Paragraph(p) if p.children.len() == 1 && matches!(p.children[0], Node::InlineMath(_))) });
                if !valid {
                    return Err(Message::EditorObjectInvalidMath);
                }
                Ok(ObjectDraft { replacement, preview, image: None })
            },
            Self::Image { title } => {
                if text.trim().is_empty()
                    || text.len() > 8192
                    || alt.len() > 2000
                    || text.contains(['\n', '\r', '\0'])
                    || alt.contains(['\n', '\r', '\0'])
                {
                    return Err(Message::EditorObjectInvalidImage);
                }
                let input = text.trim();
                let file_path = if input.to_ascii_lowercase().starts_with("file:") {
                    Some(
                        gpui::http_client::Url::parse(input)
                            .ok()
                            .and_then(|url| url.to_file_path().ok())
                            .ok_or(Message::EditorObjectInvalidImage)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    )
                } else {
                    None
                };
                let address = file_path.as_deref().unwrap_or(input);
                let rendered = super::images::flatten_local_image_url(address, base);
                let resource = image_resource(&rendered, base)?;
                let preview_address = match &resource {
                    gpui::Resource::Uri(uri) => uri.as_ref(),
                    _ => rendered.as_str(),
                };
                Ok(ObjectDraft {
                    replacement: image_markdown(address, alt, title.as_deref()),
                    preview: image_markdown(preview_address, alt, title.as_deref()),
                    image: Some(resource),
                })
            },
        }
    }
}

fn image_resource(address: &str, base: Option<&Path>) -> Result<gpui::Resource, Message> {
    use gpui::Resource;
    let lower = address.to_ascii_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("data:") {
        let url = gpui::http_client::Url::parse(address)
            .map_err(|_| Message::EditorObjectInvalidImage)?;
        if url.scheme() != "data" && url.host_str().is_none() {
            return Err(Message::EditorObjectInvalidImage);
        }
        return Ok(Resource::Uri(url.to_string().into()));
    }
    if address.contains("://") {
        return Err(Message::EditorObjectInvalidImage);
    }
    let path = Path::new(address);
    Ok(Resource::Path(
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.unwrap_or_else(|| Path::new(".")).join(path)
        }
        .into(),
    ))
}

fn escaped(value: &str, punctuation: &[char]) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch == '&' {
            result.push_str("&amp;");
        } else {
            if ch == '\\' || punctuation.contains(&ch) {
                result.push('\\');
            }
            result.push(ch);
        }
    }
    result
}

pub(super) fn image_markdown(address: &str, alt: &str, title: Option<&str>) -> String {
    let address = escaped(address, &['<', '>']);
    let title = title.map(|title| format!(" \"{}\"", escaped(title, &['"']))).unwrap_or_default();
    format!("![{}](<{address}>{title})", escaped(alt, &['[', ']', '*', '_', '`', '~', '<', '>']))
}
