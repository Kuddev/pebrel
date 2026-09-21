//! 持久化只暴露显示字段；不沿用 Cell 的链接及未来协议字段。

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::DisplaySnapshot;
use crate::term::cell::{Cell, Flags};
use crate::vte::ansi::Color;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisplayCell {
    c: char,
    fg: Color,
    bg: Color,
    flags: Flags,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    combining: Vec<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    underline: Option<Color>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    columns: usize,
    rows: Vec<Vec<DisplayCell>>,
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
        let snapshot = Self {
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
        snapshot.validate().map_err(serde::de::Error::custom)?;
        Ok(snapshot)
    }
}
