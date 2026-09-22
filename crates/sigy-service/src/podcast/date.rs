//! RSS `pubDate` instants. Named zones other than GMT, UT, UTC, and Z are not guessed.

/// Milliseconds since the Unix epoch, when the value is a recognized RSS date.
#[must_use]
pub(crate) fn parse_pub_date(value: &str) -> Option<i64> {
    let mut parts = value.split_whitespace();
    let first = parts.next()?;
    let (day_token, month_token, year_token, time_token, zone_token) =
        if let Some(day_name) = first.strip_suffix(',') {
            if !is_weekday(day_name) {
                return None;
            }
            (
                parts.next()?,
                parts.next()?,
                parts.next()?,
                parts.next()?,
                parts.next(),
            )
        } else {
            (
                first,
                parts.next()?,
                parts.next()?,
                parts.next()?,
                parts.next(),
            )
        };
    if parts.next().is_some() {
        return None;
    }
    let day: u32 = day_token.parse().ok()?;
    let month = month_index(month_token)?;
    let year: i32 = year_token.parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let (hour, minute, second, inline_offset) = split_time(time_token)?;
    let offset_minutes = match (inline_offset, zone_token) {
        (Some(offset), None) => offset,
        (None, Some(zone)) => zone_offset(zone)?,
        _ => return None,
    };
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let local = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600)?
        .checked_add(i64::from(minute) * 60)?
        .checked_add(i64::from(second))?;
    local
        .checked_sub(i64::from(offset_minutes) * 60)?
        .checked_mul(1_000)
}

fn is_weekday(value: &str) -> bool {
    matches!(value, "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat" | "Sun")
}

fn month_index(value: &str) -> Option<u32> {
    Some(match value {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    })
}

fn split_time(value: &str) -> Option<(u32, u32, u32, Option<i32>)> {
    let (clock, inline) = if value.len() > 8 && matches!(value.as_bytes().get(8), Some(b'+' | b'-'))
    {
        (&value[..8], Some(&value[8..]))
    } else {
        (value, None)
    };
    let mut clock = clock.split(':');
    let hour: u32 = clock.next()?.parse().ok()?;
    let minute: u32 = clock.next()?.parse().ok()?;
    let second: u32 = clock.next()?.parse().ok()?;
    if clock.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, inline.and_then(numeric_offset)))
}

fn zone_offset(value: &str) -> Option<i32> {
    match value {
        "GMT" | "UT" | "UTC" | "Z" => Some(0),
        other => numeric_offset(other),
    }
}

fn numeric_offset(value: &str) -> Option<i32> {
    let (sign, digits) = if let Some(rest) = value.strip_prefix('+') {
        (1, rest)
    } else {
        let rest = value.strip_prefix('-')?;
        (-1, rest)
    };
    let (hours, minutes) = if let Some((hours, minutes)) = digits.split_once(':') {
        (hours, minutes)
    } else if digits.len() == 4 {
        (&digits[..2], &digits[2..])
    } else {
        return None;
    };
    let hours: i32 = hours.parse().ok()?;
    let minutes: i32 = minutes.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 60 + minutes))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Howard Hinnant's `days_from_civil`, valid for the Gregorian calendar.
fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = u32::try_from(shifted - era * 400).ok()?;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(i64::from(era) * 146_097 + i64::from(day_of_era) - 719_468)
}

#[cfg(test)]
mod tests {
    use super::parse_pub_date;

    #[test]
    fn epoch_and_numeric_zones_match() {
        assert_eq!(parse_pub_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_pub_date("01 Jan 1970 01:00:00 +0100"), Some(0));
        assert_eq!(parse_pub_date("01 Jan 1970 00:00:00 -0000"), Some(0));
        assert!(parse_pub_date("29 Feb 2023 00:00:00 GMT").is_none());
        assert!(parse_pub_date("29 Feb 2024 00:00:00 GMT").is_some());
        assert!(parse_pub_date("01 Jan 1970 00:00:00 EST").is_none());
    }
}
