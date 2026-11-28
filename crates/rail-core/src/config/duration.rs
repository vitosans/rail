//! The duration strings accepted by `general.poll_interval`.

use std::fmt;
use std::time::Duration;

use serde::{Deserializer, de};

pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Duration, D::Error> {
    input.deserialize_str(DurationVisitor)
}

struct DurationVisitor;

impl de::Visitor<'_> for DurationVisitor {
    type Value = Duration;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a duration")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Duration, E> {
        parse(value).ok_or_else(|| E::invalid_value(de::Unexpected::Str(value), &self))
    }
}

fn parse(mut input: &str) -> Option<Duration> {
    if input == "0" {
        return Some(Duration::ZERO);
    }
    let mut total = Duration::ZERO;
    loop {
        let starts_unit = |c: char| c.is_ascii_alphabetic() || c == 'µ';
        let (number, rest) = input.split_at(input.find(starts_unit)?);
        let end = rest.find(|c| !starts_unit(c)).unwrap_or(rest.len());
        let (unit, rest) = rest.split_at(end);
        total = Unit::named(unit)?.add_component(total, number)?;
        input = rest.trim_start();
        if input.is_empty() {
            return Some(total);
        }
    }
}

/// Whitespace, including between digits, is accepted in existing configurations.
fn digits(input: &str) -> Option<(u64, usize)> {
    let mut value = 0_u64;
    let mut count = 0;
    for digit in input.chars().filter(|c| !c.is_whitespace()) {
        value = value
            .checked_mul(10)?
            .checked_add(u64::from(digit.to_digit(10)?))?;
        count += 1;
    }
    (count > 0).then_some((value, count))
}

#[derive(Clone, Copy)]
enum Unit {
    Nanos(u64),
    Seconds(u64),
}

impl Unit {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "ns" | "nsec" | "nanos" => Self::Nanos(1),
            "us" | "µs" | "usec" => Self::Nanos(1_000),
            "ms" | "msec" | "millis" => Self::Nanos(1_000_000),
            "s" | "sec" | "secs" | "second" | "seconds" => Self::Seconds(1),
            "m" | "min" | "mins" | "minute" | "minutes" => Self::Seconds(60),
            "h" | "hr" | "hrs" | "hour" | "hours" => Self::Seconds(3_600),
            "d" | "day" | "days" => Self::Seconds(86_400),
            "w" | "wk" | "wks" | "week" | "weeks" => Self::Seconds(604_800),
            "M" | "month" | "months" => Self::Seconds(2_630_016),
            "y" | "yr" | "yrs" | "year" | "years" => Self::Seconds(31_557_600),
            _ => return None,
        })
    }

    fn add_component(self, total: Duration, number: &str) -> Option<Duration> {
        let Some((whole, fraction)) = number.split_once('.') else {
            return self.add_scaled(total, digits(number)?.0, 1);
        };
        let total = self.add_scaled(total, digits(whole)?.0, 1)?;

        // The existing grammar accepts fractions through minutes at ns precision.
        // Fractions of larger units must resolve to whole seconds.
        let unit = match self {
            Self::Nanos(1) => return None,
            Self::Seconds(scale @ (1 | 60)) => Self::Nanos(scale * 1_000_000_000),
            other => other,
        };
        let (value, places) = digits(fraction)?;
        let denominator = 10_u64.checked_pow(u32::try_from(places).ok()?)?;
        unit.add_scaled(total, value, denominator)
    }

    fn add_scaled(self, total: Duration, value: u64, divisor: u64) -> Option<Duration> {
        let (Self::Nanos(scale) | Self::Seconds(scale)) = self;
        let scaled = value.checked_mul(scale)?;
        if !scaled.is_multiple_of(divisor) {
            return None;
        }
        let amount = scaled / divisor;
        match self {
            Self::Seconds(_) => total.checked_add(Duration::from_secs(amount)),
            Self::Nanos(_) => {
                // Keep the checked limit on each component before carrying full seconds.
                let nanos = amount.checked_add(u64::from(total.subsec_nanos()))?;
                Duration::from_secs(total.as_secs()).checked_add(Duration::from_nanos(nanos))
            }
        }
    }
}

#[cfg(test)]
mod tests;
