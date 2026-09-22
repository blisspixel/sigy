//! Civil schedule resolution. One IANA zone decides the instant. Gaps are misses.

use jiff::{
    Timestamp, ToSpan,
    civil::{Date, DateTime, Weekday},
    tz::{AmbiguousOffset, Offset, TimeZone},
};

use crate::{Error, Result};

pub(crate) const MAX_SCHEDULES: i64 = 256;
pub(crate) const MAX_OCCURRENCES: i64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Clock {
    pub hour: i8,
    pub minute: i8,
    pub second: i8,
    pub duration_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cadence {
    Once(Date),
    Daily,
    Weekly(i8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    Window {
        civil_date: String,
        start_ms: i64,
        end_ms: i64,
        offset_seconds: i32,
    },
    /// The civil time does not exist. `transition_ms` is when the clock jumps over it.
    Spring {
        civil_date: String,
        transition_ms: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Timing {
    Future,
    Open,
    Ended,
}

impl Slot {
    pub(crate) fn civil_date(&self) -> &str {
        match self {
            Self::Window { civil_date, .. } | Self::Spring { civil_date, .. } => civil_date,
        }
    }

    pub(crate) fn timing(&self, now_ms: i64) -> Timing {
        match *self {
            Self::Window {
                start_ms, end_ms, ..
            } => {
                if now_ms < start_ms {
                    Timing::Future
                } else if now_ms < end_ms {
                    Timing::Open
                } else {
                    Timing::Ended
                }
            }
            Self::Spring { transition_ms, .. } => {
                if now_ms < transition_ms {
                    Timing::Future
                } else {
                    Timing::Ended
                }
            }
        }
    }
}

pub(crate) fn canonical_zone(name: &str) -> Result<(TimeZone, String)> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ')
    {
        return Err(Error::InvalidInput("IANA time zone"));
    }
    let zone = TimeZone::get(name).map_err(|_| Error::InvalidInput("IANA time zone"))?;
    let canonical = zone
        .iana_name()
        .filter(|canonical| canonical.len() <= 64)
        .map(str::to_owned)
        .ok_or(Error::InvalidInput("IANA time zone"))?;
    Ok((zone, canonical))
}

pub(crate) fn zone(name: &str) -> Result<TimeZone> {
    TimeZone::get(name).map_err(|_| Error::InvalidInput("IANA time zone"))
}

pub(crate) fn parse_date(value: &str) -> Result<Date> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(Error::InvalidInput("civil date"));
    }
    let year = parse_i16(&value[0..4])?;
    let month = parse_i8(&value[5..7])?;
    let day = parse_i8(&value[8..10])?;
    Date::new(year, month, day).map_err(|_| Error::InvalidInput("civil date"))
}

pub(crate) fn parse_clock(value: &str) -> Result<(i8, i8, i8)> {
    let bytes = value.as_bytes();
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return Err(Error::InvalidInput("civil time"));
    }
    let hour = parse_i8(&value[0..2])?;
    let minute = parse_i8(&value[3..5])?;
    let second = parse_i8(&value[6..8])?;
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..60).contains(&second) {
        return Err(Error::InvalidInput("civil time"));
    }
    Ok((hour, minute, second))
}

pub(crate) fn parse_weekday(value: &str) -> Result<i8> {
    Ok(match value {
        "mon" | "monday" => 1,
        "tue" | "tuesday" => 2,
        "wed" | "wednesday" => 3,
        "thu" | "thursday" => 4,
        "fri" | "friday" => 5,
        "sat" | "saturday" => 6,
        "sun" | "sunday" => 7,
        _ => return Err(Error::InvalidInput("weekday")),
    })
}

pub(crate) fn weekday_name(weekday: i8) -> Result<&'static str> {
    Ok(match weekday {
        1 => "mon",
        2 => "tue",
        3 => "wed",
        4 => "thu",
        5 => "fri",
        6 => "sat",
        7 => "sun",
        _ => return Err(Error::InvalidInput("weekday")),
    })
}

pub(crate) fn civil_today(zone: &TimeZone, now_ms: i64) -> Result<Date> {
    let zoned = timestamp(now_ms)?.to_zoned(zone.clone());
    Ok(zoned.datetime().date())
}

pub(crate) fn add_days(date: Date, days: i64) -> Result<Date> {
    date.checked_add(days.days())
        .map_err(|_| Error::InvalidInput("civil date"))
}

pub(crate) fn is_weekday(date: Date, weekday: i8) -> bool {
    weekday_number(date.weekday()) == weekday
}

pub(crate) fn resolve(zone: &TimeZone, date: Date, clock: &Clock) -> Result<Slot> {
    let civil = date.at(clock.hour, clock.minute, clock.second, 0);
    let ambiguous = zone.to_ambiguous_zoned(civil);
    match ambiguous.offset() {
        AmbiguousOffset::Unambiguous { offset } => window(zone, civil, offset, clock),
        AmbiguousOffset::Fold { before, .. } => window(zone, civil, before, clock),
        AmbiguousOffset::Gap { .. } => Ok(Slot::Spring {
            civil_date: date.to_string(),
            transition_ms: gap_transition_ms(zone, civil)?,
        }),
    }
}

fn window(zone: &TimeZone, civil: DateTime, offset: Offset, clock: &Clock) -> Result<Slot> {
    let start = offset
        .to_timestamp(civil)
        .map_err(|_| Error::InvalidInput("civil time"))?;
    let start_ms = start.as_millisecond();
    let duration_ms = clock
        .duration_seconds
        .checked_mul(1000)
        .ok_or(Error::InvalidInput("recording duration or byte ceiling"))?;
    let end_ms = start_ms
        .checked_add(duration_ms)
        .ok_or(Error::InvalidInput("recording duration or byte ceiling"))?;
    let _ = zone;
    Ok(Slot::Window {
        civil_date: civil.date().to_string(),
        start_ms,
        end_ms,
        offset_seconds: offset.seconds(),
    })
}

fn gap_transition_ms(zone: &TimeZone, civil: DateTime) -> Result<i64> {
    let noon = civil
        .date()
        .yesterday()
        .map_err(|_| Error::InvalidInput("civil time"))?
        .at(12, 0, 0, 0);
    let mut cursor = unambiguous(zone, noon)?;
    for _ in 0..8 {
        let Some(at) = next_transition(zone, cursor) else {
            break;
        };
        if gap_contains(zone, at, civil)? {
            return Ok(at.as_millisecond());
        }
        cursor = at;
    }
    Err(Error::InvalidInput("daylight saving transition"))
}

fn gap_contains(zone: &TimeZone, at: Timestamp, civil: DateTime) -> Result<bool> {
    let before = at
        .checked_sub(1.nanoseconds())
        .map_err(|_| Error::InvalidInput("daylight saving transition"))?
        .to_zoned(zone.clone())
        .datetime();
    let after = at.to_zoned(zone.clone()).datetime();
    Ok(after > before && civil >= before && civil < after)
}

fn next_transition(zone: &TimeZone, after: Timestamp) -> Option<Timestamp> {
    zone.following(after)
        .next()
        .map(|transition| transition.timestamp())
}

fn unambiguous(zone: &TimeZone, civil: DateTime) -> Result<Timestamp> {
    let offset = match zone.to_ambiguous_zoned(civil).offset() {
        AmbiguousOffset::Unambiguous { offset } | AmbiguousOffset::Fold { before: offset, .. } => {
            offset
        }
        AmbiguousOffset::Gap { .. } => return Err(Error::InvalidInput("civil time")),
    };
    offset
        .to_timestamp(civil)
        .map_err(|_| Error::InvalidInput("civil time"))
}

fn timestamp(now_ms: i64) -> Result<Timestamp> {
    Timestamp::from_millisecond(now_ms).map_err(|_| Error::InvalidInput("clock range"))
}

fn weekday_number(day: Weekday) -> i8 {
    match day {
        Weekday::Monday => 1,
        Weekday::Tuesday => 2,
        Weekday::Wednesday => 3,
        Weekday::Thursday => 4,
        Weekday::Friday => 5,
        Weekday::Saturday => 6,
        Weekday::Sunday => 7,
    }
}

fn parse_i16(value: &str) -> Result<i16> {
    value.parse().map_err(|_| Error::InvalidInput("civil date"))
}

fn parse_i8(value: &str) -> Result<i8> {
    value.parse().map_err(|_| Error::InvalidInput("civil time"))
}

#[cfg(test)]
mod tests {
    use super::{Clock, Slot, Timing, resolve, zone};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn clock(hour: i8, minute: i8, seconds: i64) -> Clock {
        Clock {
            hour,
            minute,
            second: 0,
            duration_seconds: seconds,
        }
    }

    #[test]
    fn spring_forward_civil_time_has_no_instant() -> TestResult {
        let zone = zone("America/New_York")?;
        let date = jiff::civil::date(2026, 3, 8);
        let slot = resolve(&zone, date, &clock(2, 30, 60))?;
        let Slot::Spring { transition_ms, .. } = slot else {
            return Err("spring gap was admitted".into());
        };
        assert_eq!(transition_ms, 1_772_953_200_000);
        assert_eq!(slot.timing(transition_ms), Timing::Ended);
        assert_eq!(slot.timing(transition_ms - 1), Timing::Future);
        Ok(())
    }

    #[test]
    fn fall_back_uses_the_earlier_offset_once() -> TestResult {
        let zone = zone("America/New_York")?;
        let date = jiff::civil::date(2026, 11, 1);
        let slot = resolve(&zone, date, &clock(1, 30, 900))?;
        let Slot::Window {
            start_ms,
            end_ms,
            offset_seconds,
            ..
        } = slot
        else {
            return Err("fold was treated as a gap".into());
        };
        assert_eq!(offset_seconds, -4 * 3600);
        assert_eq!(start_ms, 1_793_511_000_000);
        assert_eq!(end_ms, start_ms + 900_000);
        assert_ne!(start_ms, 1_793_514_600_000);
        Ok(())
    }
}
