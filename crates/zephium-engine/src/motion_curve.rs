//! The chrome's timing curves, for native motion that is stepped by hand
//! rather than handed to a compositor (Windows moves page windows itself).

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

/// `--ease-emphasized`: a surface arriving, and the page sliding beside the
/// sidebar. It must stay the curve the chrome's CSS uses for the same move.
pub(crate) const EMPHASIZED: CubicBezier = CubicBezier::new(0.16, 1.0, 0.3, 1.0);

impl CubicBezier {
    pub(crate) const fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn component(s: f64, a: f64, b: f64) -> f64 {
        let inverse = 1.0 - s;
        3.0 * inverse * inverse * s * a + 3.0 * inverse * s * s * b + s * s * s
    }

    fn slope(s: f64, a: f64, b: f64) -> f64 {
        let inverse = 1.0 - s;
        3.0 * inverse * inverse * a + 6.0 * inverse * s * (b - a) + 3.0 * s * s * (1.0 - b)
    }

    /// Progress at time `t` in `0..=1`, as a browser evaluates the same curve.
    pub(crate) fn at(&self, t: f64) -> f64 {
        if !t.is_finite() || t <= 0.0 {
            return 0.0;
        }
        if t >= 1.0 {
            return 1.0;
        }
        // Newton's method from the linear guess, falling back to bisection
        // where the slope flattens; both converge well inside a frame.
        let mut s = t;
        for _ in 0..8 {
            let error = Self::component(s, self.x1, self.x2) - t;
            if error.abs() < 1e-6 {
                return Self::component(s, self.y1, self.y2);
            }
            let slope = Self::slope(s, self.x1, self.x2);
            if slope.abs() < 1e-6 {
                break;
            }
            s -= error / slope;
        }
        let (mut low, mut high) = (0.0, 1.0);
        s = t;
        for _ in 0..32 {
            let x = Self::component(s, self.x1, self.x2);
            if (x - t).abs() < 1e-6 {
                break;
            }
            if x < t {
                low = s;
            } else {
                high = s;
            }
            s = (low + high) / 2.0;
        }
        Self::component(s, self.y1, self.y2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_are_exact_and_the_curve_never_turns_back() {
        assert_eq!(EMPHASIZED.at(0.0), 0.0);
        assert_eq!(EMPHASIZED.at(1.0), 1.0);
        assert_eq!(EMPHASIZED.at(-1.0), 0.0);
        assert_eq!(EMPHASIZED.at(f64::NAN), 0.0);
        let mut last = 0.0;
        for step in 1..=100 {
            let value = EMPHASIZED.at(f64::from(step) / 100.0);
            assert!(value >= last, "progress must not reverse at step {step}");
            last = value;
        }
    }

    #[test]
    fn it_matches_the_decelerating_shape_the_chrome_draws() {
        // Most of the distance is covered early: that is what makes a long
        // duration read as soft rather than slow.
        assert!(EMPHASIZED.at(0.2) > 0.6);
        assert!(EMPHASIZED.at(0.5) > 0.9);
        let linear = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
        for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
            assert!((linear.at(t) - t).abs() < 1e-4);
        }
    }
}
