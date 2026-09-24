//! Globe geometry from one explicit instant.
//!
//! This module provides the orthographic globe projection, the equirectangular
//! world map, horizon and antimeridian segment handling, and the solar position
//! used for a geometric day/night overlay. It computes geometry only: it renders
//! nothing, holds no coastline data, and never reads the system clock. Every
//! solar result is a function of a [`UtcInstant`] supplied by the caller.
//!
//! Angles at the public boundary are degrees. Longitude is normalized to
//! `[-180, 180)` and latitude is limited to `[-90, 90]`. Coordinates describe a
//! sphere; no ellipsoid, datum, elevation, or atmospheric refraction is modeled.

use std::fmt;

/// Zenith angle of the geometric horizon. Larger values are geometric night.
pub const GEOMETRIC_HORIZON_ZENITH_DEG: f64 = 90.0;
/// Zenith angle that ends civil twilight.
pub const CIVIL_TWILIGHT_ZENITH_DEG: f64 = 96.0;
/// Zenith angle that ends nautical twilight.
pub const NAUTICAL_TWILIGHT_ZENITH_DEG: f64 = 102.0;
/// Zenith angle that ends astronomical twilight.
pub const ASTRONOMICAL_TWILIGHT_ZENITH_DEG: f64 = 108.0;

/// Julian day of 1970-01-01T00:00:00Z.
const UNIX_EPOCH_JULIAN_DAY: f64 = 2_440_587.5;
/// Julian day of J2000.0, 2000-01-01T12:00:00 TT.
const J2000_JULIAN_DAY: f64 = 2_451_545.0;
const SECONDS_PER_DAY: i64 = 86_400;
/// Julian day 0 is -4712-01-01T12:00:00 in the proleptic Julian calendar.
const MIN_JULIAN_DAY: f64 = 0.0;
/// Julian day of 10000-01-01T00:00:00Z in the proleptic Gregorian calendar.
const MAX_JULIAN_DAY: f64 = 5_373_484.5;
/// Unix seconds of Julian day 0.
const MIN_UNIX_SECONDS: i64 = -210_866_760_000;
/// Unix seconds of 10000-01-01T00:00:00Z.
const MAX_UNIX_SECONDS: i64 = 253_402_300_800;

/// Slack for a projected point that rounding places just outside the unit disc.
const DISC_EDGE_TOLERANCE: f64 = 1e-12;
/// Slack for a point whose `cos c` rounding places just behind the horizon.
const HORIZON_TOLERANCE: f64 = 1e-12;
/// Dot product at or below which two unit vectors are treated as antipodal.
const ANTIPODAL_DOT: f64 = -1.0 + 1e-12;
/// Fixed bisection bound for a horizon crossing; 2^-60 of an arc is far below
/// any terminal cell.
const HORIZON_BISECTION_STEPS: u32 = 60;

/// Rejected geometric input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidGeometry {
    /// A coordinate or instant was NaN or infinite.
    NonFinite,
    /// A latitude was outside `[-90, 90]`.
    LatitudeOutOfRange,
    /// An instant was outside Julian day 0 through the year 9999.
    InstantOutOfRange,
    /// Antipodal endpoints do not define one great-circle arc.
    AntipodalArc,
}

impl fmt::Display for InvalidGeometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "coordinate or instant is not finite",
            Self::LatitudeOutOfRange => "latitude is outside -90 to 90 degrees",
            Self::InstantOutOfRange => "instant is outside Julian day 0 through the year 9999",
            Self::AntipodalArc => "antipodal endpoints do not define one arc",
        })
    }
}

impl std::error::Error for InvalidGeometry {}

/// Wraps a longitude in degrees to `[-180, 180)`. Non-finite input stays
/// non-finite.
#[must_use]
pub fn normalize_longitude(lon_deg: f64) -> f64 {
    let wrapped = (lon_deg + 180.0).rem_euclid(360.0) - 180.0;
    // rem_euclid can round a tiny negative remainder up to the full turn.
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else {
        wrapped
    }
}

/// A validated point on the sphere in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LonLat {
    lon: f64,
    lat: f64,
}

impl LonLat {
    /// Validates a point and wraps its longitude to `[-180, 180)`.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidGeometry::NonFinite`] for NaN or infinite input and
    /// [`InvalidGeometry::LatitudeOutOfRange`] for a latitude outside
    /// `[-90, 90]`.
    pub fn new(lon_deg: f64, lat_deg: f64) -> Result<Self, InvalidGeometry> {
        if !lon_deg.is_finite() || !lat_deg.is_finite() {
            return Err(InvalidGeometry::NonFinite);
        }
        if !(-90.0..=90.0).contains(&lat_deg) {
            return Err(InvalidGeometry::LatitudeOutOfRange);
        }
        Ok(Self {
            lon: normalize_longitude(lon_deg),
            lat: lat_deg,
        })
    }

    /// Longitude in degrees, in `[-180, 180)`.
    #[must_use]
    pub const fn lon(self) -> f64 {
        self.lon
    }

    /// Latitude in degrees, in `[-90, 90]`.
    #[must_use]
    pub const fn lat(self) -> f64 {
        self.lat
    }

    /// The antipodal point.
    #[must_use]
    pub fn antipode(self) -> Self {
        Self {
            lon: normalize_longitude(self.lon + 180.0),
            lat: -self.lat,
        }
    }

    /// Great-circle angle between two points in degrees, in `[0, 180]`.
    #[must_use]
    pub fn angular_distance(self, other: Self) -> f64 {
        dot(self.unit_vector(), other.unit_vector())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    fn unit_vector(self) -> [f64; 3] {
        let (sin_lat, cos_lat) = self.lat.to_radians().sin_cos();
        let (sin_lon, cos_lon) = self.lon.to_radians().sin_cos();
        [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat]
    }

    fn from_vector(v: [f64; 3]) -> Self {
        let horizontal = v[0].hypot(v[1]);
        Self {
            lon: normalize_longitude(v[1].atan2(v[0]).to_degrees()),
            lat: v[2].atan2(horizontal).to_degrees().clamp(-90.0, 90.0),
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A point on a projection plane.
///
/// The orthographic globe uses the unit disc with `+x` east and `+y` north of
/// the center. The equirectangular map uses `x = lon / 180` and `y = lat / 90`,
/// both in `[-1, 1]`. Terminal cell aspect correction belongs to the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPoint {
    pub x: f64,
    pub y: f64,
}

/// Orthographic projection of the sphere as seen from infinitely far above one
/// center point.
///
/// Forward and inverse formulas follow Snyder, *Map Projections: A Working
/// Manual*, USGS Professional Paper 1395 (1987), equations 20-3 through 20-5,
/// 20-14 and 20-15, on a unit sphere.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orthographic {
    center: LonLat,
    sin_lat0: f64,
    cos_lat0: f64,
}

impl Orthographic {
    /// Centers the globe. The latitude is clamped to `[-90, 90]` and the
    /// longitude is wrapped to `[-180, 180)`, so rotation input can be applied
    /// without separate range checks.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidGeometry::NonFinite`] for NaN or infinite input.
    pub fn new(center_lon_deg: f64, center_lat_deg: f64) -> Result<Self, InvalidGeometry> {
        if !center_lon_deg.is_finite() || !center_lat_deg.is_finite() {
            return Err(InvalidGeometry::NonFinite);
        }
        let center = LonLat::new(center_lon_deg, center_lat_deg.clamp(-90.0, 90.0))?;
        let (sin_lat0, cos_lat0) = center.lat.to_radians().sin_cos();
        Ok(Self {
            center,
            sin_lat0,
            cos_lat0,
        })
    }

    /// The normalized center.
    #[must_use]
    pub const fn center(&self) -> LonLat {
        self.center
    }

    /// Cosine of the angular distance from the center:
    /// `sin(lat0) sin(lat) + cos(lat0) cos(lat) cos(lon - lon0)`.
    /// A negative value is on the far side of the globe.
    #[must_use]
    pub fn cos_c(&self, point: LonLat) -> f64 {
        let (sin_lat, cos_lat) = point.lat.to_radians().sin_cos();
        let cos_dlon = (point.lon - self.center.lon).to_radians().cos();
        self.sin_lat0 * sin_lat + self.cos_lat0 * cos_lat * cos_dlon
    }

    /// Whether the point is on the visible hemisphere. The horizon counts as
    /// visible, with a `1e-12` allowance so that rounding in `cos c` does not
    /// hide a point that lies on it.
    #[must_use]
    pub fn is_visible(&self, point: LonLat) -> bool {
        self.cos_c(point) >= -HORIZON_TOLERANCE
    }

    /// Projects a visible point into the unit disc, or returns `None` for a
    /// point on the far side.
    #[must_use]
    pub fn project(&self, point: LonLat) -> Option<MapPoint> {
        self.is_visible(point).then(|| self.plane(point))
    }

    fn plane(&self, point: LonLat) -> MapPoint {
        let (sin_lat, cos_lat) = point.lat.to_radians().sin_cos();
        let (sin_dlon, cos_dlon) = (point.lon - self.center.lon).to_radians().sin_cos();
        MapPoint {
            x: cos_lat * sin_dlon,
            y: self.cos_lat0 * sin_lat - self.sin_lat0 * cos_lat * cos_dlon,
        }
    }

    /// Maps a unit-disc point back to the sphere. The disc origin returns the
    /// center; a point outside the disc, beyond a `1e-12` rounding allowance,
    /// returns `None`. At a polar center, every longitude on the disc edge is
    /// recovered; at the pole itself the longitude is the center longitude.
    #[must_use]
    pub fn inverse(&self, point: MapPoint) -> Option<LonLat> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let rho = point.x.hypot(point.y);
        if rho > 1.0 + DISC_EDGE_TOLERANCE {
            return None;
        }
        if rho == 0.0 {
            return Some(self.center);
        }
        let sin_c = rho.min(1.0);
        let cos_c = (1.0 - sin_c * sin_c).max(0.0).sqrt();
        let lat = (cos_c * self.sin_lat0 + point.y * sin_c * self.cos_lat0 / rho)
            .clamp(-1.0, 1.0)
            .asin();
        let dlon =
            (point.x * sin_c).atan2(rho * self.cos_lat0 * cos_c - point.y * self.sin_lat0 * sin_c);
        Some(LonLat {
            lon: normalize_longitude(self.center.lon + dlon.to_degrees()),
            lat: lat.to_degrees().clamp(-90.0, 90.0),
        })
    }

    /// Returns the visible part of the shorter great-circle arc from `start`
    /// to `end`, in the same direction, or `Ok(None)` when none of it is
    /// visible.
    ///
    /// An arc shorter than 180 degrees crosses the horizon great circle at
    /// most once, so one sign change of `cos c` is located by a fixed number of
    /// bisection steps along the arc. The returned crossing is the last point
    /// found on the visible side.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidGeometry::AntipodalArc`] when the endpoints are
    /// antipodal and so define no unique arc.
    pub fn clip_arc(
        &self,
        start: LonLat,
        end: LonLat,
    ) -> Result<Option<(LonLat, LonLat)>, InvalidGeometry> {
        let arc = Arc::new(start, end)?;
        let start_visible = self.is_visible(start);
        let end_visible = self.is_visible(end);
        Ok(match (start_visible, end_visible) {
            (true, true) => Some((start, end)),
            (false, false) => None,
            (true, false) => Some((start, self.horizon_crossing(&arc, 0.0, 1.0))),
            (false, true) => Some((self.horizon_crossing(&arc, 1.0, 0.0), end)),
        })
    }

    fn horizon_crossing(&self, arc: &Arc, visible_t: f64, hidden_t: f64) -> LonLat {
        let center = self.center.unit_vector();
        let mut visible = visible_t;
        let mut hidden = hidden_t;
        for _ in 0..HORIZON_BISECTION_STEPS {
            let middle = f64::midpoint(visible, hidden);
            // Strict here, so the reported crossing stays visible under the
            // horizon allowance after conversion back to degrees.
            if dot(center, arc.at(middle)) >= 0.0 {
                visible = middle;
            } else {
                hidden = middle;
            }
        }
        LonLat::from_vector(arc.at(visible))
    }
}

/// A great-circle arc shorter than 180 degrees, parameterized by spherical
/// linear interpolation.
struct Arc {
    start: [f64; 3],
    end: [f64; 3],
    angle: f64,
}

impl Arc {
    fn new(start: LonLat, end: LonLat) -> Result<Self, InvalidGeometry> {
        let start = start.unit_vector();
        let end = end.unit_vector();
        let cos_angle = dot(start, end);
        if cos_angle <= ANTIPODAL_DOT {
            return Err(InvalidGeometry::AntipodalArc);
        }
        Ok(Self {
            start,
            end,
            angle: cos_angle.clamp(-1.0, 1.0).acos(),
        })
    }

    fn at(&self, t: f64) -> [f64; 3] {
        let sin_angle = self.angle.sin();
        if sin_angle < 1e-15 {
            return self.start;
        }
        let weight_start = ((1.0 - t) * self.angle).sin() / sin_angle;
        let weight_end = (t * self.angle).sin() / sin_angle;
        [
            weight_start * self.start[0] + weight_end * self.end[0],
            weight_start * self.start[1] + weight_end * self.end[1],
            weight_start * self.start[2] + weight_end * self.end[2],
        ]
    }
}

/// Projects a point onto the equirectangular world map, `x = lon / 180` and
/// `y = lat / 90`.
#[must_use]
pub fn equirectangular(point: LonLat) -> MapPoint {
    MapPoint {
        x: point.lon / 180.0,
        y: point.lat / 90.0,
    }
}

/// An equirectangular segment, split in two when it crosses the antimeridian.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MapSegment {
    Whole([MapPoint; 2]),
    /// The first piece ends on one map edge and the second begins on the
    /// opposite edge at the same latitude.
    Split([MapPoint; 2], [MapPoint; 2]),
}

/// Projects a segment onto the equirectangular map. A longitude step larger
/// than 180 degrees is read as the shorter path across the antimeridian and is
/// split there, with the crossing latitude linearly interpolated in
/// longitude. A step of exactly 180 degrees is not split. The segment is a
/// straight line on the map, suited to densely sampled source geometry.
#[must_use]
pub fn equirectangular_segment(start: LonLat, end: LonLat) -> MapSegment {
    let step = end.lon - start.lon;
    if step.abs() <= 180.0 {
        return MapSegment::Whole([equirectangular(start), equirectangular(end)]);
    }
    let (end_unwrapped, edge) = if step > 180.0 {
        (end.lon - 360.0, -180.0)
    } else {
        (end.lon + 360.0, 180.0)
    };
    let t = (edge - start.lon) / (end_unwrapped - start.lon);
    let crossing_y = (start.lat + t * (end.lat - start.lat)) / 90.0;
    MapSegment::Split(
        [
            equirectangular(start),
            MapPoint {
                x: edge / 180.0,
                y: crossing_y,
            },
        ],
        [
            MapPoint {
                x: -edge / 180.0,
                y: crossing_y,
            },
            equirectangular(end),
        ],
    )
}

/// One explicit instant on the UT time scale, stored as a Julian day.
///
/// The solar formulas are defined in Terrestrial Time; this module uses UT
/// directly. The difference, about 69 seconds in 2026, moves the Sun by well
/// under the accuracy stated on [`SolarPosition`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UtcInstant {
    julian_day: f64,
}

impl UtcInstant {
    /// Converts Unix seconds, which exclude leap seconds.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidGeometry::InstantOutOfRange`] outside Julian day 0
    /// through the end of the year 9999.
    pub fn from_unix_seconds(seconds: i64) -> Result<Self, InvalidGeometry> {
        if !(MIN_UNIX_SECONDS..MAX_UNIX_SECONDS).contains(&seconds) {
            return Err(InvalidGeometry::InstantOutOfRange);
        }
        let days = i32::try_from(seconds.div_euclid(SECONDS_PER_DAY))
            .map_err(|_| InvalidGeometry::InstantOutOfRange)?;
        let second_of_day = u32::try_from(seconds.rem_euclid(SECONDS_PER_DAY))
            .map_err(|_| InvalidGeometry::InstantOutOfRange)?;
        Ok(Self {
            julian_day: UNIX_EPOCH_JULIAN_DAY
                + f64::from(days)
                + f64::from(second_of_day) / 86_400.0,
        })
    }

    /// Accepts a Julian day on the UT scale.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidGeometry::NonFinite`] for NaN or infinite input and
    /// [`InvalidGeometry::InstantOutOfRange`] outside Julian day 0 through the
    /// end of the year 9999.
    pub fn from_julian_day(julian_day: f64) -> Result<Self, InvalidGeometry> {
        if !julian_day.is_finite() {
            return Err(InvalidGeometry::NonFinite);
        }
        if !(MIN_JULIAN_DAY..MAX_JULIAN_DAY).contains(&julian_day) {
            return Err(InvalidGeometry::InstantOutOfRange);
        }
        Ok(Self { julian_day })
    }

    /// The Julian day.
    #[must_use]
    pub const fn julian_day(self) -> f64 {
        self.julian_day
    }

    /// Hours since 00:00 UT of the same day, in `[0, 24)`.
    #[must_use]
    pub fn ut_hours(self) -> f64 {
        (self.julian_day + 0.5).rem_euclid(1.0) * 24.0
    }
}

/// Geometric sunlight: the Sun's center above or below the geometric horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daylight {
    Day,
    Night,
}

/// Named twilight bands by solar zenith angle. This is a separate
/// classification from [`Daylight`]; it describes sky geometry, not weather or
/// reception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyPhase {
    /// Zenith at most 90 degrees.
    Day,
    /// Zenith above 90 and at most 96 degrees.
    CivilTwilight,
    /// Zenith above 96 and at most 102 degrees.
    NauticalTwilight,
    /// Zenith above 102 and at most 108 degrees.
    AstronomicalTwilight,
    /// Zenith above 108 degrees.
    Night,
}

/// Apparent solar position for one instant.
///
/// Computed with the NOAA Solar Calculator formulas, which follow Meeus,
/// *Astronomical Algorithms*. NOAA states that its sunrise and sunset times are
/// accurate to about one minute for locations within 72 degrees of the equator,
/// and that the calculations are most reliable for the years 1800 to 2100.
/// Accuracy degrades toward the poles and outside that period. The horizon is
/// geometric: refraction, the solar radius, observer elevation, and parallax
/// are not applied, so the geometric terminator differs from the conventional
/// 90.833 degree sunrise zenith.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolarPosition {
    declination_deg: f64,
    equation_of_time_min: f64,
    subsolar: LonLat,
}

impl SolarPosition {
    /// Computes the solar position for the instant.
    #[must_use]
    pub fn at(instant: UtcInstant) -> Self {
        let t = (instant.julian_day - J2000_JULIAN_DAY) / 36_525.0;
        let mean_longitude = (280.466_46 + t * (36_000.769_83 + t * 0.000_303_2)).rem_euclid(360.0);
        let mean_anomaly = (357.529_11 + t * (35_999.050_29 - 0.000_153_7 * t)).to_radians();
        let eccentricity = 0.016_708_634 - t * (0.000_042_037 + 0.000_000_126_7 * t);
        let center = mean_anomaly.sin() * (1.914_602 - t * (0.004_817 + 0.000_014 * t))
            + (2.0 * mean_anomaly).sin() * (0.019_993 - 0.000_101 * t)
            + (3.0 * mean_anomaly).sin() * 0.000_289;
        let true_longitude = mean_longitude + center;
        let node = (125.04 - 1_934.136 * t).to_radians();
        let apparent_longitude = (true_longitude - 0.005_69 - 0.004_78 * node.sin()).to_radians();
        let mean_obliquity =
            23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
        let obliquity = (mean_obliquity + 0.002_56 * node.cos()).to_radians();
        let declination = (obliquity.sin() * apparent_longitude.sin()).asin();

        let half_tan = (obliquity / 2.0).tan();
        let y = half_tan * half_tan;
        let l0 = mean_longitude.to_radians();
        let equation_of_time_rad = y * (2.0 * l0).sin() - 2.0 * eccentricity * mean_anomaly.sin()
            + 4.0 * eccentricity * y * mean_anomaly.sin() * (2.0 * l0).cos()
            - 0.5 * y * y * (4.0 * l0).sin()
            - 1.25 * eccentricity * eccentricity * (2.0 * mean_anomaly).sin();
        let equation_of_time_min = 4.0 * equation_of_time_rad.to_degrees();

        let declination_deg = declination.to_degrees();
        let subsolar_lon = -15.0 * (instant.ut_hours() - 12.0 + equation_of_time_min / 60.0);
        Self {
            declination_deg,
            equation_of_time_min,
            subsolar: LonLat {
                lon: normalize_longitude(subsolar_lon),
                lat: declination_deg,
            },
        }
    }

    /// Apparent solar declination in degrees.
    #[must_use]
    pub const fn declination_deg(&self) -> f64 {
        self.declination_deg
    }

    /// Equation of time in minutes: apparent minus mean solar time.
    #[must_use]
    pub const fn equation_of_time_min(&self) -> f64 {
        self.equation_of_time_min
    }

    /// The point where the Sun is at the zenith.
    #[must_use]
    pub const fn subsolar_point(&self) -> LonLat {
        self.subsolar
    }

    /// Geometric solar zenith angle at a point, in `[0, 180]` degrees.
    #[must_use]
    pub fn zenith_deg(&self, point: LonLat) -> f64 {
        self.subsolar.angular_distance(point)
    }

    /// Geometric day or night: night when the zenith exceeds 90 degrees.
    #[must_use]
    pub fn daylight(&self, point: LonLat) -> Daylight {
        if self.zenith_deg(point) > GEOMETRIC_HORIZON_ZENITH_DEG {
            Daylight::Night
        } else {
            Daylight::Day
        }
    }

    /// Named twilight band at a point.
    #[must_use]
    pub fn sky_phase(&self, point: LonLat) -> SkyPhase {
        let zenith = self.zenith_deg(point);
        if zenith <= GEOMETRIC_HORIZON_ZENITH_DEG {
            SkyPhase::Day
        } else if zenith <= CIVIL_TWILIGHT_ZENITH_DEG {
            SkyPhase::CivilTwilight
        } else if zenith <= NAUTICAL_TWILIGHT_ZENITH_DEG {
            SkyPhase::NauticalTwilight
        } else if zenith <= ASTRONOMICAL_TWILIGHT_ZENITH_DEG {
            SkyPhase::AstronomicalTwilight
        } else {
            SkyPhase::Night
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), InvalidGeometry>;

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} differs from {expected} by more than {tolerance}"
        );
    }

    fn assert_same_point(actual: LonLat, expected: LonLat, tolerance: f64) {
        assert_close(actual.lat(), expected.lat(), tolerance);
        let lon_error = normalize_longitude(actual.lon() - expected.lon()).abs();
        assert!(
            lon_error <= tolerance || expected.lat().abs() >= 90.0 - tolerance,
            "{actual:?} differs from {expected:?}"
        );
    }

    /// Days from 1970-01-01 in the proleptic Gregorian calendar, after Howard
    /// Hinnant's `days_from_civil`.
    fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month_index = (month + 9) % 12;
        let day_of_year = (153 * month_index + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    fn unix(date: (i64, i64, i64), time: (i64, i64, i64)) -> i64 {
        days_from_civil(date.0, date.1, date.2) * SECONDS_PER_DAY
            + time.0 * 3_600
            + time.1 * 60
            + time.2
    }

    #[test]
    fn instants_convert_without_a_clock() -> TestResult {
        let j2000 = UtcInstant::from_unix_seconds(unix((2000, 1, 1), (12, 0, 0)))?;
        assert_close(j2000.julian_day(), J2000_JULIAN_DAY, 1e-9);
        assert_close(j2000.ut_hours(), 12.0, 1e-9);
        let before_epoch = UtcInstant::from_unix_seconds(-1)?;
        assert_close(before_epoch.ut_hours(), 24.0 - 1.0 / 3_600.0, 1e-6);
        assert_eq!(
            UtcInstant::from_unix_seconds(MAX_UNIX_SECONDS),
            Err(InvalidGeometry::InstantOutOfRange)
        );
        assert_eq!(
            UtcInstant::from_unix_seconds(i64::MIN),
            Err(InvalidGeometry::InstantOutOfRange)
        );
        assert!(UtcInstant::from_unix_seconds(MIN_UNIX_SECONDS).is_ok());
        assert_eq!(
            UtcInstant::from_julian_day(f64::NAN),
            Err(InvalidGeometry::NonFinite)
        );
        assert_eq!(
            UtcInstant::from_julian_day(-0.5),
            Err(InvalidGeometry::InstantOutOfRange)
        );
        Ok(())
    }

    /// Meeus, Astronomical Algorithms, 2nd ed., example 25.a: 1992-10-13 0h TD,
    /// apparent declination -7 degrees 47' 06" (-7.78507). The instant is used
    /// as UT, about one minute early, which moves declination by under 0.001
    /// degree.
    #[test]
    fn meeus_example_25a_declination() -> TestResult {
        let sun = SolarPosition::at(UtcInstant::from_julian_day(2_448_908.5)?);
        assert_close(sun.declination_deg(), -7.785_07, 0.01);
        Ok(())
    }

    /// Meeus example 28.a: 1992-10-13 0h TD, equation of time +13m 42.6s.
    #[test]
    fn meeus_example_28a_equation_of_time() -> TestResult {
        let sun = SolarPosition::at(UtcInstant::from_julian_day(2_448_908.5)?);
        assert_close(sun.equation_of_time_min(), 13.0 + 42.6 / 60.0, 0.5);
        Ok(())
    }

    /// NREL SPA (Reda and Andreas, NREL/TP-560-34302) example: 2003-10-17
    /// 12:30:30 local time at UTC-7, latitude 39.742476, longitude -105.1786.
    /// Geocentric declination -9.31434 and topocentric zenith before
    /// refraction 50.12795 degrees. The tolerance covers NOAA versus SPA,
    /// the ignored 67 s delta T, and the 0.002 degree topocentric parallax.
    #[test]
    fn nrel_spa_reference_case() -> TestResult {
        let instant = UtcInstant::from_unix_seconds(unix((2003, 10, 17), (19, 30, 30)))?;
        let sun = SolarPosition::at(instant);
        assert_close(sun.declination_deg(), -9.314_34, 0.05);
        let golden = LonLat::new(-105.1786, 39.742_476)?;
        assert_close(sun.zenith_deg(golden), 50.127_95, 0.05);
        assert_eq!(sun.daylight(golden), Daylight::Day);
        Ok(())
    }

    #[test]
    fn subsolar_point_and_antipode() -> TestResult {
        for seconds in [0, 1_000_000_007, 1_789_000_000, -1_234_567_890] {
            let sun = SolarPosition::at(UtcInstant::from_unix_seconds(seconds)?);
            let subsolar = sun.subsolar_point();
            assert_close(sun.zenith_deg(subsolar), 0.0, 1e-6);
            assert_close(sun.zenith_deg(subsolar.antipode()), 180.0, 1e-6);
            assert_eq!(sun.sky_phase(subsolar.antipode()), SkyPhase::Night);
        }
        Ok(())
    }

    #[test]
    fn solstices_light_opposite_poles() -> TestResult {
        let north = LonLat::new(0.0, 90.0)?;
        let south = LonLat::new(0.0, -90.0)?;
        let june = SolarPosition::at(UtcInstant::from_unix_seconds(unix(
            (2026, 6, 21),
            (12, 0, 0),
        ))?);
        assert!(june.declination_deg() > 23.4);
        assert_eq!(june.daylight(north), Daylight::Day);
        assert_eq!(june.daylight(south), Daylight::Night);
        let december = SolarPosition::at(UtcInstant::from_unix_seconds(unix(
            (2026, 12, 21),
            (12, 0, 0),
        ))?);
        assert!(december.declination_deg() < -23.4);
        assert_eq!(december.daylight(north), Daylight::Night);
        assert_eq!(december.daylight(south), Daylight::Day);
        Ok(())
    }

    #[test]
    fn noon_subsolar_longitude_tracks_equation_of_time() -> TestResult {
        let sun = SolarPosition::at(UtcInstant::from_unix_seconds(unix(
            (2026, 9, 24),
            (12, 0, 0),
        ))?);
        assert_close(
            sun.subsolar_point().lon(),
            -15.0 * sun.equation_of_time_min() / 60.0,
            1e-9,
        );
        // Late September: the Sun transits Greenwich before noon UT.
        assert!(sun.subsolar_point().lon() < 0.0);
        Ok(())
    }

    #[test]
    fn twilight_bands_follow_zenith_thresholds() -> TestResult {
        let sun = SolarPosition::at(UtcInstant::from_unix_seconds(0)?);
        let subsolar = sun.subsolar_point();
        let cases = [
            (89.0, SkyPhase::Day, Daylight::Day),
            (93.0, SkyPhase::CivilTwilight, Daylight::Night),
            (99.0, SkyPhase::NauticalTwilight, Daylight::Night),
            (105.0, SkyPhase::AstronomicalTwilight, Daylight::Night),
            (111.0, SkyPhase::Night, Daylight::Night),
        ];
        // Walk north or south along the subsolar meridian.
        for (zenith, phase, daylight) in cases {
            let direction = if subsolar.lat() > 0.0 { -1.0 } else { 1.0 };
            let lat = subsolar.lat() + direction * zenith;
            let point = if lat.abs() <= 90.0 {
                LonLat::new(subsolar.lon(), lat)?
            } else {
                LonLat::new(subsolar.lon() + 180.0, direction * 180.0 - lat)?
            };
            assert_close(sun.zenith_deg(point), zenith, 1e-9);
            assert_eq!(sun.sky_phase(point), phase);
            assert_eq!(sun.daylight(point), daylight);
        }
        Ok(())
    }

    #[test]
    fn coordinates_validate_and_wrap() -> TestResult {
        assert_eq!(LonLat::new(f64::NAN, 0.0), Err(InvalidGeometry::NonFinite));
        assert_eq!(
            LonLat::new(0.0, 90.5),
            Err(InvalidGeometry::LatitudeOutOfRange)
        );
        assert_close(LonLat::new(180.0, 0.0)?.lon(), -180.0, 0.0);
        assert_close(LonLat::new(540.0, 0.0)?.lon(), -180.0, 0.0);
        assert_close(LonLat::new(-190.0, 0.0)?.lon(), 170.0, 1e-12);
        assert!(normalize_longitude(-1e-18) < 180.0);
        assert!(normalize_longitude(f64::INFINITY).is_nan());
        Ok(())
    }

    #[test]
    fn orthographic_center_horizon_and_far_side() -> TestResult {
        let globe = Orthographic::new(10.0, 20.0)?;
        let center = globe.project(LonLat::new(10.0, 20.0)?);
        assert_eq!(center, Some(MapPoint { x: 0.0, y: 0.0 }));
        let horizon = LonLat::new(100.0, 0.0)?;
        assert_close(globe.cos_c(horizon), 0.0, 1e-12);
        let edge = globe.project(horizon).ok_or(InvalidGeometry::NonFinite)?;
        assert_close(edge.x.hypot(edge.y), 1.0, 1e-12);
        let north_of_center = globe
            .project(LonLat::new(10.0, 90.0)?)
            .ok_or(InvalidGeometry::NonFinite)?;
        assert_close(north_of_center.x, 0.0, 1e-12);
        assert!(north_of_center.y > 0.0);
        let east = globe
            .project(LonLat::new(40.0, 20.0)?)
            .ok_or(InvalidGeometry::NonFinite)?;
        assert!(east.x > 0.0);
        assert_eq!(globe.project(LonLat::new(-170.0, -20.0)?), None);
        assert!(!globe.is_visible(LonLat::new(-120.0, -30.0)?));
        Ok(())
    }

    #[test]
    fn orthographic_inverse_round_trips() -> TestResult {
        for (lon0, lat0) in [
            (0.0, 0.0),
            (-73.5, 45.5),
            (179.0, -60.0),
            (0.0, 90.0),
            (0.0, -90.0),
        ] {
            let globe = Orthographic::new(lon0, lat0)?;
            for lat in (-89..=89).step_by(7) {
                for lon in (-180..180).step_by(11) {
                    let point = LonLat::new(f64::from(lon), f64::from(lat))?;
                    if let Some(plane) = globe.project(point) {
                        let back = globe.inverse(plane).ok_or(InvalidGeometry::NonFinite)?;
                        assert_same_point(back, point, 1e-9);
                    }
                }
            }
            assert_eq!(
                globe.inverse(MapPoint { x: 0.0, y: 0.0 }),
                Some(globe.center())
            );
            assert_eq!(globe.inverse(MapPoint { x: 0.8, y: 0.8 }), None);
            assert_eq!(
                globe.inverse(MapPoint {
                    x: f64::NAN,
                    y: 0.0
                }),
                None
            );
        }
        Ok(())
    }

    #[test]
    fn polar_centers_see_one_hemisphere() -> TestResult {
        let north = Orthographic::new(30.0, 90.0)?;
        let south = Orthographic::new(30.0, -90.0)?;
        for lon in (-180..180).step_by(15) {
            let lon = f64::from(lon);
            assert!(north.is_visible(LonLat::new(lon, 1.0)?));
            assert!(!north.is_visible(LonLat::new(lon, -1.0)?));
            assert!(south.is_visible(LonLat::new(lon, -1.0)?));
            assert_close(north.cos_c(LonLat::new(lon, 0.0)?), 0.0, 1e-12);
            let edge = north
                .project(LonLat::new(lon, 0.0)?)
                .ok_or(InvalidGeometry::NonFinite)?;
            assert_close(edge.x.hypot(edge.y), 1.0, 1e-12);
        }
        let pole = north
            .inverse(MapPoint { x: 0.0, y: 0.0 })
            .ok_or(InvalidGeometry::NonFinite)?;
        assert_close(pole.lat(), 90.0, 0.0);
        Ok(())
    }

    #[test]
    fn center_is_clamped_and_wrapped() -> TestResult {
        let globe = Orthographic::new(190.0, 95.0)?;
        assert_close(globe.center().lon(), -170.0, 1e-12);
        assert_close(globe.center().lat(), 90.0, 0.0);
        let wrapped = Orthographic::new(-540.0, -120.0)?;
        assert_close(wrapped.center().lon(), -180.0, 0.0);
        assert_close(wrapped.center().lat(), -90.0, 0.0);
        let same = Orthographic::new(-190.0, 10.0)?;
        let reference = Orthographic::new(170.0, 10.0)?;
        let point = LonLat::new(-175.0, 25.0)?;
        assert_eq!(same.project(point), reference.project(point));
        assert_eq!(
            Orthographic::new(f64::INFINITY, 0.0),
            Err(InvalidGeometry::NonFinite)
        );
        Ok(())
    }

    #[test]
    fn arcs_clip_at_the_horizon() -> TestResult {
        let globe = Orthographic::new(0.0, 0.0)?;
        let inside = LonLat::new(60.0, 0.0)?;
        let outside = LonLat::new(120.0, 0.0)?;
        let (start, crossing) = globe
            .clip_arc(inside, outside)?
            .ok_or(InvalidGeometry::NonFinite)?;
        assert_eq!(start, inside);
        assert_close(crossing.lon(), 90.0, 1e-9);
        assert!(globe.is_visible(crossing));
        let (crossing, end) = globe
            .clip_arc(outside, inside)?
            .ok_or(InvalidGeometry::NonFinite)?;
        assert_eq!(end, inside);
        assert_close(crossing.lon(), 90.0, 1e-9);

        // A tilted arc crosses where cos c changes sign.
        let tilted = globe
            .clip_arc(LonLat::new(80.0, 40.0)?, LonLat::new(100.0, -30.0)?)?
            .ok_or(InvalidGeometry::NonFinite)?;
        assert_close(globe.cos_c(tilted.1), 0.0, 1e-12);

        assert_eq!(
            globe.clip_arc(LonLat::new(100.0, 0.0)?, LonLat::new(170.0, 10.0)?)?,
            None
        );
        let both = (LonLat::new(-30.0, 10.0)?, LonLat::new(30.0, -10.0)?);
        assert_eq!(globe.clip_arc(both.0, both.1)?, Some(both));
        assert_eq!(
            globe.clip_arc(LonLat::new(10.0, 20.0)?, LonLat::new(-170.0, -20.0)?),
            Err(InvalidGeometry::AntipodalArc)
        );
        Ok(())
    }

    #[test]
    fn antimeridian_segments_split() -> TestResult {
        let eastward =
            equirectangular_segment(LonLat::new(179.0, 10.0)?, LonLat::new(-179.0, 20.0)?);
        let MapSegment::Split(first, second) = eastward else {
            return Err(InvalidGeometry::NonFinite);
        };
        assert_close(first[0].x, 179.0 / 180.0, 1e-12);
        assert_close(first[1].x, 1.0, 0.0);
        assert_close(first[1].y, 15.0 / 90.0, 1e-12);
        assert_close(second[0].x, -1.0, 0.0);
        assert_close(second[0].y, 15.0 / 90.0, 1e-12);
        assert_close(second[1].x, -179.0 / 180.0, 1e-12);

        let westward =
            equirectangular_segment(LonLat::new(-179.0, 20.0)?, LonLat::new(179.0, 10.0)?);
        let MapSegment::Split(first, second) = westward else {
            return Err(InvalidGeometry::NonFinite);
        };
        assert_close(first[1].x, -1.0, 0.0);
        assert_close(second[0].x, 1.0, 0.0);
        assert_close(first[1].y, 15.0 / 90.0, 1e-12);

        let short = equirectangular_segment(LonLat::new(-10.0, 0.0)?, LonLat::new(10.0, 5.0)?);
        assert!(matches!(short, MapSegment::Whole(_)));
        Ok(())
    }
}
