//! Per-link, single-pane screen baseline. Only acknowledged baselines receive
//! row deltas; switching panes, reconnecting or missing a baseline sends a full
//! snapshot. No terminal bytes or user input are replayed by this adapter.
use serde_json::{Value, json};

#[derive(Clone, Default)]
pub(super) struct ScreenBaseline {
    target: Option<(u64, u64)>,
    screen: Option<Value>,
    sequence: u64,
}

impl ScreenBaseline {
    pub(super) fn encode(&mut self, result: &mut Value, acknowledged: u64) {
        let Some(target) = result["window_id"].as_u64().zip(result["pane_id"].as_u64()) else {
            return;
        };
        let Some(screen) = result.get("screen").filter(|value| value.is_object()).cloned() else {
            return;
        };
        let compatible = self.target == Some(target)
            && acknowledged == self.sequence
            && self.screen.as_ref().is_some_and(|previous| {
                previous["version"] == screen["version"]
                    && previous["columns"] == screen["columns"]
                    && previous["rows"].as_array().map(Vec::len)
                        == screen["rows"].as_array().map(Vec::len)
            });
        let base = self.sequence;
        if self.target != Some(target) || self.screen.as_ref() != Some(&screen) {
            self.sequence += 1;
        }
        result["screen_seq"] = json!(self.sequence);
        if compatible {
            let previous = self.screen.as_ref().unwrap();
            let rows: Vec<_> = screen["rows"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(index, row)| previous["rows"][*index] != **row)
                .map(|(index, row)| json!([index, row]))
                .collect();
            let delta = json!({"base":base,"rows":rows,"cursor":screen["cursor"],"palette":screen["palette"]});
            // During a full redraw the normal snapshot can be smaller than indexed rows.
            if serde_json::to_vec(&delta).unwrap().len()
                < serde_json::to_vec(&screen).unwrap().len()
            {
                result.as_object_mut().unwrap().remove("screen");
                result["screen_delta"] = delta;
            }
        }
        self.target = Some(target);
        self.screen = Some(screen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(pane: u64) -> Value {
        json!({"window_id":1,"pane_id":pane,"screen":{"version":1,"columns":120,
            "rows":vec![vec![json!([" ",1,-257,-258,0]);120];100],"cursor":[0,99,1],"palette":[]}})
    }

    #[test]
    fn mobile_latency_one_changed_row_does_not_resend_one_hundred_rows() {
        let mut cache = ScreenBaseline::default();
        let mut first = frame(2);
        cache.encode(&mut first, 0);
        assert!(first.get("screen").is_some());
        let mut changed = frame(2);
        changed["screen"]["rows"][99][0][0] = json!("a");
        let full_size = serde_json::to_vec(&changed).unwrap().len();
        cache.encode(&mut changed, first["screen_seq"].as_u64().unwrap());
        assert!(changed.get("screen").is_none());
        assert_eq!(changed["screen_delta"]["rows"].as_array().unwrap().len(), 1);
        assert!(serde_json::to_vec(&changed).unwrap().len() < full_size / 50);
        let mut missing = frame(2);
        cache.encode(&mut missing, 0);
        assert!(missing.get("screen").is_some());
        let mut switched = frame(3);
        cache.encode(&mut switched, missing["screen_seq"].as_u64().unwrap());
        assert!(switched.get("screen").is_some());
    }

    #[test]
    fn mobile_latency_unchanged_screen_keeps_revision_and_resize_requires_snapshot() {
        let mut cache = ScreenBaseline::default();
        let mut first = frame(2);
        cache.encode(&mut first, 0);
        let sequence = first["screen_seq"].as_u64().unwrap();
        let mut same = frame(2);
        cache.encode(&mut same, sequence);
        assert_eq!(same["screen_seq"], sequence);
        assert!(same["screen_delta"]["rows"].as_array().unwrap().is_empty());
        let mut resized = frame(2);
        resized["screen"]["rows"].as_array_mut().unwrap().pop();
        cache.encode(&mut resized, sequence);
        assert!(resized.get("screen").is_some());
    }
}
