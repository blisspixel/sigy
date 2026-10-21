//! Optional presentation-only motion driven by the terminal's monotonic clock.

use std::time::{Duration, Instant};

use super::{Draw, Explorer};

const FRAME: Duration = Duration::from_millis(100);
const IDLE: Duration = Duration::from_mins(5);
const REST: Duration = Duration::from_millis(250);

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Spin {
    allowed: bool,
    enabled: bool,
    active: bool,
    input: Option<Instant>,
    anchor: Option<Instant>,
    frames: u32,
}

impl Explorer {
    /// Only an interactive, continuous terminal session may animate.
    pub fn configure_motion(&mut self, interactive: bool, one_frame: bool, now: Instant) {
        self.globe.spin = Spin {
            allowed: interactive
                && !one_frame
                && !self.modes.reduced_motion
                && !self.modes.linear
                && !self.modes.monochrome,
            input: Some(now),
            ..Spin::default()
        };
    }

    pub fn motion_input(&mut self, now: Instant) {
        if self
            .globe
            .spin
            .input
            .is_none_or(|input| now.saturating_duration_since(input) >= IDLE)
        {
            self.globe.spin.anchor = None;
        }
        self.globe.spin.input = Some(now);
    }

    pub fn advance_spin(&mut self, now: Instant, visible: bool) -> Duration {
        let spin = &mut self.globe.spin;
        let active = spin.allowed
            && spin.enabled
            && visible
            && !self.globe.flat
            && spin
                .input
                .is_some_and(|input| now.saturating_duration_since(input) < IDLE);
        if spin.active != active {
            self.draw = Draw::Needed;
        }
        spin.active = active;
        if !active {
            spin.anchor = None;
            return REST;
        }
        if let Some(anchor) = spin.anchor {
            let elapsed = now.saturating_duration_since(anchor);
            if elapsed < FRAME {
                return FRAME.saturating_sub(elapsed);
            }
            self.globe.center.0 =
                super::wrap_longitude(self.globe.center.0 + elapsed.as_secs_f64() * 6.0);
            spin.frames = spin.frames.saturating_add(1);
            self.draw = Draw::Needed;
        }
        spin.anchor = Some(now);
        FRAME
    }

    pub const fn spin_label(&self) -> Option<&'static str> {
        if !self.globe.spin.allowed {
            None
        } else if !self.globe.spin.enabled {
            Some("off")
        } else if self.globe.spin.active {
            Some("on")
        } else {
            Some("paused")
        }
    }

    pub const fn spin_requested(&self) -> bool {
        self.globe.spin.allowed && self.globe.spin.enabled && !self.globe.flat
    }

    pub(super) fn toggle_spin(&mut self) {
        let spin = &mut self.globe.spin;
        if spin.allowed {
            spin.enabled = !spin.enabled;
            spin.anchor = None;
            spin.active = false;
        } else {
            self.status = "Globe rotation is disabled by the presentation or terminal mode.".into();
        }
    }

    pub(super) fn stop_spin(&mut self) {
        self.globe.spin.enabled = false;
        self.globe.spin.active = false;
        self.globe.spin.anchor = None;
    }

    pub const fn animation_frames(&self) -> u32 {
        self.globe.spin.frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::{Effect, Key, Modes};

    fn model(now: Instant) -> Explorer {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: false,
                linear: false,
                monochrome: false,
                emphasis: crate::explorer::state::Emphasis::Standard,
            },
            0,
        );
        model.configure_motion(true, false, now);
        model.handle(Key::Char('7'));
        model
    }

    #[test]
    fn elapsed_rotation_wraps_and_keeps_ordinary_idle_cadence() {
        let now = Instant::now();
        let mut model = model(now);
        assert_eq!(model.advance_spin(now, true), REST);
        assert_eq!(model.animation_frames(), 0);
        assert_eq!(model.handle(Key::Char('r')), Effect::None);
        assert_eq!(model.advance_spin(now, true), FRAME);
        assert_eq!(
            model.advance_spin(now + Duration::from_millis(40), true),
            Duration::from_millis(60)
        );
        assert_eq!(model.animation_frames(), 0);
        model.advance_spin(now + Duration::from_millis(100), true);
        assert!((model.globe_center().0 - 0.6).abs() < 1e-10);
        model.advance_spin(now + Duration::from_secs(31), true);
        assert!((model.globe_center().0 + 174.0).abs() < 1e-10);
        assert!((model.globe_center().1 - 20.0).abs() < f64::EPSILON);
        assert_eq!(model.animation_frames(), 2);
        model.handle(Key::Char('r'));
        assert_eq!(
            model.advance_spin(now + Duration::from_secs(32), true),
            REST
        );
        assert_eq!(model.spin_label(), Some("off"));
    }

    #[test]
    fn visibility_flat_and_idle_pause_without_catching_up() {
        let now = Instant::now();
        let mut model = model(now);
        model.handle(Key::Char('r'));
        model.advance_spin(now, true);
        model.advance_spin(now + Duration::from_secs(1), true);
        let center = model.globe_center();
        assert_eq!(
            model.advance_spin(now + Duration::from_secs(2), false),
            REST
        );
        assert_eq!(model.spin_label(), Some("paused"));
        model.advance_spin(now + Duration::from_secs(100), true);
        assert_eq!(model.globe_center(), center);
        model.handle(Key::Char('m'));
        assert_eq!(
            model.advance_spin(now + Duration::from_secs(101), true),
            REST
        );
        model.handle(Key::Char('m'));
        model.advance_spin(now + Duration::from_secs(200), true);
        assert_eq!(model.globe_center(), center);
        assert_eq!(model.advance_spin(now + IDLE, true), REST);
        model.motion_input(now + IDLE + Duration::from_secs(10));
        model.advance_spin(now + IDLE + Duration::from_secs(10), true);
        assert_eq!(model.globe_center(), center);
        model.advance_spin(now + IDLE + Duration::from_secs(11), true);
        assert!((model.globe_center().0 - center.0 - 6.0).abs() < 1e-10);
    }

    #[test]
    fn manual_controls_stop_and_toggle_restarts_current_longitude() {
        let now = Instant::now();
        for key in ['h', 'l', 'j', 'k', 'c'] {
            let mut model = model(now);
            model.handle(Key::Char('r'));
            model.advance_spin(now, true);
            assert_eq!(model.handle(Key::Char(key)), Effect::None);
            assert_eq!(model.spin_label(), Some("off"));
            let center = model.globe_center();
            assert_eq!(model.advance_spin(now + Duration::from_secs(2), true), REST);
            model.handle(Key::Char('r'));
            model.advance_spin(now + Duration::from_secs(3), true);
            assert_eq!(model.globe_center(), center);
        }
    }

    #[test]
    fn accessibility_noninteractive_and_one_frame_never_animate() {
        let now = Instant::now();
        for (interactive, one_frame, reduced_motion, linear, monochrome) in [
            (false, false, false, false, false),
            (true, true, false, false, false),
            (true, false, true, false, false),
            (true, false, false, true, false),
            (true, false, false, false, true),
        ] {
            let mut model = model(now);
            model.modes = Modes {
                reduced_motion,
                linear,
                monochrome,
                emphasis: crate::explorer::state::Emphasis::Standard,
            };
            model.configure_motion(interactive, one_frame, now);
            let center = model.globe_center();
            model.handle(Key::Char('r'));
            assert_eq!(model.advance_spin(now, true), REST);
            assert_eq!(model.advance_spin(now + Duration::from_secs(1), true), REST);
            assert_eq!(model.globe_center(), center);
            assert_eq!(model.animation_frames(), 0);
            assert_eq!(model.spin_label(), None);
        }
    }

    #[test]
    fn environment_modes_disable_motion_in_isolated_children()
    -> Result<(), Box<dyn std::error::Error>> {
        for (name, value) in [
            ("SIGY_REDUCED_MOTION", "1"),
            ("SIGY_LINEAR", "1"),
            ("NO_COLOR", "1"),
            ("TERM", "dumb"),
        ] {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "explorer::state::spin::tests::motion_environment_child",
                    "--ignored",
                ])
                .env_remove("SIGY_REDUCED_MOTION")
                .env_remove("SIGY_LINEAR")
                .env_remove("SIGY_MONOCHROME")
                .env_remove("SIGY_HIGH_CONTRAST")
                .env_remove("NO_COLOR")
                .env_remove("FORCE_COLOR")
                .env_remove("TERM")
                .env("SIGY_TEST_MOTION_CHILD", "1")
                .env(name, value)
                .output()?;
            assert!(
                output.status.success(),
                "{name}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[test]
    fn environment_color_flags_apply_in_isolated_children() -> Result<(), Box<dyn std::error::Error>>
    {
        for (name, value, expect) in [
            ("SIGY_LINEAR", "anything", "1,0,0,0"),
            ("SIGY_LINEAR", "", "0,0,0,0"),
            ("SIGY_MONOCHROME", "yes", "0,1,0,0"),
            ("SIGY_MONOCHROME", "", "0,0,0,0"),
            ("SIGY_REDUCED_MOTION", "yes", "0,0,0,1"),
            ("SIGY_REDUCED_MOTION", "", "0,0,0,0"),
            ("SIGY_HIGH_CONTRAST", "on", "0,0,1,0"),
            ("SIGY_HIGH_CONTRAST", "", "0,0,0,0"),
            ("NO_COLOR", "1", "0,1,0,0"),
            ("TERM", "dumb", "0,1,0,0"),
        ] {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "explorer::state::spin::tests::color_environment_child",
                    "--ignored",
                ])
                .env_remove("SIGY_REDUCED_MOTION")
                .env_remove("SIGY_LINEAR")
                .env_remove("SIGY_MONOCHROME")
                .env_remove("SIGY_HIGH_CONTRAST")
                .env_remove("NO_COLOR")
                .env_remove("FORCE_COLOR")
                .env_remove("TERM")
                .env("SIGY_TEST_COLOR_CHILD", "1")
                .env("SIGY_TEST_COLOR_EXPECT", expect)
                .env(name, value)
                .output()?;
            assert!(
                output.status.success(),
                "{name}={value}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "invoked by environment_color_flags_apply_in_isolated_children"]
    fn color_environment_child() {
        if std::env::var("SIGY_TEST_COLOR_CHILD").ok().as_deref() != Some("1") {
            return;
        }
        let modes = Modes::resolve(false, false, false);
        let got = format!(
            "{},{},{},{}",
            u8::from(modes.linear),
            u8::from(modes.monochrome),
            u8::from(modes.emphasis.high()),
            u8::from(modes.reduced_motion)
        );
        let expect = std::env::var("SIGY_TEST_COLOR_EXPECT").unwrap_or_default();
        assert_eq!(got, expect);
    }

    #[test]
    #[ignore = "invoked by environment_modes_disable_motion_in_isolated_children"]
    fn motion_environment_child() {
        if std::env::var("SIGY_TEST_MOTION_CHILD").ok().as_deref() != Some("1") {
            return;
        }
        let now = Instant::now();
        let mut model = Explorer::new(Modes::resolve(false, false, false), 0);
        model.configure_motion(true, false, now);
        model.handle(Key::Char('7'));
        model.handle(Key::Char('r'));
        assert_eq!(model.advance_spin(now, true), REST);
        assert_eq!(model.advance_spin(now + Duration::from_secs(1), true), REST);
        assert_eq!(model.globe_center(), (0.0, 20.0));
        assert_eq!(model.animation_frames(), 0);
    }
}
