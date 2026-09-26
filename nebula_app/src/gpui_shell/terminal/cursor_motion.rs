//! Visual cursor trajectory; never writes terminal coordinates.
//!
//! Behavioral reference: silkmux CursorMotionTracker / projection retreat guard.
//! Match Flutter Curves.easeOutCubic, including its 0.001 Bezier x tolerance.

use std::collections::VecDeque;
use std::time::Duration;

pub(super) const DURATION: Duration = Duration::from_millis(90);
const INPUT_LIFETIME: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Position {
    pub col: f64,
    pub row: f64,
}

impl Position {
    pub fn new(col: f64, row: f64) -> Self {
        Self { col, row }
    }
    fn distance(self, other: Self) -> f64 {
        (self.col - other.col).hypot(self.row - other.row)
    }
}

#[derive(Default)]
pub(super) struct CursorMotion {
    pub visual: Position,
    target: Option<Position>,
    from: Position,
    started: Duration,
    last_time: Duration,
    moving: bool,
    candidate: Option<(Position, Duration, u32)>,
    input_guard: Duration,
    authorizations: VecDeque<(Option<f64>, Duration)>,
}

impl CursorMotion {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn active(&self) -> bool {
        self.moving || self.candidate.is_some()
    }

    /// Input bytes are already encoded by the terminal adapter. These permits
    /// only classify explicit editing; they do not predict a logical position.
    pub fn note_input(&mut self, bytes: &[u8], now: Duration) {
        let budget = match bytes {
            b"\x08" | b"\x7f" | b"\x02" | b"\x1b[D" | b"\x1bOD" => Some(Some(2.0)),
            b"\x01" | b"\x15" | b"\x17" | b"\x1bb" | b"\x1b[H" | b"\x1bOH" | b"\x1b[1~"
            | b"\x1b[7~" | b"\x1b[A" | b"\x1b[B" | b"\x1bOA" | b"\x1bOB" | b"\x1b[1;5D" => {
                Some(None)
            },
            _ => None,
        };
        self.authorizations.retain(|(_, until)| now < *until);
        if let Some(budget) = budget {
            if self.authorizations.len() < 8 {
                self.authorizations.push_back((budget, now + INPUT_LIFETIME));
            }
        } else if bytes.first().is_some_and(|b| *b >= 0x20 && *b != 0x7f)
            || matches!(
                bytes,
                b"\x05"
                    | b"\x1b[C"
                    | b"\x1bOC"
                    | b"\x1b[F"
                    | b"\x1bOF"
                    | b"\x1b[4~"
                    | b"\x1b[8~"
                    | b"\x1b[3~"
            )
        {
            self.input_guard = now + INPUT_LIFETIME;
        }
    }

    pub fn update(
        &mut self,
        target: Position,
        now: Duration,
        enabled: bool,
        prompt_guard: bool,
        cols: usize,
    ) -> Position {
        let now = now.max(self.last_time);
        self.last_time = now;
        self.advance(now);
        self.authorizations.retain(|(_, until)| now < *until);
        let Some(previous) = self.target.filter(|_| enabled) else {
            self.snap(target);
            return self.visual;
        };
        if target == previous {
            self.candidate = None;
            return self.visual;
        }
        let retreat = (previous.row - target.row) * cols as f64 + previous.col - target.col;
        if prompt_guard && retreat > 0.0 {
            let bounded = self
                .authorizations
                .iter()
                .position(|(budget, _)| budget.is_some_and(|max| retreat <= max));
            if let Some(index) = bounded {
                self.authorizations.remove(index);
            } else {
                let (candidate, since, frames) = self.candidate.get_or_insert((target, now, 0));
                if *candidate != target {
                    (*candidate, *since, *frames) = (target, now, 0);
                }
                *frames += 1;
                if *frames < 4 || now.saturating_sub(*since) < DURATION || now < self.input_guard {
                    return self.visual;
                }
                let permit = self
                    .authorizations
                    .iter()
                    .position(|(budget, _)| budget.is_none_or(|max| retreat <= max));
                if let Some(index) = permit {
                    self.authorizations.remove(index);
                } else {
                    self.snap(target);
                    return self.visual;
                }
            }
        } else if retreat < 0.0 {
            self.input_guard = Duration::ZERO;
        }
        self.candidate = None;
        if self.visual.distance(target) > 8.0 {
            self.snap(target);
        } else {
            self.from = self.visual;
            self.target = Some(target);
            self.started = now;
            self.moving = true;
        }
        self.visual
    }

    fn snap(&mut self, target: Position) {
        self.visual = target;
        self.target = Some(target);
        self.from = target;
        self.moving = false;
        self.candidate = None;
    }

    fn advance(&mut self, now: Duration) {
        if !self.moving {
            return;
        }
        let target = self.target.unwrap();
        let progress = now.saturating_sub(self.started).as_secs_f64() / DURATION.as_secs_f64();
        if progress >= 1.0 {
            self.visual = target;
            self.moving = false;
        } else {
            let t = ease_out_cubic(progress);
            self.visual = Position::new(
                self.from.col + (target.col - self.from.col) * t,
                self.from.row + (target.row - self.from.row) * t,
            );
        }
    }
}

fn ease_out_cubic(t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let bezier = |a: f64, b: f64, s: f64| {
        3.0 * a * (1.0 - s) * (1.0 - s) * s + 3.0 * b * (1.0 - s) * s * s + s * s * s
    };
    let (mut low, mut high) = (0.0, 1.0);
    loop {
        let s = (low + high) / 2.0;
        let x = bezier(0.215, 0.355, s);
        if (t - x).abs() < 0.001 {
            return bezier(0.61, 1.0, s);
        }
        if x < t {
            low = s;
        } else {
            high = s;
        }
    }
}

#[cfg(test)]
#[path = "cursor_motion/tests.rs"]
mod tests;
