use super::super::startup_tests::{feed, open};
use super::*;
use gpui::TestAppContext;
use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};

fn capture(view: &TerminalView) -> RenderSnapshot {
    RenderSnapshot::capture(
        &*view.session.as_ref().unwrap().term.lock(),
        &SnapshotConfig { rows: 24, cols: 80 },
    )
}

fn geometry(shape: CursorShape) -> CursorGeometry {
    CursorGeometry {
        screen: (1, false, 0),
        grid: (80, 24),
        metrics: (9.0, 20.0, 1.0),
        projection_shift: 0,
        shape,
    }
}

#[gpui::test]
fn cursor_motion_hot_applies_without_replacing_the_session_or_logical_cursor(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            let term = view.session.as_ref().unwrap().term.clone();
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            view.apply_settings(cx);
            let before = capture(view);
            let shape = before.cursor.as_ref().unwrap().shape;
            let g = geometry(shape);
            assert_eq!(
                view.visual_cursor(before.cursor.as_ref(), 0, g, true, window, cx),
                Some(Position::new(0.0, 0.0))
            );
            feed(view, b"abcd");
            let snap = capture(view);
            let logical = snap.cursor.as_ref().unwrap().col;
            assert_eq!(logical, 4);
            assert_eq!(
                view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx),
                Some(Position::new(0.0, 0.0))
            );
            assert!(view.cursor_animation.motion.active());
            assert_eq!(capture(view).cursor.unwrap().col, 4);
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Off;
            view.apply_settings(cx);
            assert_eq!(
                view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx),
                Some(Position::new(4.0, 0.0))
            );
            assert!(!view.cursor_animation.motion.active());
            assert!(Arc::ptr_eq(&term, &view.session.as_ref().unwrap().term));
        })
    });
}

#[gpui::test]
fn cursor_motion_resets_for_long_hide_scrolled_reduced_motion_and_changed_geometry(
    cx: &mut TestAppContext,
) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            let mut snap = capture(view);
            let mut g = geometry(snap.cursor.as_ref().unwrap().shape);
            let cursor = snap.cursor.as_mut().unwrap();
            for reason in 0..7 {
                view.cursor_animation.reset();
                cx.set_reduce_motion(false);
                view.set_output_visible(true, cx);
                view.visual_cursor(Some(cursor), 0, g, true, window, cx);
                view.visual_cursor(Some(cursor), 4, g, true, window, cx);
                assert!(view.cursor_animation.motion.active());
                match reason {
                    0 => {
                        g.screen.1 = !g.screen.1;
                    },
                    1 => {
                        g.metrics.2 = 1.5;
                    },
                    2 => {
                        g.grid.0 = 81;
                    },
                    3 => {
                        cx.set_reduce_motion(true);
                    },
                    4 => {
                        view.set_output_visible(false, cx);
                    },
                    5 => {},
                    _ => {
                        view.visual_cursor(None, 4, g, true, window, cx);
                        view.cursor_animation.clock -= REPAINT_HIDE_GRACE;
                    },
                }
                assert_eq!(
                    view.visual_cursor(Some(cursor), 4, g, reason != 5, window, cx),
                    Some(Position::new(4.0, 0.0))
                );
                assert!(!view.cursor_animation.motion.active());
            }
        })
    });
}

#[gpui::test]
fn psreadline_hidden_repaint_preserves_typing_and_backspace_motion(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            feed(view, b"QA> ");
            let snap = capture(view);
            let g = geometry(snap.cursor.as_ref().unwrap().shape);
            view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx);
            for (input, echo, from, to) in [
                (b"a".as_slice(), b"a".as_slice(), 4, 5),
                (b"b".as_slice(), b"\x1b[1;5Hab".as_slice(), 5, 6),
                (b"\x7f".as_slice(), b"\x1b[1;6H".as_slice(), 6, 5),
            ] {
                view.cursor_animation.note_input(input);
                feed(view, b"\x1b[?25l");
                let hidden = capture(view);
                let hidden_g = CursorGeometry { shape: CursorShape::Hidden, ..g };
                assert_eq!(
                    view.visual_cursor(hidden.cursor.as_ref(), 0, hidden_g, true, window, cx),
                    None
                );
                assert_eq!(view.cursor_animation.motion.visual.col, from as f64);
                // Repaint may include backward positions; never animate those.
                feed(view, echo);
                let hidden = capture(view);
                assert_eq!(
                    view.visual_cursor(hidden.cursor.as_ref(), 0, hidden_g, true, window, cx),
                    None
                );
                feed(view, b"\x1b[?25h");
                let shown = capture(view);
                assert_eq!(usize::from(shown.cursor.as_ref().unwrap().col), to);
                assert_eq!(
                    view.visual_cursor(shown.cursor.as_ref(), to, g, true, window, cx),
                    Some(Position::new(from as f64, 0.0))
                );
                view.cursor_animation.clock -= Duration::from_millis(45);
                let halfway =
                    view.visual_cursor(shown.cursor.as_ref(), to, g, true, window, cx).unwrap();
                assert!(halfway.col > from.min(to) as f64 && halfway.col < from.max(to) as f64);
                view.cursor_animation.clock -= Duration::from_millis(45);
                assert_eq!(
                    view.visual_cursor(shown.cursor.as_ref(), to, g, true, window, cx),
                    Some(Position::new(to as f64, 0.0))
                );
            }
        });
    });
}

#[gpui::test]
fn hidden_cursor_stops_frame_demand_and_invalidates_changed_surface(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            let snap = capture(view);
            let g = geometry(snap.cursor.as_ref().unwrap().shape);
            view.visual_cursor(snap.cursor.as_ref(), 0, g, true, window, cx);
            view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx);
            view.visual_cursor(None, 0, g, true, window, cx);
            assert!(view.cursor_animation.hidden_since.is_some());
        });
        window.simulate_next_frame(cx);
    });
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.cursor_animation.frame_pending);
            let snap = capture(view);
            let mut g = geometry(snap.cursor.as_ref().unwrap().shape);
            g.screen.1 = true;
            view.visual_cursor(None, 0, g, true, window, cx);
            assert!(view.cursor_animation.geometry.is_none());
            assert!(!view.cursor_animation.motion.active());
            g.screen.1 = false;
            assert_eq!(
                view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx),
                Some(Position::new(4.0, 0.0))
            );
        });
    });
}

#[gpui::test]
fn blink_phase_does_not_reset_motion_and_frame_requests_are_single_flight(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            let snap = capture(view);
            let g = geometry(snap.cursor.as_ref().unwrap().shape);
            view.visual_cursor(snap.cursor.as_ref(), 0, g, true, window, cx);
            view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx);
            view.cursor_visible = false;
            view.visual_cursor(snap.cursor.as_ref(), 4, g, true, window, cx);
            assert!(view.cursor_animation.motion.active());
            assert!(view.cursor_animation.frame_pending);
            view.set_output_visible(false, cx);
        })
    });
    window.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    view.read_with(window, |view, _| {
        assert!(!view.cursor_animation.frame_pending);
        assert!(!view.cursor_animation.motion.active());
    });
}

#[gpui::test]
fn negotiated_key_protocols_keep_one_bounded_left_permit(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            cx.global_mut::<Settings>().cursor_motion = nebula_settings::CursorMotion::Smooth;
            let snapshot = capture(view);
            let g = geometry(snapshot.cursor.as_ref().unwrap().shape);
            let key = gpui::Keystroke::parse("left").unwrap();
            for mode in
                [TermMode::empty(), TermMode::WIN32_INPUT_MODE, TermMode::DISAMBIGUATE_ESC_CODES]
            {
                view.cursor_animation.reset();
                view.visual_cursor(snapshot.cursor.as_ref(), 8, g, true, window, cx);
                let bytes = keymap::encode(&key, &mode).unwrap();
                view.write_user_key(key.clone(), bytes, cx);
                view.visual_cursor(snapshot.cursor.as_ref(), 7, g, true, window, cx);
                assert!(view.cursor_animation.motion.active());
                view.cursor_animation.clock -= std::time::Duration::from_millis(90);
                view.visual_cursor(snapshot.cursor.as_ref(), 7, g, true, window, cx);
                // The next retreat is unrequested, even when the first key was
                // encoded differently. It must not consume a duplicate permit.
                view.visual_cursor(snapshot.cursor.as_ref(), 6, g, true, window, cx);
                view.cursor_animation.clock -= std::time::Duration::from_millis(45);
                assert_eq!(
                    view.visual_cursor(snapshot.cursor.as_ref(), 6, g, true, window, cx),
                    Some(Position::new(7.0, 0.0))
                );
            }
        });
    });
}
