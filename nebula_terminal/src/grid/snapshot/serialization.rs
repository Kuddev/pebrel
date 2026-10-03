//! 持久化只暴露显示字段；不沿用 Cell 的链接及未来协议字段。

use serde::de::{DeserializeSeed, Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{DisplaySnapshot, MAX_SNAPSHOT_CELLS};
use crate::term::cell::{Cell, Flags};
use crate::vte::ansi::Color;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayCell {
    c: char,
    fg: Color,
    bg: Color,
    flags: Flags,
    #[serde(default, deserialize_with = "decode_combining", skip_serializing_if = "Vec::is_empty")]
    combining: Vec<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    underline: Option<Color>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    columns: usize,
    #[serde(deserialize_with = "decode_rows")]
    rows: Vec<Vec<DisplayCell>>,
}

// 解码中计数：字节上限挡不住 `[]` 空行等在内存中的放大。
fn over<E: Error>() -> E {
    E::custom("display snapshot exceeds decode budget")
}

fn decode_combining<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<char>, D::Error> {
    struct Chars;
    impl<'de> Visitor<'de> for Chars {
        type Value = Vec<char>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("combining characters")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<char>, A::Error> {
            let mut chars = Vec::new();
            while let Some(c) = seq.next_element()? {
                if chars.len() == MAX_SNAPSHOT_CELLS {
                    return Err(over());
                }
                chars.push(c);
            }
            Ok(chars)
        }
    }
    deserializer.deserialize_seq(Chars)
}

struct Budget {
    cells: usize,
    combining: usize,
}

impl<'de> DeserializeSeed<'de> for &mut Budget {
    type Value = Vec<DisplayCell>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for &mut Budget {
    type Value = Vec<DisplayCell>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("display row")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut row = Vec::new();
        while let Some(cell) = seq.next_element::<DisplayCell>()? {
            self.cells = self.cells.checked_sub(1).ok_or_else(over)?;
            self.combining = self.combining.checked_sub(cell.combining.len()).ok_or_else(over)?;
            row.push(cell);
        }
        Ok(row)
    }
}

fn decode_rows<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Vec<DisplayCell>>, D::Error> {
    struct Rows;
    impl<'de> Visitor<'de> for Rows {
        type Value = Vec<Vec<DisplayCell>>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("display rows")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut budget = Budget { cells: MAX_SNAPSHOT_CELLS, combining: MAX_SNAPSHOT_CELLS };
            let mut rows = Vec::new();
            while let Some(row) = seq.next_element_seed(&mut budget)? {
                if rows.len() == MAX_SNAPSHOT_CELLS {
                    return Err(over());
                }
                rows.push(row);
            }
            Ok(rows)
        }
    }
    deserializer.deserialize_seq(Rows)
}

impl Serialize for DisplaySnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WireSnapshot {
            columns: self.columns,
            rows: self
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| DisplayCell {
                            c: cell.c,
                            fg: cell.fg,
                            bg: cell.bg,
                            flags: cell.flags,
                            combining: cell.zerowidth().unwrap_or_default().to_vec(),
                            underline: cell.underline_color(),
                        })
                        .collect()
                })
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DisplaySnapshot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WireSnapshot::deserialize(deserializer)?;
        let mut snapshot = Self {
            columns: wire.columns,
            rows: wire
                .rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|value| {
                            let mut cell = Cell {
                                c: value.c,
                                fg: value.fg,
                                bg: value.bg,
                                flags: value.flags,
                                extra: None,
                            };
                            for character in value.combining {
                                cell.push_zerowidth(character);
                            }
                            cell.set_underline_color(value.underline);
                            cell
                        })
                        .collect()
                })
                .collect(),
        };
        snapshot.sanitize_wide_flags();
        snapshot.validate().map_err(Error::custom)?;
        Ok(snapshot)
    }
}
