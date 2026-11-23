//! Shared numeric contracts for background and terminal shader playback.

use crate::RawSettings;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderTiming {
    pub speed: f32,
    pub frame_rate: u16,
}

impl Default for ShaderTiming {
    fn default() -> Self {
        Self { speed: 1.0, frame_rate: 20 }
    }
}

impl ShaderTiming {
    pub fn validate(self) -> Result<(), &'static str> {
        if !self.speed.is_finite() || !(0.0..=10.0).contains(&self.speed) {
            return Err("shader speed must be between 0 and 10");
        }
        if !(1..=60).contains(&self.frame_rate) {
            return Err("shader frame_rate must be between 1 and 60");
        }
        Ok(())
    }

    pub fn from_raw(raw: &RawSettings, prefix: &str) -> Self {
        let defaults = Self::default();
        let speed = raw
            .f32(&format!("{prefix}_speed"))
            .filter(|value| Self { speed: *value, ..defaults }.validate().is_ok())
            .unwrap_or(defaults.speed);
        let frame_rate = raw
            .value(&format!("{prefix}_frame_rate"))
            .and_then(|value| value.parse().ok())
            .filter(|value| Self { frame_rate: *value, ..defaults }.validate().is_ok())
            .unwrap_or(defaults.frame_rate);
        Self { speed, frame_rate }
    }

    pub fn interval(self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.frame_rate.clamp(1, 60)))
    }
}

pub(crate) fn parameter(raw: &RawSettings, key: &str) -> f32 {
    raw.f32(key).filter(|value| value.is_finite()).unwrap_or(0.0)
}
