//! Optional presentation-only motion driven by the terminal's monotonic clock.

use std::time::{Duration, Instant};

use super::{Draw, Explorer};

const FRAME: Duration = Duration::from_millis(100);
const IDLE: Duration = Duration::from_mins(5);
const REST: Duration = Duration::from_millis(250);
const GLIDE: Duration = Duration::from_millis(400);
const FLOURISH: Duration = Duration::from_millis(800);
const RAMP: Duration = Duration::from_millis(400);
const RAMP_SECS: f64 = 0.4;
const CRUISE: f64 = 6.0;
const FLOURISH_DEGREES: f64 = 24.0;
const PULSE: u128 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Run {
    #[default]
    Idle,
    Running,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Flourish {
    #[default]
    Pending,
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    origin: Instant,
    duration: Duration,
    start_lon: f64,
    start_lat: f64,
    delta_lon: f64,
    delta_lat: f64,
    target_lon: f64,
    target_lat: f64,
    flourish: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spin {
    allowed: bool,
    enabled: bool,
    pulse_lit: bool,
    run: Run,
    input: Option<Instant>,
    anchor: Option<Instant>,
    ramp_origin: Option<Instant>,
    stop_origin: Option<Instant>,
    stop_speed: f64,
    frames: u32,
    glide: Option<Glide>,
    flourish: Flourish,
    pulse_origin: Option<Instant>,
}

impl Default for Spin {
    fn default() -> Self {
        Self {
            allowed: false,
            enabled: false,
            pulse_lit: true,
            run: Run::Idle,
            input: None,
            anchor: None,
            ramp_origin: None,
            stop_origin: None,
            stop_speed: 0.0,
            frames: 0,
            glide: None,
            flourish: Flourish::Pending,
            pulse_origin: None,
        }
    }
}

impl Explorer {
    /// Only an interactive, continuous terminal session may animate.
    pub fn configure_motion(&mut self, interactive: bool, one_frame: bool, now: Instant) {
        let allowed = interactive
            && !one_frame
            && !self.modes.reduced_motion
            && !self.modes.linear
            && !self.modes.monochrome;
        self.globe.spin = Spin {
            allowed,
            input: Some(now),
            flourish: if allowed {
                Flourish::Pending
            } else {
                Flourish::Finished
            },
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

    /// Land an in-progress turn so the key that interrupted it is still applied.
    pub(super) fn settle_motion(&mut self) {
        let Some(glide) = self.globe.spin.glide.take() else {
            return;
        };
        self.globe.center = (glide.target_lon, glide.target_lat);
        if glide.flourish {
            self.globe.spin.flourish = Flourish::Finished;
        }
    }

    pub(super) fn turn_by(&mut self, dlon: f64, dlat: f64) {
        self.stop_spin();
        let (lon, lat) = self.globe.center;
        self.glide_toward(
            super::wrap_longitude(lon + dlon),
            (lat + dlat).clamp(-90.0, 90.0),
        );
    }

    pub(super) fn glide_toward(&mut self, longitude: f64, latitude: f64) {
        self.glide_for(longitude, latitude, GLIDE, false);
    }

    /// Start the one-session flourish when the globe canvas can hold it.
    pub fn note_globe_canvas(&mut self, width: u16, height: u16) {
        if self.globe.spin.flourish != Flourish::Pending {
            return;
        }
        if !self.globe.spin.allowed || width < 20 || height < 8 {
            self.globe.spin.flourish = Flourish::Finished;
            return;
        }
        let (lon, lat) = self.globe.center;
        self.globe.center = (super::wrap_longitude(lon - FLOURISH_DEGREES), lat);
        self.globe.spin.flourish = Flourish::Running;
        self.glide_for(lon, lat, FLOURISH, true);
        self.draw = Draw::Needed;
    }

    pub const fn flourish_wordmark(&self) -> bool {
        matches!(self.globe.spin.flourish, Flourish::Running)
    }

    pub fn advance_spin(&mut self, now: Instant, visible: bool) -> Duration {
        let glide = self.advance_glide(now);
        let spin = self.advance_rotation(now, visible);
        let pulse = self.advance_pulse(now);
        glide.min(spin).min(pulse)
    }

    pub const fn spin_label(&self) -> Option<&'static str> {
        if !self.globe.spin.allowed {
            None
        } else if !self.globe.spin.enabled {
            Some("off")
        } else if matches!(self.globe.spin.run, Run::Running | Run::Stopping) {
            Some("on")
        } else {
            Some("paused")
        }
    }

    pub const fn spin_requested(&self) -> bool {
        self.globe.spin.allowed && self.globe.spin.enabled && !self.globe.flat
    }

    pub(super) fn toggle_spin(&mut self) {
        if !self.globe.spin.allowed {
            self.status = "Globe turning is off in this presentation mode.".into();
            return;
        }
        if self.globe.spin.run == Run::Stopping {
            self.stop_spin();
            return;
        }
        if self.globe.spin.enabled {
            self.globe.spin.run = Run::Stopping;
            return;
        }
        self.globe.spin.enabled = true;
        self.globe.spin.run = Run::Idle;
        self.globe.spin.anchor = None;
        self.globe.spin.ramp_origin = None;
        self.globe.spin.stop_origin = None;
        self.globe.spin.stop_speed = 0.0;
    }

    pub(super) fn stop_spin(&mut self) {
        let spin = &mut self.globe.spin;
        spin.enabled = false;
        spin.run = Run::Idle;
        spin.anchor = None;
        spin.ramp_origin = None;
        spin.stop_origin = None;
        spin.stop_speed = 0.0;
    }

    pub const fn animation_frames(&self) -> u32 {
        self.globe.spin.frames
    }

    /// The station whose play was confirmed, while its receipt is still running.
    #[must_use]
    pub fn playing_station(&self) -> Option<&str> {
        let running = self
            .playback
            .as_ref()
            .is_some_and(|view| matches!(view.state.as_str(), "running" | "playing"));
        if running {
            self.live_station.as_deref()
        } else {
            None
        }
    }

    /// `true` while the live marker is drawn. Reduced motion keeps it steady.
    #[must_use]
    pub fn marker_pulse_lit(&self) -> bool {
        if self.playing_station().is_none() {
            return false;
        }
        if !self.globe.spin.allowed {
            return true;
        }
        self.globe.spin.pulse_lit
    }

    #[cfg(test)]
    pub fn place_globe_for_test(&mut self, longitude: f64, latitude: f64) {
        self.globe.center = (longitude, latitude);
        self.globe.spin.glide = None;
    }

    #[cfg(test)]
    pub fn show_playing_station_for_test(&mut self, id: &str) {
        self.live_station = Some(id.to_owned());
        self.playback = Some(super::PlaybackView {
            id: "listen".to_owned(),
            state: "running".to_owned(),
            format: None,
            source_revision: "revision".to_owned(),
            failure: None,
        });
    }

    fn glide_for(&mut self, longitude: f64, latitude: f64, duration: Duration, flourish: bool) {
        let latitude = latitude.clamp(-90.0, 90.0);
        let longitude = presented_longitude(longitude);
        let (from_lon, from_lat) = self.globe.center;
        let delta_lon = super::wrap_longitude(longitude - from_lon);
        let delta_lat = latitude - from_lat;
        if !flourish && self.globe.spin.flourish == Flourish::Running {
            self.globe.spin.flourish = Flourish::Finished;
        }
        let instant = !self.globe.spin.allowed
            || self.globe.spin.input.is_none()
            || (delta_lon.abs() < 1e-9 && delta_lat.abs() < 1e-9);
        if instant {
            self.globe.center = (longitude, latitude);
            self.globe.spin.glide = None;
            if flourish {
                self.globe.spin.flourish = Flourish::Finished;
            }
            return;
        }
        let Some(origin) = self.globe.spin.input else {
            return;
        };
        self.globe.spin.glide = Some(Glide {
            origin,
            duration,
            start_lon: from_lon,
            start_lat: from_lat,
            delta_lon,
            delta_lat,
            target_lon: longitude,
            target_lat: latitude,
            flourish,
        });
    }

    fn advance_glide(&mut self, now: Instant) -> Duration {
        let Some(glide) = self.globe.spin.glide else {
            return REST;
        };
        let elapsed = now.saturating_duration_since(glide.origin);
        if elapsed.is_zero() {
            return FRAME;
        }
        let duration = glide.duration.as_secs_f64().max(f64::EPSILON);
        let t = elapsed.as_secs_f64() / duration;
        let eased = ease_out(t);
        self.globe.center.0 = super::wrap_longitude(glide.start_lon + glide.delta_lon * eased);
        self.globe.center.1 = (glide.start_lat + glide.delta_lat * eased).clamp(-90.0, 90.0);
        self.globe.spin.frames = self.globe.spin.frames.saturating_add(1);
        self.draw = Draw::Needed;
        if t >= 1.0 {
            self.globe.center = (glide.target_lon, glide.target_lat);
            self.globe.spin.glide = None;
            if glide.flourish {
                self.globe.spin.flourish = Flourish::Finished;
            }
            return REST;
        }
        FRAME
    }

    fn advance_rotation(&mut self, now: Instant, visible: bool) -> Duration {
        if self.globe.spin.run == Run::Stopping {
            return self.advance_stop(now, visible);
        }
        let fresh = self
            .globe
            .spin
            .input
            .is_some_and(|input| now.saturating_duration_since(input) < IDLE);
        let wants = self.globe.spin.allowed
            && self.globe.spin.enabled
            && visible
            && !self.globe.flat
            && fresh;
        let running = self.globe.spin.run == Run::Running;
        if running != wants {
            self.draw = Draw::Needed;
        }
        if !wants {
            self.globe.spin.run = Run::Idle;
            self.globe.spin.anchor = None;
            self.globe.spin.ramp_origin = None;
            return REST;
        }
        self.globe.spin.run = Run::Running;
        if self.globe.spin.ramp_origin.is_none() {
            self.globe.spin.ramp_origin = Some(now);
        }
        if let Some(anchor) = self.globe.spin.anchor {
            let elapsed = now.saturating_duration_since(anchor);
            if elapsed < FRAME {
                return FRAME.saturating_sub(elapsed);
            }
            let origin = self.globe.spin.ramp_origin.unwrap_or(anchor);
            let travel = spin_travel(
                anchor.saturating_duration_since(origin).as_secs_f64(),
                now.saturating_duration_since(origin).as_secs_f64(),
            );
            if travel.abs() > 1e-12 {
                self.globe.center.0 = super::wrap_longitude(self.globe.center.0 + travel);
                self.globe.spin.frames = self.globe.spin.frames.saturating_add(1);
                self.draw = Draw::Needed;
            }
        }
        self.globe.spin.anchor = Some(now);
        FRAME
    }

    fn advance_stop(&mut self, now: Instant, visible: bool) -> Duration {
        let show = visible && !self.globe.flat && self.globe.spin.allowed;
        if !show {
            self.stop_spin();
            return REST;
        }
        if self.globe.spin.stop_origin.is_none() {
            let anchor = self.globe.spin.anchor.unwrap_or(now);
            let speed = self.globe.spin.ramp_origin.map_or(0.0, |origin| {
                cruise_speed(anchor.saturating_duration_since(origin).as_secs_f64())
            });
            if speed.abs() < 1e-12 {
                self.stop_spin();
                return REST;
            }
            self.globe.spin.stop_origin = Some(anchor);
            self.globe.spin.stop_speed = speed;
            self.globe.spin.anchor = Some(anchor);
        }
        let origin = self.globe.spin.stop_origin.unwrap_or(now);
        let speed = self.globe.spin.stop_speed;
        let elapsed = now.saturating_duration_since(origin);
        let anchor = self.globe.spin.anchor.unwrap_or(origin);
        let step = now.saturating_duration_since(anchor);
        if elapsed < RAMP && step < FRAME {
            return FRAME.saturating_sub(step);
        }
        let travel = decel_travel(
            speed,
            anchor.saturating_duration_since(origin).as_secs_f64(),
            elapsed.as_secs_f64().min(RAMP_SECS),
        );
        if travel.abs() > 1e-12 {
            self.globe.center.0 = super::wrap_longitude(self.globe.center.0 + travel);
            self.globe.spin.frames = self.globe.spin.frames.saturating_add(1);
            self.draw = Draw::Needed;
        }
        if elapsed >= RAMP {
            self.stop_spin();
            return REST;
        }
        self.globe.spin.anchor = Some(now);
        self.globe.spin.run = Run::Stopping;
        FRAME
    }

    fn advance_pulse(&mut self, now: Instant) -> Duration {
        if self.playing_station().is_none() || !self.globe.spin.allowed {
            return REST;
        }
        if self.globe.spin.pulse_origin.is_none() {
            self.globe.spin.pulse_origin = Some(now);
        }
        let origin = self.globe.spin.pulse_origin.unwrap_or(now);
        let elapsed = now.saturating_duration_since(origin);
        let lit = (elapsed.as_millis() / PULSE).is_multiple_of(2);
        if lit != self.globe.spin.pulse_lit {
            self.globe.spin.pulse_lit = lit;
            self.globe.spin.frames = self.globe.spin.frames.saturating_add(1);
            self.draw = Draw::Needed;
        }
        let remaining = PULSE - (elapsed.as_millis() % PULSE);
        Duration::from_millis(
            u64::try_from(remaining).unwrap_or(u64::try_from(PULSE).unwrap_or(500)),
        )
    }
}

/// Keep an in-range longitude's bits. Wrapping `-73.6` again is not the same value.
fn presented_longitude(value: f64) -> f64 {
    if (-180.0..=180.0).contains(&value) {
        value
    } else {
        super::wrap_longitude(value)
    }
}

fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn cruise_speed(t: f64) -> f64 {
    if t <= 0.0 {
        0.0
    } else if t < RAMP_SECS {
        CRUISE * (t / RAMP_SECS)
    } else {
        CRUISE
    }
}

/// Distance traveled while speed rises linearly from 0 to 6 deg/s over 400 ms.
fn spin_travel(t0: f64, t1: f64) -> f64 {
    fn integral(t: f64) -> f64 {
        if t <= 0.0 {
            0.0
        } else if t < RAMP_SECS {
            (CRUISE / (2.0 * RAMP_SECS)) * t * t
        } else {
            (CRUISE * RAMP_SECS / 2.0) + CRUISE * (t - RAMP_SECS)
        }
    }
    integral(t1) - integral(t0)
}

/// Distance while speed falls linearly from `speed` to 0 over 400 ms.
fn decel_travel(speed: f64, t0: f64, t1: f64) -> f64 {
    fn integral(speed: f64, t: f64) -> f64 {
        let t = t.clamp(0.0, RAMP_SECS);
        speed * (t - (t * t) / (2.0 * RAMP_SECS))
    }
    integral(speed, t1) - integral(speed, t0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::state::{Effect, Key, Modes};

    fn motion_model(now: Instant) -> Explorer {
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

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn elapsed_rotation_wraps_and_keeps_ordinary_idle_cadence() {
        let now = Instant::now();
        let mut model = motion_model(now);
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
        near(model.globe_center().0, 0.075);
        model.advance_spin(now + Duration::from_secs(31), true);
        near(model.globe_center().0, -175.2);
        near(model.globe_center().1, 20.0);
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
        let mut model = motion_model(now);
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
        near(model.globe_center().0, center.0 + 4.8);
    }

    #[test]
    fn manual_controls_stop_and_toggle_restarts_current_longitude() {
        let now = Instant::now();
        for key in ['h', 'l', 'j', 'k', 'c'] {
            let mut model = motion_model(now);
            model.handle(Key::Char('r'));
            model.advance_spin(now, true);
            assert_eq!(model.handle(Key::Char(key)), Effect::None);
            assert_eq!(model.spin_label(), Some("off"));
            model.advance_spin(now + Duration::from_millis(400), true);
            let center = model.globe_center();
            let expected = match key {
                'h' => (-15.0, 20.0),
                'l' => (15.0, 20.0),
                'j' => (0.0, 5.0),
                'k' => (0.0, 35.0),
                _ => (0.0, 20.0),
            };
            near(center.0, expected.0);
            near(center.1, expected.1);
            assert_eq!(model.advance_spin(now + Duration::from_secs(2), true), REST);
            assert_eq!(model.globe_center(), center);
            model.handle(Key::Char('r'));
            model.advance_spin(now + Duration::from_secs(3), true);
            assert_eq!(model.globe_center(), center);
        }
    }

    #[test]
    fn eased_steps_use_the_short_path_and_a_second_key_lands_immediately() {
        let now = Instant::now();
        let mut model = motion_model(now);
        assert_eq!(model.focus(), crate::explorer::state::Focus::Results);
        assert_eq!(model.handle(Key::Left), Effect::None);
        assert_eq!(model.workspace(), crate::explorer::state::Workspace::Globe);
        near(model.globe_center().0, 0.0);
        model.advance_spin(now + Duration::from_millis(200), true);
        near(model.globe_center().0, -13.125);
        model.advance_spin(now + Duration::from_millis(400), true);
        near(model.globe_center().0, -15.0);
        near(model.globe_center().1, 20.0);
        model.motion_input(now + Duration::from_millis(400));
        model.handle(Key::Char('l'));
        near(model.globe_center().0, -15.0);
        model.advance_spin(now + Duration::from_millis(400), true);
        near(model.globe_center().0, -15.0);
        model.advance_spin(now + Duration::from_millis(800), true);
        near(model.globe_center().0, 0.0);
        model.handle(Key::Tab);
        assert_eq!(model.focus(), crate::explorer::state::Focus::Detail);
        model.handle(Key::Right);
        assert_eq!(
            model.workspace(),
            crate::explorer::state::Workspace::Explore
        );

        let mut across = motion_model(now);
        across.place_globe_for_test(170.0, 0.0);
        across.handle(Key::Char('l'));
        across.advance_spin(now + GLIDE, true);
        near(across.globe_center().0, -175.0);
        across.place_globe_for_test(-170.0, 10.0);
        across.motion_input(now + GLIDE);
        across.handle(Key::Char('h'));
        across.advance_spin(now + GLIDE + GLIDE, true);
        near(across.globe_center().0, 175.0);
        near(across.globe_center().1, 10.0);
    }

    #[test]
    fn motion_off_steps_center_and_flourish_are_instant_or_skipped() {
        let now = Instant::now();
        let mut model = motion_model(now);
        model.modes.reduced_motion = true;
        model.configure_motion(true, false, now);
        let center = model.globe_center();
        model.note_globe_canvas(80, 24);
        assert_eq!(model.globe_center(), center);
        assert!(!model.flourish_wordmark());
        model.handle(Key::Left);
        assert_eq!(model.globe_center(), (-15.0, 20.0));
        assert_eq!(model.animation_frames(), 0);
    }

    #[test]
    fn flourish_plays_once_and_a_key_skips_to_the_target() {
        let now = Instant::now();
        let mut model = motion_model(now);
        let home = model.globe_center();
        model.note_globe_canvas(8, 4);
        assert_eq!(model.globe_center(), home);
        assert!(!model.flourish_wordmark());
        model.note_globe_canvas(80, 24);
        assert_eq!(model.globe_center(), home);

        let mut shown = motion_model(now);
        shown.note_globe_canvas(40, 16);
        assert!(shown.flourish_wordmark());
        near(shown.globe_center().0, home.0 - FLOURISH_DEGREES);
        shown.motion_input(now + Duration::from_millis(100));
        shown.handle(Key::Char('l'));
        near(shown.globe_center().0, home.0);
        assert!(!shown.flourish_wordmark());
        shown.advance_spin(now + Duration::from_millis(100), true);
        near(shown.globe_center().0, home.0);
        shown.advance_spin(now + Duration::from_millis(500), true);
        near(shown.globe_center().0, home.0 + 15.0);
        shown.note_globe_canvas(80, 24);
        near(shown.globe_center().0, home.0 + 15.0);
    }

    #[test]
    fn live_marker_pulses_twice_a_second_and_freezes_without_motion() {
        let now = Instant::now();
        let mut model = motion_model(now);
        model.show_playing_station_for_test("station-a");
        assert!(model.marker_pulse_lit());
        for step in 0..=10 {
            model.advance_spin(now + Duration::from_millis(step * 100), true);
        }
        assert_eq!(model.animation_frames(), 2);
        assert!(model.marker_pulse_lit());

        let mut still = motion_model(now);
        still.modes.reduced_motion = true;
        still.configure_motion(true, false, now);
        still.show_playing_station_for_test("station-a");
        still.advance_spin(now + Duration::from_secs(2), true);
        assert!(still.marker_pulse_lit());
        assert_eq!(still.animation_frames(), 0);
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
            let mut model = motion_model(now);
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
