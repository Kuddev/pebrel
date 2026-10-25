//! Qualification counters. A paint call is not a native presentation/fence.
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct Histogram {
    count: u64,
    sum_us: u64,
    max_us: u64,
    bins: Vec<u64>,
    bin_width_us: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self { count: 0, sum_us: 0, max_us: 0, bins: vec![0; 10_002], bin_width_us: 1 }
    }
}

impl Histogram {
    pub fn with_bin_width(bin_width_us: u64) -> Self {
        assert!(bin_width_us > 0);
        Self { bin_width_us, ..Default::default() }
    }
    pub fn record(&mut self, elapsed: Duration) {
        let us = elapsed.as_micros().min(u64::MAX as u128) as u64;
        self.count += 1;
        self.sum_us = self.sum_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        self.bins[us.div_ceil(self.bin_width_us).min(10_001) as usize] += 1;
    }

    pub fn percentile_upper_us(&self, percentile: u64) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let target = (self.count * percentile).div_ceil(100);
        let mut cumulative = 0;
        for (index, count) in self.bins.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return Some(if index == 10_001 {
                    self.max_us
                } else {
                    index as u64 * self.bin_width_us
                });
            }
        }
        None
    }
}

#[derive(Default, Serialize)]
pub struct Metrics {
    pub generated: u64,
    pub stale_completions: u64,
    pub timer_requests: u64,
    pub timer_fires: u64,
    pub paint_calls: u64,
    pub failed_paints: u64,
    pub retirement_requests: u64,
    pub bounds_events: u64,
    pub scale_changes: u64,
    pub activation_events: u64,
    pub pause_events: u64,
    pub resume_events: u64,
    pub keys: u64,
    pub peak_generated_capacity: usize,
    pub peak_cursor_capacity: usize,
    pub decoded_outputs: u64,
    pub gif_loops: u64,
    pub source_changes: u64,
    pub revoked_presentations: u64,
    pub decoder_error: Option<String>,
    pub producer_started: u64,
    pub producer_completed: u64,
    pub producers_live: u64,
    pub producers_peak: u64,
    pub gpu_waiters_live: u64,
    pub gpu_waiters_peak: u64,
    pub stream_completion_errors: u64,
    pub shader_prepare_started: u64,
    pub shader_prepare_completed: u64,
    pub shader_prepare_live: u64,
    pub shader_prepare_peak: u64,
    pub shader_prepare_adopted: u64,
    pub shader_prepare_stale: u64,
    pub shader_prepare_cancelled: u64,
    pub shader_prepare_errors: u64,
    pub shader_prepare_cpu: Histogram,
    pub missed_deadlines: u64,
    pub transitions: Vec<Transition>,
    pub transition_overflow: u64,
    pub prepare: Histogram,
    pub paint_image_cpu: Histogram,
    pub canvas_cpu: Histogram,
    pub input_handler_to_paint: Histogram,
}

#[derive(Serialize)]
pub struct Transition {
    pub playing: bool,
    pub wall_ms: u64,
    pub media_ms: u64,
    pub generated: u64,
    pub timer_requests: u64,
    pub timer_fires: u64,
    pub generation: u64,
}

impl Metrics {
    pub fn transition(&mut self, playing: bool, wall: Duration, media: Duration, generation: u64) {
        if self.transitions.len() == 256 {
            self.transition_overflow += 1;
            return;
        }
        self.transitions.push(Transition {
            playing,
            wall_ms: wall.as_millis().min(u128::from(u64::MAX)) as u64,
            media_ms: media.as_millis().min(u128::from(u64::MAX)) as u64,
            generated: self.generated,
            timer_requests: self.timer_requests,
            timer_fires: self.timer_fires,
            generation,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles_include_overflow_without_hiding_a_slow_tail() {
        let mut h = Histogram::default();
        assert_eq!(h.percentile_upper_us(95), None);
        for _ in 0..94 {
            h.record(Duration::from_micros(10));
        }
        for _ in 0..6 {
            h.record(Duration::from_micros(30_000));
        }
        assert_eq!(h.percentile_upper_us(95), Some(30_000));
        assert_eq!(h.percentile_upper_us(50), Some(10));
    }
    #[test]
    fn wider_bins_bound_native_decode_tail_without_changing_storage() {
        let mut h = Histogram::with_bin_width(10);
        h.record(Duration::from_micros(33333));
        assert_eq!(h.percentile_upper_us(95), Some(33340));
        h.record(Duration::from_micros(200000));
        assert_eq!(h.percentile_upper_us(95), Some(200000));
        assert_eq!(h.bins.len(), 10002);
    }
}
