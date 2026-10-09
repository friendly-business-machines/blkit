use std::{fmt, str::FromStr};

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A signed days/time duration stored as a decimal number of seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DTDuration(pub Decimal);

/// A signed years/months duration stored as a decimal number of months.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct YMDuration(pub Decimal);

fn parse_units(text: &str, months: bool) -> Result<Decimal, String> {
    if !text.is_ascii() {
        return Err("duration must be ASCII ISO text".into());
    }
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let Some(text) = text.strip_prefix(['p', 'P']) else {
        return Err("duration must start with P".into());
    };
    let mut total = Decimal::ZERO;
    let mut time = false;
    let mut saw_unit = false;
    let mut time_unit = false;
    let mut last = -1;
    let bytes = text.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        if !months && !time && bytes[pos].eq_ignore_ascii_case(&b't') {
            time = true;
            last = 0;
            pos += 1;
            continue;
        }
        let start = pos;
        while pos < bytes.len() && (bytes[pos].is_ascii_digit() || bytes[pos] == b'.') {
            pos += 1;
        }
        if start == pos || pos == bytes.len() {
            return Err(format!("invalid duration: {text}"));
        }
        let value = Decimal::from_str_exact(&text[start..pos])
            .map_err(|_| format!("invalid duration number: {text}"))?;
        let unit = bytes[pos].to_ascii_uppercase();
        pos += 1;
        let (rank, multiplier) = match (months, time, unit) {
            (true, false, b'Y') => (1, 12),
            (true, false, b'M') => (2, 1),
            (false, false, b'D') => (1, 86400),
            (false, true, b'H') => (1, 3600),
            (false, true, b'M') => (2, 60),
            (false, true, b'S') => (3, 1),
            _ => return Err(format!("invalid duration unit: {text}")),
        };
        if rank <= last {
            return Err(format!("duplicate or out-of-order duration unit: {text}"));
        }
        last = rank;
        saw_unit = true;
        time_unit |= time;
        total = value
            .checked_mul(Decimal::from(multiplier))
            .and_then(|value| total.checked_add(value))
            .ok_or("duration overflow")?;
    }
    if !saw_unit || (time && !time_unit) {
        return Err(format!("invalid duration: {text}"));
    }
    Ok(if negative { -total } else { total })
}

fn parts(value: Decimal, units: &[(i64, char)], prefix: &str) -> String {
    if value.is_zero() {
        return format!("{prefix}0{}", units.last().unwrap().1);
    }
    let mut output = String::from(if value.is_sign_negative() { "-" } else { "" });
    output.push('P');
    let mut remaining = value.abs();
    for (index, &(seconds, symbol)) in units.iter().enumerate() {
        if symbol == 'H' && !output.ends_with('T') {
            output.push('T');
        }
        let component = if index == units.len() - 1 {
            remaining
        } else {
            let base = Decimal::from(seconds);
            let remainder = remaining % base;
            let component = (remaining - remainder) / base;
            remaining = remainder;
            component
        };
        if !component.is_zero() {
            output.push_str(&component.normalize().to_string());
            output.push(symbol);
        }
        if symbol == 'D' && !remaining.is_zero() {
            output.push('T');
        }
    }
    if output.ends_with('T') {
        output.pop();
    }
    output
}

macro_rules! checked_arithmetic {
    ($ty:ty) => {
        impl $ty {
            pub fn checked_add(self, other: Self) -> Result<Self, String> {
                self.0
                    .checked_add(other.0)
                    .map(Self)
                    .ok_or("duration overflow".into())
            }
            pub fn checked_sub(self, other: Self) -> Result<Self, String> {
                self.0
                    .checked_sub(other.0)
                    .map(Self)
                    .ok_or("duration overflow".into())
            }
            pub fn checked_mul(self, factor: Decimal) -> Result<Self, String> {
                self.0
                    .checked_mul(factor)
                    .map(Self)
                    .ok_or("duration overflow".into())
            }
            pub fn checked_div(self, divisor: Decimal) -> Result<Self, String> {
                if divisor.is_zero() {
                    return Err("division by zero".into());
                }
                self.0
                    .checked_div(divisor)
                    .map(Self)
                    .ok_or("duration overflow".into())
            }
            pub fn abs(self) -> Self {
                Self(self.0.abs())
            }
            pub fn is_negative(self) -> bool {
                !self.0.is_zero() && self.0.is_sign_negative()
            }
            pub fn checked_round(self, step: Self, mode: &str) -> Result<Self, String> {
                if step.0 <= Decimal::ZERO {
                    return Err("duration rounding requires a positive step".into());
                }
                let rem = self.0 % step.0;
                let base = self.0.checked_sub(rem).ok_or("duration overflow")?;
                if rem.is_zero() {
                    return Ok(Self(base));
                }
                let halfway = rem.abs().cmp(&(step.0 - rem.abs()));
                let away = match mode {
                    "roundUp" => true,
                    "roundDown" => false,
                    "round" | "roundHalfUp" => halfway.is_ge(),
                    "roundHalfDown" => halfway.is_gt(),
                    "roundHalfEven" => {
                        halfway.is_gt()
                            || halfway.is_eq()
                                && (base
                                    .checked_div(step.0)
                                    .ok_or("duration rounding overflow")?
                                    % Decimal::from(2))
                                    != Decimal::ZERO
                    }
                    _ => return Err("invalid duration rounding mode".into()),
                };
                let result = if away {
                    base.checked_add(if rem.is_sign_negative() {
                        -step.0
                    } else {
                        step.0
                    })
                } else {
                    Some(base)
                };
                result.map(Self).ok_or_else(|| "duration overflow".into())
            }
        }
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(D::Error::custom)
            }
        }
    };
}
checked_arithmetic!(DTDuration);
checked_arithmetic!(YMDuration);

impl DTDuration {
    pub fn total_seconds(self) -> Decimal {
        self.0
    }
    pub fn total_minutes(self) -> Decimal {
        self.0 / Decimal::from(60)
    }
    pub fn total_hours(self) -> Decimal {
        self.0 / Decimal::from(3600)
    }
    pub fn total_days(self) -> Decimal {
        self.0 / Decimal::from(86400)
    }
    pub fn days(self) -> Decimal {
        (self.0 - self.0 % Decimal::from(86400)) / Decimal::from(86400)
    }
    pub fn hours(self) -> Decimal {
        let value = self.0 % Decimal::from(86400);
        (value - value % Decimal::from(3600)) / Decimal::from(3600)
    }
    pub fn minutes(self) -> Decimal {
        let value = self.0 % Decimal::from(3600);
        (value - value % Decimal::from(60)) / Decimal::from(60)
    }
    pub fn seconds(self) -> Decimal {
        self.0 % Decimal::from(60)
    }
}

impl YMDuration {
    pub fn total_months(self) -> Decimal {
        self.0
    }
    pub fn total_years(self) -> Decimal {
        self.0 / Decimal::from(12)
    }
    pub fn years(self) -> Decimal {
        (self.0 - self.0 % Decimal::from(12)) / Decimal::from(12)
    }
    pub fn months(self) -> Decimal {
        self.0 % Decimal::from(12)
    }
}

impl FromStr for DTDuration {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_units(text, false).map(Self)
    }
}

impl FromStr for YMDuration {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_units(text, true).map(Self)
    }
}

impl fmt::Display for DTDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            parts(
                self.0,
                &[(86400, 'D'), (3600, 'H'), (60, 'M'), (1, 'S')],
                "PT"
            )
        )
    }
}

impl fmt::Display for YMDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", parts(self.0, &[(12, 'Y'), (1, 'M')], "P"))
    }
}
