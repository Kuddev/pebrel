//! Replay the terminal features required by Codex message surfaces and stars.
//! Fixture colors follow codex rust-v0.154.0 tui/src/style.rs and
//! bottom_pane/chat_composer/sparkle.rs; this does not emulate a live Codex session.

use nebula_terminal::event::VoidListener;
use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
use nebula_terminal::term::color::Colors;
use nebula_terminal::term::test::TermSize;
use nebula_terminal::term::{Config, Term};
use nebula_terminal::vte::ansi::{self, Color, NamedColor, Rgb};

use super::{Palette, resolve_app_colors_into, rgb_from_rgba};
use crate::display::terminal_color::TerminalColorResolver;
use crate::gpui_shell::terminal::colors::from_ansi_rgb;
use crate::gpui_shell::theme::ResolvedTheme;

fn palette(name: nebula_settings::ThemeName) -> Palette {
    let resolved = ResolvedTheme::builtin(name, None);
    let color = |[r, g, b]: [u8; 3]| from_ansi_rgb(Rgb { r, g, b });
    Palette {
        background: color(resolved.terminal_background()),
        foreground: color(resolved.terminal_foreground()),
        ..Palette::default()
    }
}

fn capture(palette: &Palette) -> (RenderSnapshot, Rgb) {
    let bg = palette.query_reply(NamedColor::Background as usize, &Colors::default());
    let fg = palette.query_reply(NamedColor::Foreground as usize, &Colors::default());
    let (top, alpha) = if palette.is_dark() { (255.0, 0.12) } else { (0.0, 0.04) };
    let blend = |channel: u8| (channel as f32 * (1.0 - alpha) + top * alpha).round() as u8;
    let user = Rgb { r: blend(bg.r), g: blend(bg.g), b: blend(bg.b) };
    let bytes = format!(
        "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m用户 message\x1b[K\r\n⠁⠂⠄⠈⠐⠠⡀⢀\x1b[K\r\n\x1b[0mAI reply",
        user.r, user.g, user.b, fg.r, fg.g, fg.b,
    );
    let mut term = Term::new(Config::default(), &TermSize::new(40, 4), VoidListener);
    let mut parser: ansi::Processor = ansi::Processor::new();
    parser.advance(&mut term, bytes.as_bytes());
    (RenderSnapshot::capture(&term, &SnapshotConfig { rows: 4, cols: 40 }), user)
}

#[test]
fn every_builtin_theme_keeps_message_surfaces_and_braille_stars() {
    for name in nebula_settings::ThemeName::BUILTIN {
        let palette = palette(name);
        let (mut snapshot, user) = capture(&palette);
        assert_ne!(from_ansi_rgb(user), palette.background, "{name:?}");
        let star_colors: Vec<_> = snapshot
            .segments
            .iter()
            .filter(|seg| seg.row == 1)
            .flat_map(|seg| seg.cells.iter().map(|cell| cell.fg))
            .collect();
        let mut resolver = TerminalColorResolver::default();
        resolve_app_colors_into(&mut snapshot, &palette, &Colors::default(), &mut resolver);
        for row in [0, 1] {
            let run = snapshot.bg_runs.iter().find(|run| run.row == row).unwrap();
            assert_eq!((run.start, run.end, run.color), (0, 40, Color::Spec(user)), "{name:?}");
        }
        assert!(
            snapshot.bg_runs.iter().all(|run| run.row != 2),
            "{name:?}: AI reply acquired a message background"
        );
        let stars: String = snapshot
            .segments
            .iter()
            .filter(|seg| seg.row == 1)
            .flat_map(|seg| seg.cells.iter().map(|cell| cell.text.as_str()))
            .collect();
        assert_eq!(stars, "⠁⠂⠄⠈⠐⠠⡀⢀");
        let after: Vec<_> = snapshot
            .segments
            .iter()
            .filter(|seg| seg.row == 1)
            .flat_map(|seg| seg.cells.iter().map(|cell| cell.fg))
            .collect();
        assert_eq!(star_colors, after, "{name:?}: star colors were changed");
    }
}

#[test]
fn message_background_remains_distinct_after_every_builtin_theme_switch() {
    for from in nebula_settings::ThemeName::BUILTIN {
        for to in nebula_settings::ThemeName::BUILTIN {
            let original = palette(from);
            let next = palette(to);
            let (mut snapshot, _) = capture(&original);
            let mut resolver = TerminalColorResolver::default();
            resolver
                .theme_changed(rgb_from_rgba(original.background), rgb_from_rgba(next.background));
            resolve_app_colors_into(&mut snapshot, &next, &Colors::default(), &mut resolver);
            let user = snapshot.bg_runs.iter().find(|run| run.row == 0).unwrap();
            let background = next.resolve(user.color, &Colors::default(), false);
            assert_ne!(background, next.background, "{from:?} -> {to:?}");
            assert!(snapshot.bg_runs.iter().all(|run| run.row != 2));
        }
    }
}
