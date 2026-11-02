//! Motion settings: how widget numbers glide to their new values, and how widgets fade in.
//! Each has its own timing curve, a CSS-style cubic Bézier the dashboard lets you drag into shape.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A timing curve from (0, 0) to (1, 1), shaped by two control points, like CSS `cubic-bezier()`.
/// x is time and y is progress, both 0–1; y may go past them, which overshoots.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// How far up and down a control point can be dragged.
pub const CURVE_MIN_Y: f32 = -0.5;
pub const CURVE_MAX_Y: f32 = 1.5;

impl Curve {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    pub const LINEAR: Curve = Curve::new(0., 0., 1., 1.);
    pub const EASE: Curve = Curve::new(0.25, 0.1, 0.25, 1.);
    pub const EASE_OUT: Curve = Curve::new(0., 0., 0.58, 1.);

    /// The named curves the dashboard offers as starting points (the first five are CSS's own).
    pub const PRESETS: [(&'static str, Curve); 7] = [
        ("Linear", Curve::LINEAR),
        ("Ease", Curve::EASE),
        ("Ease in", Curve::new(0.42, 0., 1., 1.)),
        ("Ease out", Curve::EASE_OUT),
        ("Ease in-out", Curve::new(0.42, 0., 0.58, 1.)),
        ("Snappy", Curve::new(0.2, 0.9, 0.1, 1.)),
        ("Overshoot", Curve::new(0.34, 1.4, 0.64, 1.)),
    ];

    /// Progress at time `t` (0–1): find where the curve is at x = `t`, and return its y there.
    pub fn ease(self, t: f32) -> f32 {
        if t <= 0. {
            return 0.;
        }
        if t >= 1. {
            return 1.;
        }
        bezier(self.y1, self.y2, self.solve_x(t))
    }

    /// The curve's own parameter (0–1) where its x is `x`. Newton's method converges in a few
    /// steps for most curves; halving the range is the fallback when the slope is near flat.
    fn solve_x(self, x: f32) -> f32 {
        let mut s = x;
        for _ in 0..8 {
            let error = bezier(self.x1, self.x2, s) - x;
            if error.abs() < 1e-5 {
                return s;
            }
            let slope = bezier_slope(self.x1, self.x2, s);
            if slope.abs() < 1e-6 {
                break;
            }
            s -= error / slope;
        }
        let (mut low, mut high) = (0., 1.);
        s = x;
        for _ in 0..30 {
            if bezier(self.x1, self.x2, s) < x { low = s } else { high = s }
            s = (low + high) / 2.;
        }
        s
    }

    /// Control point 0 or 1, as (x, y).
    pub fn point(self, index: usize) -> (f32, f32) {
        if index == 0 { (self.x1, self.y1) } else { (self.x2, self.y2) }
    }

    /// Moves control point 0 or 1. x stays within 0–1 so the curve never goes back in time.
    pub fn set_point(&mut self, index: usize, x: f32, y: f32) {
        let (x, y) = (x.clamp(0., 1.), y.clamp(CURVE_MIN_Y, CURVE_MAX_Y));
        if index == 0 {
            (self.x1, self.y1) = (x, y);
        } else {
            (self.x2, self.y2) = (x, y);
        }
    }

    /// The preset this curve is, if any.
    pub fn preset_name(self) -> Option<&'static str> {
        let close = |a: f32, b: f32| (a - b).abs() < 0.005;
        Self::PRESETS.iter().find_map(|(name, preset)| {
            let same = close(self.x1, preset.x1)
                && close(self.y1, preset.y1)
                && close(self.x2, preset.x2)
                && close(self.y2, preset.y2);
            same.then_some(*name)
        })
    }
}

/// One coordinate of a cubic Bézier from 0 to 1 with control values `a` and `b`, at `t`.
fn bezier(a: f32, b: f32, t: f32) -> f32 {
    let u = 1. - t;
    3. * u * u * t * a + 3. * u * t * t * b + t * t * t
}

fn bezier_slope(a: f32, b: f32, t: f32) -> f32 {
    let u = 1. - t;
    3. * u * u * a + 6. * u * t * (b - a) + 3. * t * t * (1. - b)
}

/// One animation: on or off, how long it takes and how it moves.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    pub enabled: bool,
    pub duration_ms: f32,
    pub curve: Curve,
}

impl Default for Motion {
    fn default() -> Self {
        Self { enabled: true, duration_ms: 500., curve: Curve::EASE_OUT }
    }
}

impl Motion {
    /// The eased progress of an animation that started at `started`, or `None` once it's
    /// finished (or when this animation is off), so the caller can stop asking for frames.
    pub fn progress(&self, started: Instant) -> Option<f32> {
        if !self.enabled || self.duration_ms <= 0. {
            return None;
        }
        let t = started.elapsed().as_secs_f32() * 1000. / self.duration_ms;
        (t < 1.).then(|| self.curve.ease(t))
    }
}

/// Which of the two animations; the dashboard has an editor for each.
#[derive(Clone, Copy, PartialEq)]
pub enum MotionKind {
    Values,
    FadeIn,
}

/// All motion settings, shared by every widget. Saved with the dashboard.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Animations {
    /// Numbers, bars and labels glide to each new reading instead of jumping.
    pub values: Motion,
    /// A widget fades in when it's shown on the desktop.
    pub fade_in: Motion,
}

impl Default for Animations {
    fn default() -> Self {
        Self {
            values: Motion { duration_ms: 600., curve: Curve::EASE_OUT, ..Default::default() },
            fade_in: Motion { duration_ms: 400., curve: Curve::EASE, ..Default::default() },
        }
    }
}

impl Animations {
    pub fn get(&self, kind: MotionKind) -> &Motion {
        match kind {
            MotionKind::Values => &self.values,
            MotionKind::FadeIn => &self.fade_in,
        }
    }

    pub fn get_mut(&mut self, kind: MotionKind) -> &mut Motion {
        match kind {
            MotionKind::Values => &mut self.values,
            MotionKind::FadeIn => &mut self.fade_in,
        }
    }
}

/// `from` moved `t` of the way to `to` (`t` past 1 overshoots).
pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        for i in 0..=10 {
            let t = i as f32 / 10.;
            assert!((Curve::LINEAR.ease(t) - t).abs() < 1e-4);
        }
    }

    #[test]
    fn ends_are_fixed_and_ease_out_leads() {
        for (_, curve) in Curve::PRESETS {
            assert_eq!(curve.ease(0.), 0.);
            assert_eq!(curve.ease(1.), 1.);
        }
        assert!(Curve::EASE_OUT.ease(0.5) > 0.5);
    }
}
