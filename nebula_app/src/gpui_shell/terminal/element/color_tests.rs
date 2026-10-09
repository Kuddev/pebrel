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

#[test]
fn sgr_dim_uses_the_palette_dim_slots() {
    let palette = Palette::default();
    let none = Colors::default();
    let dim = |color| palette.resolve_styled(color, &none, false, true);
    assert_eq!(dim(Color::Named(NamedColor::Foreground)), palette.dim_foreground);
    assert_eq!(dim(Color::Named(NamedColor::Red)), palette.dim[1]);
    assert_eq!(dim(Color::Named(NamedColor::BrightRed)), palette.ansi[1]);
    assert_eq!(dim(Color::Indexed(3)), palette.dim[3]);
    assert_eq!(dim(Color::Indexed(11)), palette.ansi[3]);
    let spec = Rgb { r: 200, g: 100, b: 50 };
    assert_eq!(dim(Color::Spec(spec)), Palette::dim_of(from_ansi_rgb(spec)));
    assert_eq!(
        dim(Color::Indexed(196)),
        Palette::dim_of(palette.resolve(Color::Indexed(196), &none, false))
    );
    // Dim wins over bold's brightening.
    assert_eq!(
        palette.resolve_styled(Color::Named(NamedColor::Foreground), &none, true, true),
        palette.dim_foreground
    );
    assert_eq!(palette.resolve_styled(Color::Indexed(1), &none, true, true), palette.dim[1]);
    for color in [Color::Named(NamedColor::Red), Color::Indexed(3), Color::Spec(spec)] {
        assert_eq!(
            palette.resolve_styled(color, &none, true, false),
            palette.resolve(color, &none, true)
        );
    }

    // An OSC 4 override of the source color is dimmed; an override of the dim
    // slot itself takes precedence.
    let mut overrides = Colors::default();
    let red = Rgb { r: 250, g: 0, b: 0 };
    overrides[NamedColor::Red] = Some(red);
    for color in [Color::Named(NamedColor::Red), Color::Indexed(1)] {
        assert_eq!(
            palette.resolve_styled(color, &overrides, false, true),
            Palette::dim_of(from_ansi_rgb(red))
        );
    }
    let dim_red = Rgb { r: 90, g: 0, b: 0 };
    overrides[NamedColor::DimRed] = Some(dim_red);
    for color in [Color::Named(NamedColor::Red), Color::Indexed(1)] {
        assert_eq!(palette.resolve_styled(color, &overrides, false, true), from_ansi_rgb(dim_red));
    }
}

/// Claude Code and Codex draw reasoning and prompt suggestions with SGR 2 on
/// the default foreground. On Nord the dimmed foreground misses the minimum
/// contrast, so the resolver bakes an adjusted `Spec`; painting must not dim
/// that `Spec` a second time.
#[test]
fn dim_text_stays_distinct_after_contrast_adjustment_without_dimming_twice() {
    let nord = palette(nebula_settings::ThemeName::Nord);
    let nord = Palette { dim_foreground: Palette::dim_of(nord.foreground), ..nord };
    let none = Colors::default();
    let mut term = Term::new(Config::default(), &TermSize::new(40, 2), VoidListener);
    let mut parser: ansi::Processor = ansi::Processor::new();
    parser.advance(&mut term, b"typed \x1b[2mthinking\x1b[0m");
    let mut snapshot = RenderSnapshot::capture(&term, &SnapshotConfig { rows: 2, cols: 40 });
    let mut resolver = TerminalColorResolver::default();
    resolve_app_colors_into(&mut snapshot, &nord, &none, &mut resolver);

    let painted = |col: u16| {
        let cell = snapshot
            .segments
            .iter()
            .flat_map(|segment| segment.cells.iter())
            .find(|cell| cell.col == col)
            .unwrap();
        rgb_from_rgba(nord.resolve_styled(cell.fg, &none, cell.bold, cell.dim))
    };
    let expected = TerminalColorResolver::default().resolve_foreground(
        rgb_from_rgba(nord.dim_foreground),
        rgb_from_rgba(nord.background),
        true,
        rgb_from_rgba(nord.foreground),
        rgb_from_rgba(nord.background),
    );
    let (typed, thinking) = (painted(0), painted(6));
    assert_eq!(thinking, expected);
    assert_eq!(typed, rgb_from_rgba(nord.foreground));
    let sum =
        |rgb: crate::display::color::Rgb| u16::from(rgb.r) + u16::from(rgb.g) + u16::from(rgb.b);
    assert!(sum(thinking) < sum(typed), "dim text must stay darker than typed text");
}
