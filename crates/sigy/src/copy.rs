//! Human copy shared by the CLI, explorer and panel. JSON keeps exact values.

use sigy_service::domain::money::Usd;

/// Group decimal digits. `10000` renders as `10,000`.
#[must_use]
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().rev().enumerate() {
        if index > 0 && index.is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out.chars().rev().collect()
}

/// `0 stations`, `1 station`, `2 stations`, with grouped digits.
#[must_use]
pub fn station_count(count: u64) -> String {
    count_label(count, "station", "stations")
}

/// `1 candidate`, `2 candidates`.
#[must_use]
pub fn count_label(count: u64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

/// Shared cache size: `1 of 10,000 stations`.
#[must_use]
pub fn cache_span(shown: u64, total: u64) -> String {
    format!("{} of {}", grouped(shown), station_count(total))
}

/// Compact exact USD. At least two decimal places, and every nonzero sub-cent digit.
///
/// Parsing uses the accounting decimal. There is no floating conversion. An
/// unparseable value is returned unchanged so a display failure cannot look like zero.
#[must_use]
pub fn compact_usd(exact: &str) -> String {
    let Ok(value) = exact.parse::<Usd>() else {
        return exact.to_owned();
    };
    let micros = value.micros();
    let whole = micros / Usd::MICROS_PER_DOLLAR;
    let fraction = micros % Usd::MICROS_PER_DOLLAR;
    let mut digits = format!("{fraction:06}");
    while digits.len() > 2 && digits.ends_with('0') {
        digits.pop();
    }
    format!("${whole}.{digits}")
}

/// Capability sentence from the snapshot field. A budget amount is not this fact.
#[must_use]
pub fn paid_processing(dispatch_available: bool) -> &'static str {
    if dispatch_available {
        "Paid processing: reported available without a qualified provider."
    } else {
        "Paid processing: unavailable."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_group_digits_and_use_one_plural() {
        assert_eq!(station_count(0), "0 stations");
        assert_eq!(station_count(1), "1 station");
        assert_eq!(station_count(2), "2 stations");
        assert_eq!(cache_span(1, 10_000), "1 of 10,000 stations");
        assert_eq!(cache_span(0, 0), "0 of 0 stations");
        assert_eq!(count_label(1, "candidate", "candidates"), "1 candidate");
        assert_eq!(count_label(2, "candidate", "candidates"), "2 candidates");
    }

    #[test]
    fn compact_usd_keeps_sub_cent_digits_and_never_rounds_to_zero() {
        for (exact, shown) in [
            ("0", "$0.00"),
            ("0.000000", "$0.00"),
            ("0.000001", "$0.000001"),
            ("0.004999", "$0.004999"),
            ("0.005000", "$0.005"),
            ("0.009999", "$0.009999"),
            ("0.010000", "$0.01"),
            ("3.200000", "$3.20"),
            ("3.200001", "$3.200001"),
            ("9223372036854.775807", "$9223372036854.775807"),
        ] {
            assert_eq!(compact_usd(exact), shown, "{exact}");
            assert_ne!(compact_usd("0.000001"), "$0.00");
        }
        assert_eq!(compact_usd("not-a-number"), "not-a-number");
    }
}
