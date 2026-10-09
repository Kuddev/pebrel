//! Shared timing for the confirmed input-focus and theme-reveal transitions.

/// CSS cubic-bezier(0.85, 0, 0.15, 1), including its time-axis inversion.
pub(super) fn ease(delta: f32) -> f32 {
    let delta = delta.clamp(0.0, 1.0);
    if delta == 0.0 || delta == 1.0 {
        return delta;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        let u = 1.0 - t;
        let x = 3.0 * 0.85 * u * u * t + 3.0 * 0.15 * u * t * t + t * t * t;
        if x < delta {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::ease;

    #[test]
    fn easing_matches_the_reference_time_axis_and_preserves_endpoints() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.3953125) - 0.15625).abs() < 0.0001);
        assert!((ease(0.6046875) - 0.84375).abs() < 0.0001);
        let samples = (0..=100).map(|step| ease(step as f32 / 100.0)).collect::<Vec<_>>();
        assert!(samples.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
