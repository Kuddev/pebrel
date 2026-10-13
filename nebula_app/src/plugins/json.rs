//! Lua 与宿主间的数据必须同时限制深度、节点和文本；仅限制 VM 内存会漏掉展开复制。

use mlua::{LuaSerdeExt, Value as LuaValue};
use serde_json::{Map, Number, Value};

use super::{Error, MAX_JSON_BYTES, MAX_JSON_DEPTH, MAX_JSON_NODES, Result};

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
    slots: usize,
}

impl Budget {
    fn reserve_slots(&mut self, count: usize) -> Result<()> {
        self.slots = self.slots.saturating_add(count);
        if self.slots > MAX_JSON_NODES {
            return Err(Error::new(
                "payload_limit",
                "payload container allocation budget exhausted",
            ));
        }
        Ok(())
    }

    fn visit(&mut self, depth: usize, bytes: usize) -> Result<()> {
        self.nodes += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if depth > MAX_JSON_DEPTH || self.nodes > MAX_JSON_NODES || self.bytes > MAX_JSON_BYTES {
            return Err(Error::new(
                "payload_limit",
                "payload exceeds its depth, node, or text budget",
            ));
        }
        Ok(())
    }
}

pub(super) fn validate(value: &Value) -> Result<()> {
    fn walk(value: &Value, depth: usize, budget: &mut Budget) -> Result<()> {
        budget.visit(depth, 0)?;
        match value {
            Value::String(value) => budget.visit(depth, value.len())?,
            Value::Array(values) => {
                for value in values {
                    walk(value, depth + 1, budget)?;
                }
            },
            Value::Object(values) => {
                for (key, value) in values {
                    budget.visit(depth, key.len())?;
                    walk(value, depth + 1, budget)?;
                }
            },
            _ => {},
        }
        Ok(())
    }
    walk(value, 0, &mut Budget::default())
}

pub(super) fn from_lua(lua: &mlua::Lua, value: LuaValue) -> Result<Value> {
    fn walk(
        value: LuaValue,
        depth: usize,
        budget: &mut Budget,
        array_marker: *const std::ffi::c_void,
    ) -> Result<Value> {
        budget.visit(depth, 0)?;
        Ok(match value {
            LuaValue::Nil => Value::Null,
            LuaValue::LightUserData(value) if value.0.is_null() => Value::Null,
            LuaValue::Boolean(value) => Value::Bool(value),
            LuaValue::Integer(value) => Value::Number(value.into()),
            LuaValue::Number(value) => Value::Number(Number::from_f64(value).ok_or_else(|| {
                Error::new("invalid_payload", "non-finite numbers are not JSON values")
            })?),
            LuaValue::String(value) => {
                let text = value.to_str().map_err(|error| Error::new("invalid_utf8", error))?;
                budget.visit(depth, text.len())?;
                Value::String(text.to_string())
            },
            LuaValue::Table(table) => {
                let length = table.raw_len();
                if length > MAX_JSON_NODES {
                    return Err(Error::new(
                        "payload_limit",
                        "array length exceeds the node budget",
                    ));
                }
                let array = length > 0
                    || table.metatable().is_some_and(|value| value.to_pointer() == array_marker);
                if array {
                    // 先预留整棵输出树的槽位预算，再分配，避免深层数组逐层按上限扩容。
                    budget.reserve_slots(length)?;
                }
                let mut object = Map::new();
                let mut values = if array { vec![Value::Null; length] } else { Vec::new() };
                let mut count = 0;
                for pair in table.pairs::<LuaValue, LuaValue>() {
                    let (key, value) =
                        pair.map_err(|error| Error::new("invalid_payload", error))?;
                    count += 1;
                    match (array, key) {
                        (true, LuaValue::Integer(index))
                            if index >= 1 && (index as usize) <= length =>
                        {
                            values[index as usize - 1] =
                                walk(value, depth + 1, budget, array_marker)?;
                        },
                        (false, LuaValue::String(key)) => {
                            budget.reserve_slots(1)?;
                            let key =
                                key.to_str().map_err(|error| Error::new("invalid_utf8", error))?;
                            budget.visit(depth, key.len())?;
                            let value = walk(value, depth + 1, budget, array_marker)?;
                            object.insert(key.to_string(), value);
                        },
                        _ => {
                            return Err(Error::new(
                                "invalid_payload",
                                "tables must be string-keyed objects or dense arrays",
                            ));
                        },
                    }
                }
                if array && count != length {
                    return Err(Error::new("invalid_payload", "sparse arrays are not supported"));
                }
                if array { Value::Array(values) } else { Value::Object(object) }
            },
            _ => {
                return Err(Error::new(
                    "invalid_payload",
                    "functions, threads, and userdata are not JSON values",
                ));
            },
        })
    }
    walk(value, 0, &mut Budget::default(), lua.array_metatable().to_pointer())
}
