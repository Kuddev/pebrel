use super::*;

fn at(ms: u64) -> Duration {
    Duration::from_millis(ms)
}
fn p(col: f64) -> Position {
    Position::new(col, 0.0)
}
fn step(m: &mut CursorMotion, col: f64, ms: u64) -> Position {
    m.update(p(col), at(ms), true, false, 80)
}

#[test]
fn flutter_reference_samples_and_endpoints_match() {
    for (ms, expected) in [
        (0, 0.0),
        (15, 0.436340294),
        (30, 0.722610222),
        (45, 0.875116348),
        (60, 0.954034002),
        (75, 0.990065658),
        (90, 1.0),
    ] {
        assert!((ease_out_cubic(ms as f64 / 90.0) - expected).abs() < 1e-9);
    }
    let mut motion = CursorMotion::default();
    step(&mut motion, 0.0, 0);
    step(&mut motion, 4.0, 0);
    assert!((step(&mut motion, 4.0, 45).col - 3.500465393).abs() < 1e-8);
    assert_eq!(step(&mut motion, 4.0, 90), p(4.0));
    assert!(!motion.active());
}

#[test]
fn continuous_targets_and_repeated_or_regressed_frames_do_not_pull_back() {
    let mut m = CursorMotion::default();
    step(&mut m, 0.0, 0);
    step(&mut m, 4.0, 0);
    let halfway = step(&mut m, 4.0, 45);
    assert_eq!(step(&mut m, 6.0, 45), halfway);
    assert_eq!(step(&mut m, 6.0, 20), halfway);
    assert!(step(&mut m, 6.0, 60).col > halfway.col);
    assert_eq!(step(&mut m, 6.0, 135), p(6.0));
}

#[test]
fn eight_cells_animate_but_euclidean_distance_above_eight_snaps() {
    let mut m = CursorMotion::default();
    step(&mut m, 0.0, 0);
    assert_eq!(step(&mut m, 8.0, 0), p(0.0));
    assert!(m.active());
    step(&mut m, 8.0, 90);
    assert_eq!(step(&mut m, 16.01, 90), p(16.01));
    assert!(!m.active());
    assert_eq!(
        m.update(Position::new(22.01, 6.0), at(90), true, false, 80),
        Position::new(22.01, 6.0)
    );
}

#[test]
fn disable_reset_and_independent_panes_discard_motion() {
    let mut a = CursorMotion::default();
    let mut b = CursorMotion::default();
    step(&mut a, 0.0, 0);
    step(&mut b, 10.0, 0);
    step(&mut a, 4.0, 0);
    assert_eq!(step(&mut b, 10.0, 45), p(10.0));
    assert_eq!(a.update(p(4.0), at(45), false, false, 80), p(4.0));
    assert!(!a.active());
    a.reset();
    assert_eq!(step(&mut a, 5.0, 46), p(5.0));
}

#[test]
fn prompt_reset_is_held_but_explicit_left_gets_one_bounded_permit() {
    let mut m = CursorMotion::default();
    step(&mut m, 7.0, 0);
    m.note_input(b"x", at(1));
    assert_eq!(m.update(p(0.0), at(2), true, true, 80), p(7.0));
    assert_eq!(m.update(p(8.0), at(30), true, true, 80), p(7.0));
    m.update(p(8.0), at(120), true, true, 80);
    m.note_input(b"\x1b[D", at(121));
    assert_eq!(m.update(p(0.0), at(122), true, true, 80), p(8.0));
    assert_eq!(m.update(p(7.0), at(130), true, true, 80), p(8.0));
    assert_eq!(m.update(p(7.0), at(220), true, true, 80), p(7.0));
    // That authorization cannot grant a second retreat.
    assert_eq!(m.update(p(6.0), at(221), true, true, 80), p(7.0));
    for ms in [240, 260, 311] {
        m.update(p(6.0), at(ms), true, true, 80);
    }
    assert_eq!(m.visual, p(6.0));
    assert!(!m.active());
}

#[test]
fn changing_retreat_candidate_restarts_both_time_and_frame_confirmation() {
    let mut m = CursorMotion::default();
    step(&mut m, 8.0, 0);
    for ms in [1, 30, 60] {
        m.update(p(0.0), at(ms), true, true, 80);
    }
    assert_eq!(m.update(p(1.0), at(91), true, true, 80), p(8.0));
    for ms in [100, 110, 120] {
        assert_eq!(m.update(p(1.0), at(ms), true, true, 80), p(8.0));
    }
    assert_eq!(m.update(p(1.0), at(181), true, true, 80), p(1.0));
    assert!(!m.active());
}

#[test]
fn expired_input_and_alternate_screen_do_not_authorize_prompt_heuristics() {
    let mut m = CursorMotion::default();
    step(&mut m, 8.0, 0);
    m.note_input(b"\x7f", at(1));
    assert_eq!(m.update(p(7.0), at(300), true, true, 80), p(8.0));
    // TUI positioning is allowed immediately when prompt guarding is off.
    assert_eq!(m.update(p(6.0), at(301), true, false, 80), p(8.0));
    assert_eq!(m.update(p(6.0), at(391), true, false, 80), p(6.0));
}

#[test]
fn forward_editing_keys_hold_prompt_redraws_until_the_input_guard_expires() {
    for input in [&b"x"[..], b"\x1b[C", b"\x1b[F", b"\x1b[3~"] {
        let mut m = CursorMotion::default();
        step(&mut m, 8.0, 0);
        m.note_input(input, at(1));
        for ms in [2, 35, 70, 100, 200, 250] {
            assert_eq!(m.update(p(0.0), at(ms), true, true, 80), p(8.0));
        }
        assert_eq!(m.update(p(0.0), at(251), true, true, 80), p(0.0));
        assert!(!m.active());
    }
}
