use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::business::BusinessValue;
use super::{
    Date, DateTime, Time, YMDuration, change_month, combine, compare_checked, dt_between_dates,
    dt_between_datetimes, valid_date,
};

pub trait FinancialValue: BusinessValue {
    fn local(self) -> NaiveDateTime;
    fn with_local(self, local: NaiveDateTime) -> Result<Self, String>;
    fn elapsed(self, other: Self, include_time: bool) -> Result<Decimal, String>;
}
impl FinancialValue for Date {
    fn local(self) -> NaiveDateTime {
        self.date.and_hms_opt(0, 0, 0).unwrap()
    }
    fn with_local(self, local: NaiveDateTime) -> Result<Self, String> {
        valid_date(Self {
            date: local.date(),
            ..self
        })
    }
    fn elapsed(self, other: Self, include_time: bool) -> Result<Decimal, String> {
        if include_time {
            return Err("includeTime requires DateTime".into());
        }
        compare_checked(&self, &other, chrono::Local::now())?;
        divide(dt_between_dates(self, other)?.0, Decimal::from(86_400))
    }
}
impl FinancialValue for DateTime {
    fn local(self) -> NaiveDateTime {
        self.datetime
    }
    fn with_local(self, local: NaiveDateTime) -> Result<Self, String> {
        combine(
            Date {
                date: local.date(),
                zone: self.zone,
            },
            Time {
                time: local.time(),
                zone: self.zone,
            },
        )
    }
    fn elapsed(self, other: Self, include_time: bool) -> Result<Decimal, String> {
        compare_checked(&self, &other, chrono::Local::now())?;
        if include_time {
            divide(dt_between_datetimes(self, other)?.0, Decimal::from(86_400))
        } else {
            Ok(Decimal::from(
                other
                    .datetime
                    .date()
                    .signed_duration_since(self.datetime.date())
                    .num_days(),
            ))
        }
    }
}
fn divide(a: Decimal, b: Decimal) -> Result<Decimal, String> {
    a.checked_div(b)
        .ok_or_else(|| "day count overflow or zero denominator".into())
}
fn leap(year: i32) -> bool {
    NaiveDate::from_ymd_opt(year, 2, 29).is_some()
}
fn last_february(date: NaiveDate) -> bool {
    date.month() == 2 && date.day() == if leap(date.year()) { 29 } else { 28 }
}
fn thirty_360(a: NaiveDate, b: NaiveDate, european: bool) -> Decimal {
    let (mut d1, mut d2) = (a.day() as i32, b.day() as i32);
    let mut m2 = b.month() as i32;
    let mut y2 = b.year();
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        if last_february(a) || d1 == 31 {
            d1 = 30;
        }
        if last_february(b) && last_february(a) {
            d2 = 30;
        } else if d2 == 31 {
            if d1 >= 30 {
                d2 = 30;
            } else {
                d2 = 1;
                m2 += 1;
                if m2 == 13 {
                    m2 = 1;
                    y2 += 1;
                }
            }
        }
    }
    Decimal::from((y2 - a.year()) * 360 + (m2 - a.month() as i32) * 30 + d2 - d1)
}
fn calendar_difference<T: FinancialValue>(
    a: T,
    b: T,
    years: bool,
    include_time: bool,
) -> Result<Decimal, String> {
    let negative = a.elapsed(b, include_time)?.is_sign_negative();
    let (start, end) = if negative { (b, a) } else { (a, b) };
    let (start_date, end_date) = (start.day().date, end.day().date);
    let start_time = start.local().time();
    let end_time = end.local().time();
    let mut periods = (end_date.year() - start_date.year()) * if years { 1 } else { 12 };
    if !years {
        periods += end_date.month() as i32 - start_date.month() as i32;
    }
    let stride = if years { 12 } else { 1 };
    let anchor = |n: i32| change_month(start_date, YMDuration(Decimal::from(n * stride)));
    while anchor(periods)? > end_date
        || include_time && anchor(periods)? == end_date && start_time > end_time
    {
        periods -= 1;
    }
    let at = anchor(periods)?;
    let after = anchor(periods + 1)?;
    let elapsed = Decimal::from(end_date.signed_duration_since(at).num_days());
    let fractional = if include_time {
        let nanos = end_time
            .signed_duration_since(start_time)
            .num_nanoseconds()
            .ok_or("time difference overflow")?;
        elapsed
            .checked_add(divide(
                Decimal::from(nanos),
                Decimal::from(86_400_000_000_000_i64),
            )?)
            .ok_or("day count overflow")?
    } else {
        elapsed
    };
    let span = Decimal::from(after.signed_duration_since(at).num_days());
    let result = Decimal::from(periods)
        .checked_add(divide(fractional, span)?)
        .ok_or("day count overflow")?;
    Ok(if negative { -result } else { result })
}
pub trait DifferenceOption {
    fn resolve(self) -> (String, bool);
}
impl DifferenceOption for String {
    fn resolve(self) -> (String, bool) {
        (self, false)
    }
}
impl DifferenceOption for bool {
    fn resolve(self) -> (String, bool) {
        ("calendar".into(), self)
    }
}
pub fn difference_three<T: FinancialValue, O: DifferenceOption>(
    a: T,
    b: T,
    name: &str,
    option: O,
) -> Result<Decimal, String> {
    let (basis, include_time) = option.resolve();
    difference(a, b, name, &basis, include_time)
}
pub fn difference<T: FinancialValue>(
    a: T,
    b: T,
    name: &str,
    basis: &str,
    include_time: bool,
) -> Result<Decimal, String> {
    let days = a.elapsed(b, include_time)?;
    if name == "daysBetween" {
        return Ok(days);
    }
    let years = name == "yearsBetween";
    let ratio = match basis {
        "calendar" => return calendar_difference(a, b, years, include_time),
        "actual/365" => divide(
            days.checked_mul(Decimal::from(if years { 1 } else { 12 }))
                .ok_or("day count overflow")?,
            Decimal::from(365),
        )?,
        "actual/360" => divide(
            days.checked_mul(Decimal::from(if years { 1 } else { 12 }))
                .ok_or("day count overflow")?,
            Decimal::from(360),
        )?,
        "actual/actual" => {
            let sign = if days.is_sign_negative() {
                -Decimal::ONE
            } else {
                Decimal::ONE
            };
            let (start, end) = if days.is_sign_negative() {
                (b, a)
            } else {
                (a, b)
            };
            let mut cursor = start;
            let mut fraction = Decimal::ZERO;
            while cursor.elapsed(end, include_time)? > Decimal::ZERO {
                let boundary = NaiveDate::from_ymd_opt(cursor.local().year() + 1, 1, 1)
                    .ok_or("date out of range")?
                    .and_hms_opt(0, 0, 0)
                    .ok_or("date out of range")?;
                let boundary = cursor.with_local(boundary)?;
                let until = if boundary.elapsed(end, include_time)? > Decimal::ZERO {
                    boundary
                } else {
                    end
                };
                fraction = fraction
                    .checked_add(divide(
                        cursor.elapsed(until, include_time)?,
                        Decimal::from(if leap(cursor.local().year()) {
                            366
                        } else {
                            365
                        }),
                    )?)
                    .ok_or("day count overflow")?;
                cursor = until;
            }
            fraction * sign
        }
        "30/360" | "30E/360" => {
            let negative = days.is_sign_negative();
            let (start, end) = if negative { (b, a) } else { (a, b) };
            let mut convention = thirty_360(start.day().date, end.day().date, basis == "30E/360");
            if include_time {
                let extra = Decimal::from(
                    end.local()
                        .time()
                        .signed_duration_since(start.local().time())
                        .num_nanoseconds()
                        .ok_or("time difference overflow")?,
                );
                convention = convention
                    .checked_add(divide(extra, Decimal::from(86_400_000_000_000_i64))?)
                    .ok_or("day count overflow")?;
            }
            let signed = if negative { -convention } else { convention };
            divide(
                signed
                    .checked_mul(Decimal::from(if years { 1 } else { 12 }))
                    .ok_or("day count overflow")?,
                Decimal::from(360),
            )?
        }
        _ => return Err(format!("invalid day-count basis: {basis}")),
    };
    if years || matches!(basis, "actual/365" | "actual/360" | "30/360" | "30E/360") {
        Ok(ratio)
    } else {
        ratio
            .checked_mul(Decimal::from(12))
            .ok_or_else(|| "day count overflow".into())
    }
}
fn fiscal<T: BusinessValue>(
    value: T,
    month: u32,
    day: u32,
    quarter: bool,
) -> Result<String, String> {
    let date = value.day().date;
    let start_this_year =
        NaiveDate::from_ymd_opt(date.year(), month, day).ok_or("invalid financial year basis")?;
    let first_year = if date < start_this_year {
        date.year() - 1
    } else {
        date.year()
    };
    let start = NaiveDate::from_ymd_opt(first_year, month, day).ok_or("date out of range")?;
    let end_year = if month == 1 && day == 1 {
        first_year
    } else {
        first_year + 1
    };
    if !quarter {
        return Ok(format!("FY{end_year}"));
    }
    let mut quarter_number = 1;
    for index in 1..4 {
        let boundary = change_month(start, YMDuration(Decimal::from(index * 3)))?;
        if date < boundary {
            break;
        }
        quarter_number += 1;
    }
    Ok(format!("FY{end_year}Q{quarter_number}"))
}
pub trait FiscalBasis {
    fn year<T: BusinessValue>(self, value: T, quarter: bool) -> Result<String, String>;
}
impl FiscalBasis for Decimal {
    fn year<T: BusinessValue>(self, value: T, quarter: bool) -> Result<String, String> {
        financial_year_month(value, self, quarter)
    }
}
impl FiscalBasis for String {
    fn year<T: BusinessValue>(self, value: T, quarter: bool) -> Result<String, String> {
        financial_year_named(value, &self, quarter)
    }
}
pub fn financial_year<T: BusinessValue, B: FiscalBasis>(
    value: T,
    basis: B,
    quarter: bool,
) -> Result<String, String> {
    basis.year(value, quarter)
}
pub fn financial_year_month<T: BusinessValue>(
    value: T,
    month: Decimal,
    quarter: bool,
) -> Result<String, String> {
    let month = month
        .to_u32()
        .filter(|m| (1..=12).contains(m) && Decimal::from(*m) == month)
        .ok_or("financial year month must be 1 through 12")?;
    fiscal(value, month, 1, quarter)
}
pub fn financial_year_named<T: BusinessValue>(
    value: T,
    basis: &str,
    quarter: bool,
) -> Result<String, String> {
    let (month, day) = match basis {
        "AU" => (7, 1),
        "UK" => (4, 6),
        "US" => (10, 1),
        "IN" | "JP" | "CA" | "NZ" => (4, 1),
        _ => return Err(format!("invalid financial year basis: {basis}")),
    };
    fiscal(value, month, day, quarter)
}
