//! JSON 树保留类型、字段顺序与层级；折叠只是呈现，不改写原始结果。

use std::collections::HashSet;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

pub(crate) struct Node {
    pub key: Option<String>,
    pub parent: Option<usize>,
    pub depth: usize,
    pub kind: Kind,
    pub value: String,
    pub children: Vec<usize>,
}

pub(crate) struct Document {
    pub nodes: Vec<Node>,
}

pub(crate) struct Row {
    pub node: usize,
    pub closing: bool,
    pub depth: usize,
    pub text: String,
    pub expandable: bool,
    pub expanded: bool,
    pub kind: Kind,
}

impl Document {
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        if text.len() > nebula_terminal::inline_content::MAX_CONTENT_BYTES {
            return Err("limit");
        }
        if text.trim().is_empty() {
            return Err("json");
        }
        let value = if text.trim_start().starts_with('[') {
            Value::Array(super::bounded_values(text)?)
        } else {
            serde_json::from_str(text)
                .or_else(|_| super::bounded_values(text).map(Value::Array))
                .map_err(|_| "json")?
        };
        let mut document = Self { nodes: Vec::new() };
        document.push(value, None, None, 0)?;
        Ok(document)
    }

    fn push(
        &mut self,
        value: Value,
        key: Option<String>,
        parent: Option<usize>,
        depth: usize,
    ) -> Result<usize, &'static str> {
        if self.nodes.len() >= super::MAX_TABLE_CELLS {
            return Err("limit");
        }
        let kind = match &value {
            Value::Object(_) => Kind::Object,
            Value::Array(_) => Kind::Array,
            Value::String(_) => Kind::String,
            Value::Number(_) => Kind::Number,
            Value::Bool(_) => Kind::Bool,
            Value::Null => Kind::Null,
        };
        let scalar = if matches!(kind, Kind::Object | Kind::Array) {
            String::new()
        } else {
            value.to_string()
        };
        let id = self.nodes.len();
        self.nodes.push(Node { key, parent, depth, kind, value: scalar, children: Vec::new() });
        let children = match value {
            Value::Object(fields) => fields
                .into_iter()
                .map(|(key, value)| self.push(value, Some(key), Some(id), depth + 1))
                .collect::<Result<_, _>>()?,
            Value::Array(values) => values
                .into_iter()
                .map(|value| self.push(value, None, Some(id), depth + 1))
                .collect::<Result<_, _>>()?,
            _ => Vec::new(),
        };
        self.nodes[id].children = children;
        Ok(id)
    }

    pub fn initially_expanded(&self) -> HashSet<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| {
                (id == 0 || (node.depth == 1 && node.kind == Kind::Object)).then_some(id)
            })
            .collect()
    }

    fn inline(&self, id: usize) -> Option<String> {
        let node = &self.nodes[id];
        if node.children.is_empty() {
            return Some(match node.kind {
                Kind::Object => "{}".into(),
                Kind::Array => "[]".into(),
                _ => node.value.clone(),
            });
        }
        if node.kind != Kind::Array || node.children.len() > 6 {
            return None;
        }
        let mut values = Vec::new();
        let mut length = 4;
        for &child in &node.children {
            let value = &self.nodes[child];
            if matches!(value.kind, Kind::Object | Kind::Array) {
                return None;
            }
            length += value.value.chars().count() + 2;
            if length > 80 {
                return None;
            }
            values.push(value.value.as_str());
        }
        Some(format!("[ {} ]", values.join(", ")))
    }

    fn brief(&self, id: usize) -> String {
        let node = &self.nodes[id];
        match node.kind {
            Kind::Object => "{…}".into(),
            Kind::Array => "[…]".into(),
            _ => shortened(&node.value, 48),
        }
    }

    pub fn preview(&self, id: usize, items: &str) -> String {
        let node = &self.nodes[id];
        if let Some(inline) = self.inline(id) {
            // 标量没有折叠入口，必须保留全文供横向滚动和选中复制。
            return inline;
        }
        let values = node
            .children
            .iter()
            .take(2)
            .map(|&child| {
                let field = &self.nodes[child];
                if node.kind == Kind::Object {
                    let key =
                        serde_json::to_string(field.key.as_deref().unwrap_or_default()).unwrap();
                    format!("{}: {}", shortened(&key, 48), self.brief(child))
                } else if field.kind == Kind::Object {
                    let identity = field.children.iter().copied().find(|&key| {
                        self.nodes[key].key.as_deref().is_some_and(|key| {
                            key.eq_ignore_ascii_case("id")
                                || key.ends_with("Id")
                                || key.ends_with("_id")
                                || key.eq_ignore_ascii_case("uuid")
                                || key.eq_ignore_ascii_case("orderNo")
                        })
                    });
                    identity.map_or_else(|| self.brief(child), |id| self.brief(id))
                } else {
                    self.brief(child)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let tail = if node.children.len() > 2 { ", …" } else { "" };
        if node.kind == Kind::Object {
            format!("{{ {values}{tail} }}")
        } else {
            format!("[ {items} | {values}{tail} ]")
        }
    }

    pub fn rows(
        &self,
        expanded: &HashSet<usize>,
        query: &str,
        item_label: impl Fn(usize) -> String,
    ) -> Vec<Row> {
        let query = query.to_lowercase();
        let mut matches = vec![query.is_empty(); self.nodes.len()];
        if !query.is_empty() {
            for (id, node) in self.nodes.iter().enumerate() {
                matches[id] = node.parent.is_some_and(|parent| matches[parent])
                    || node.key.as_ref().is_some_and(|key| key.to_lowercase().contains(&query))
                    || node.value.to_lowercase().contains(&query);
            }
            for id in (0..self.nodes.len()).rev() {
                if matches[id]
                    && let Some(parent) = self.nodes[id].parent
                {
                    matches[parent] = true;
                }
            }
        }
        let mut rows = Vec::new();
        // 只在查询/折叠变化时生成可见行；每帧渲染仍只实例化视口内的文本。
        let mut pending = vec![(0, false)];
        while let Some((id, closing)) = pending.pop() {
            if !matches[id] {
                continue;
            }
            let node = &self.nodes[id];
            let expandable = !closing && self.inline(id).is_none();
            let open = expandable && (expanded.contains(&id) || !query.is_empty());
            let suffix = if node
                .parent
                .is_some_and(|parent| self.nodes[parent].children.last().copied() != Some(id))
            {
                ","
            } else {
                ""
            };
            let value = if closing {
                if node.kind == Kind::Object { "}".into() } else { "]".into() }
            } else if open {
                if node.kind == Kind::Object { "{".into() } else { "[".into() }
            } else {
                self.preview(id, &item_label(node.children.len()))
            };
            let key = if closing { None } else { node.key.as_ref() };
            let text = format!(
                "{}{}{}",
                key.map_or_else(String::new, |key| format!(
                    "{}: ",
                    serde_json::to_string(key).unwrap()
                )),
                value,
                if open { "" } else { suffix }
            );
            rows.push(Row {
                node: id,
                closing,
                depth: node.depth,
                text,
                expandable,
                expanded: open,
                kind: node.kind,
            });
            if open {
                pending.push((id, true));
                pending.extend(node.children.iter().rev().map(|&child| (child, false)));
            }
        }
        rows
    }
}

fn shortened(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_scalar_and_field_name_remain_complete_outside_collapsed_previews() {
        let key = "字段".repeat(300);
        let value = "完整内容<&>".repeat(150);
        let source = serde_json::json!({key.clone(): value.clone()}).to_string();
        let document = Document::parse(&source).unwrap();
        let rows = document.rows(&document.initially_expanded(), "", |n| n.to_string());
        assert_eq!(
            rows[1].text,
            format!(
                "{}: {}",
                serde_json::to_string(&key).unwrap(),
                serde_json::to_string(&value).unwrap()
            )
        );
        assert!(!rows[1].expandable);
        assert!(document.preview(0, "1 item").chars().count() < 120);
    }

    #[test]
    fn nested_records_keep_types_inline_arrays_and_identifying_previews() {
        let document = Document::parse(r#"{"code":200,"message":"success","data":{"tags":["vip","new-user"],"address":{"province":"广东省","city":"深圳市","street":"科技路"},"orders":[{"orderId":"A20240101"},{"orderId":"A20240102"}],"empty":null}}"#).unwrap();
        let mut expanded = document.initially_expanded();
        let label = |count| format!("{count} items");
        let rows = document.rows(&expanded, "", label);
        assert!(
            rows.iter().any(|row| row.text.contains(r#"[ "vip", "new-user" ]"#) && !row.expandable)
        );
        let address = rows.iter().find(|row| row.text.starts_with("\"address\"")).unwrap();
        assert!(!address.expanded);
        assert!(
            address.text.contains("广东省")
                && address.text.contains("深圳市")
                && address.text.contains('…')
        );
        let orders = rows.iter().find(|row| row.text.starts_with("\"orders\"")).unwrap();
        assert!(
            orders.text.contains("2 items")
                && orders.text.contains("A20240101")
                && orders.text.contains("A20240102")
        );
        expanded.insert(address.node);
        assert!(document.rows(&expanded, "", label).iter().any(|row| row.text.contains("科技路")));
        let filtered = document.rows(&expanded, "A20240102", label);
        assert!(filtered.iter().any(|row| row.text.contains("A20240102")));
        assert!(!filtered.iter().any(|row| row.text.contains("A20240101")));
        assert!(document.nodes.iter().any(|node| node.kind == Kind::Null && node.value == "null"));
    }
}
