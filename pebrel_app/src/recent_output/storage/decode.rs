//! Enforce archive cardinality while visiting the on-disk map and record arrays.
use std::collections::BTreeMap;
use std::fmt;

use serde::de::{Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::{Archive, CommandRecord, MAX_PANES, valid_pane_id};
use crate::recent_output::{MAX_RECORDS, RecentOutput};

type Panes = BTreeMap<String, Vec<CommandRecord>>;

pub(super) fn panes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Panes, D::Error> {
    struct PaneMap;

    impl<'de> Visitor<'de> for PaneMap {
        type Value = Panes;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a map of pane IDs to recent command records")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Panes, A::Error> {
            let mut panes = Panes::new();
            while let Some(id) = map.next_key::<String>()? {
                if panes.len() == MAX_PANES {
                    return Err(A::Error::custom("too many panes in output archive"));
                }
                if !valid_pane_id(&id) || panes.contains_key(&id) {
                    return Err(A::Error::custom("invalid or duplicate pane reference"));
                }
                let records: Records = map.next_value()?;
                panes.insert(id, records.0);
            }
            Ok(panes)
        }
    }
    deserializer.deserialize_map(PaneMap)
}

#[derive(Deserialize)]
struct Records(#[serde(deserialize_with = "records")] Vec<CommandRecord>);

fn records<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<CommandRecord>, D::Error> {
    struct RecordList;

    impl<'de> Visitor<'de> for RecordList {
        type Value = Vec<CommandRecord>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a bounded list of recent command records")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            while records.len() < MAX_RECORDS {
                let Some(record) = seq.next_element()? else { return Ok(records) };
                RecentOutput::validate_records(std::slice::from_ref(&record))
                    .map_err(A::Error::custom)?;
                records.push(record);
            }
            if seq.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::custom("too many command records"));
            }
            Ok(records)
        }
    }
    deserializer.deserialize_seq(RecordList)
}

pub(super) fn previous<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Box<Archive>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Previous {
        version: u32,
        #[serde(deserialize_with = "panes")]
        panes: Panes,
        // Older JSON can spell the absent generation as an explicit null.
        #[serde(default, rename = "previous")]
        _end: (),
    }

    Ok(Option::<Previous>::deserialize(deserializer)?.map(|generation| {
        Box::new(Archive { version: generation.version, panes: generation.panes, previous: None })
    }))
}
