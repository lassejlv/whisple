//! Interruptible motion for the voice window.
//!
//! Entrances use the strong ease-out curve and retarget from the current
//! value. Level meters use a critically damped spring so they track speech
//! without bouncing.

use std::time::{Duration, Instant};

/// Strong ease-out for entrances and press feedback.
/// `cubic-bezier(0.23, 1, 0.32, 1)`.
pub fn ease_out(t: f32) -> f32 {
    cubic_bezier(0.23, 1.0, 0.32, 1.0, t)
}

/// Strong ease-in-out for something already on screen that morphs.
/// `cubic-bezier(0.77, 0, 0.175, 1)`.
#[allow(dead_code)]
pub fn ease_in_out(t: f32) -> f32 {
    cubic_bezier(0.77, 0.0, 0.175, 1.0, t)
}

pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let mut t = x;
    for _ in 0..8 {
        let error = sample(t, x1, x2) - x;
        let slope = sample_derivative(t, x1, x2);
        if slope.abs() < 1.0e-4 {
            break;
        }
        t = (t - error / slope).clamp(0.0, 1.0);
    }
    sample(t, y1, y2)
}

fn sample(t: f32, a: f32, b: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
}

fn sample_derivative(t: f32, a: f32, b: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
}

/// A duration-based ease that starts from the value on screen when retargeted.
#[derive(Clone)]
pub struct Ease {
    pub value: f32,
    from: f32,
    target: f32,
    start: Instant,
    duration: Duration,
    epsilon: f32,
}

impl Ease {
    pub fn at(value: f32, duration: Duration, epsilon: f32) -> Self {
        Self {
            value,
            from: value,
            target: value,
            start: Instant::now(),
            duration,
            epsilon,
        }
    }

    pub fn chrome(value: f32) -> Self {
        Self::at(value, Duration::from_millis(220), 0.6)
    }

    pub fn press(value: f32) -> Self {
        Self::at(value, Duration::from_millis(140), 0.01)
    }

    pub fn set(&mut self, target: f32) {
        if (self.target - target).abs() <= self.epsilon {
            self.target = target;
            return;
        }
        self.from = self.value;
        self.target = target;
        self.start = Instant::now();
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.from = value;
        self.target = value;
        self.start = Instant::now();
    }

    pub fn step(&mut self) -> bool {
        self.step_at(Instant::now())
    }

    fn step_at(&mut self, now: Instant) -> bool {
        let duration = self.duration.as_secs_f32().max(0.001);
        let t = now.saturating_duration_since(self.start).as_secs_f32() / duration;
        if t >= 1.0 {
            self.value = self.target;
            return false;
        }
        self.value = self.from + (self.target - self.from) * ease_out(t);
        true
    }

    pub fn busy(&self) -> bool {
        (self.value - self.target).abs() > self.epsilon
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Grow the native window before revealing content, then hold its size
    /// until the animation settles. AppKit resizes can trigger synchronous
    /// draws, so only the content should resize on each animation frame.
    #[cfg(any(target_os = "macos", test))]
    pub fn window_height(&self, placed_height: f32) -> f32 {
        if self.value == self.target {
            self.target.round()
        } else {
            placed_height.max(self.value).max(self.target).round()
        }
    }
}

/// Critically damped spring. No bounce.
#[derive(Clone)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    pub fn level(value: f32) -> Self {
        let stiffness = 640.0;
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping: 2.0 * stiffness.sqrt(),
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
        self.target = value;
    }

    pub fn step(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0, 0.032);
        if dt == 0.0 {
            return (self.target - self.value).abs() >= 0.004 || self.velocity.abs() >= 0.02;
        }
        let accel = (self.target - self.value) * self.stiffness - self.velocity * self.damping;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;
        if (self.target - self.value).abs() < 0.004 && self.velocity.abs() < 0.02 {
            self.value = self.target;
            self.velocity = 0.0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_starts_ahead_of_linear() {
        assert!((ease_out(0.0) - 0.0).abs() < 0.001);
        assert!((ease_out(1.0) - 1.0).abs() < 0.001);
        assert!(ease_out(0.2) > 0.45);
        assert!(ease_in_out(0.0).abs() < 0.001);
        assert!((ease_in_out(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn level_spring_does_not_overshoot() {
        let mut spring = Spring::level(0.08);
        spring.target = 1.0;
        let mut peak = spring.value;
        for _ in 0..120 {
            spring.step(1.0 / 60.0);
            peak = peak.max(spring.value);
        }
        assert!(peak < 1.03, "peak {peak}");
        assert!((spring.value - 1.0).abs() < 0.02);
    }

    fn native_resizes(ease: &mut Ease, mut placed: f32) -> Vec<f32> {
        let start = ease.start;
        let mut sizes = Vec::new();
        for millis in (0..=240).step_by(8) {
            ease.step_at(start + Duration::from_millis(millis));
            let height = ease.window_height(placed);
            assert!(height + 0.5 >= ease.value, "window clipped its content");
            if height != placed {
                sizes.push(height);
                placed = height;
            }
        }
        sizes
    }

    #[test]
    fn native_window_grows_once_for_settings() {
        let mut ease = Ease::chrome(58.0);
        ease.set(431.0);
        assert_eq!(native_resizes(&mut ease, 58.0), vec![431.0]);
    }

    #[test]
    fn native_window_shrinks_once_after_settings_close() {
        let mut ease = Ease::chrome(431.0);
        ease.set(58.0);
        assert_eq!(native_resizes(&mut ease, 431.0), vec![58.0]);
    }

    #[test]
    fn native_window_shrinks_once_when_returning_from_subpage() {
        let mut ease = Ease::chrome(451.0);
        ease.set(431.0);
        assert_eq!(native_resizes(&mut ease, 451.0), vec![431.0]);
    }

    #[test]
    fn interrupted_close_keeps_space_for_reopened_settings() {
        let mut ease = Ease::chrome(503.0);
        ease.set(58.0);
        ease.step_at(ease.start + Duration::from_millis(80));
        ease.set(431.0);
        assert_eq!(native_resizes(&mut ease, 503.0), vec![431.0]);
    }
}
