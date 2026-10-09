use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::{Calendar, CalendarPoint, Date, DateTime, Time, combine, valid_date};

pub trait BusinessValue: Copy {
    fn day(self) -> Date;
    fn with_day(self, date: NaiveDate) -> Result<Self, String>;
}
impl BusinessValue for Date {
    fn day(self) -> Date {
        self
    }
    fn with_day(self, date: NaiveDate) -> Result<Self, String> {
        valid_date(Date { date, ..self })
    }
}
impl BusinessValue for DateTime {
    fn day(self) -> Date {
        Date {
            date: self.datetime.date(),
            zone: self.zone,
        }
    }
    fn with_day(self, date: NaiveDate) -> Result<Self, String> {
        combine(
            Date {
                date,
                zone: self.zone,
            },
            Time {
                time: self.datetime.time(),
                zone: self.zone,
            },
        )
    }
}

fn step(date: NaiveDate, n: i64) -> Result<NaiveDate, String> {
    date.checked_add_signed(Duration::days(n))
        .ok_or_else(|| "date outside supported range".into())
}
fn weekday(date: NaiveDate) -> bool {
    date.weekday().number_from_monday() <= 5
}
fn calendar_contains(calendar: &Calendar, value: Date, strict: bool) -> Result<bool, String> {
    if strict {
        let point = CalendarPoint::Date(value);
        if point.compare(calendar.valid_from())?.is_lt()
            || point.compare(calendar.valid_to())?.is_gt()
        {
            return Err(
                "bl.CalendarRangeError: date is outside the calendar validity range".into(),
            );
        }
    }
    calendar.contains(value)
}
fn business(date: Date, calendar: Option<&Calendar>, strict: bool) -> Result<bool, String> {
    let holiday = match calendar {
        Some(calendar) => calendar_contains(calendar, date, strict)?,
        None => false,
    };
    Ok(weekday(date.date) && !holiday)
}
pub fn predicate<T: BusinessValue>(
    value: T,
    name: &str,
    calendar: Option<&Calendar>,
) -> Result<bool, String> {
    let date = value.day();
    Ok(match name {
        "isWeekday" => weekday(date.date),
        "isWeekend" => !weekday(date.date),
        "isPublicHoliday" => calendar_contains(calendar.ok_or("missing calendar")?, date, false)?,
        "isBusinessDay" => business(date, calendar, false)?,
        _ => return Err(format!("unknown date predicate: {name}")),
    })
}
fn month_start(date: NaiveDate) -> Result<NaiveDate, String> {
    date.with_day(1).ok_or_else(|| "invalid month".into())
}
fn next_month(date: NaiveDate) -> Result<NaiveDate, String> {
    let (y, m) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).ok_or_else(|| "date outside supported range".into())
}
fn dow(number: Decimal) -> Result<Weekday, String> {
    match number.to_u32().filter(|day| Decimal::from(*day) == number) {
        Some(1) => Ok(Weekday::Mon),
        Some(2) => Ok(Weekday::Tue),
        Some(3) => Ok(Weekday::Wed),
        Some(4) => Ok(Weekday::Thu),
        Some(5) => Ok(Weekday::Fri),
        Some(6) => Ok(Weekday::Sat),
        Some(7) => Ok(Weekday::Sun),
        _ => Err("day of week must be an integer from 1 (Monday) to 7 (Sunday)".into()),
    }
}
fn day_count(number: Decimal) -> Result<i64, String> {
    number
        .to_i64()
        .filter(|n| Decimal::from(*n) == number)
        .ok_or_else(|| "day count must be an integer".into())
}
fn week_in_month(date: NaiveDate, day: Weekday, ordinal: i64) -> Result<NaiveDate, String> {
    if ordinal == 0 {
        return Err("week occurrence cannot be zero".into());
    }
    let first = month_start(date)?;
    let last = step(next_month(date)?, -1)?;
    let found = if ordinal > 0 {
        let delta = (7 + i64::from(day.number_from_monday())
            - i64::from(first.weekday().number_from_monday()))
            % 7;
        step(
            first,
            delta
                + (ordinal - 1)
                    .checked_mul(7)
                    .ok_or("week occurrence overflow")?,
        )?
    } else {
        let delta = (7 + i64::from(last.weekday().number_from_monday())
            - i64::from(day.number_from_monday()))
            % 7;
        step(
            last,
            -delta
                + (ordinal + 1)
                    .checked_mul(7)
                    .ok_or("week occurrence overflow")?,
        )?
    };
    if found.month() != date.month() {
        return Err("week occurrence is outside the month".into());
    }
    Ok(found)
}
fn navigate(date: NaiveDate, day: Weekday, forward: bool) -> Result<NaiveDate, String> {
    let current = i64::from(date.weekday().number_from_monday());
    let target = i64::from(day.number_from_monday());
    let delta = if forward {
        (target - current).rem_euclid(7)
    } else {
        (current - target).rem_euclid(7)
    };
    step(
        date,
        if forward {
            if delta == 0 { 7 } else { delta }
        } else {
            -if delta == 0 { 7 } else { delta }
        },
    )
}
pub fn operation<T: BusinessValue>(
    value: T,
    name: &str,
    first: Option<Decimal>,
    second: Option<Decimal>,
    calendar: Option<&Calendar>,
    strict: bool,
) -> Result<T, String> {
    let current = value.day();
    let date = current.date;
    let selected = match name {
        "firstDayOfMonth" => month_start(date)?,
        "lastDayOfMonth" => step(next_month(date)?, -1)?,
        "lastDayOfPrevMonth" => step(month_start(date)?, -1)?,
        "firstDayOfNextMonth" => next_month(date)?,
        "firstDayOfWeekInMonth" => {
            week_in_month(date, dow(first.ok_or("missing day of week")?)?, 1)?
        }
        "lastDayOfWeekInMonth" => {
            week_in_month(date, dow(first.ok_or("missing day of week")?)?, -1)?
        }
        "nthDayOfWeekInMonth" => week_in_month(
            date,
            dow(second.ok_or("missing day of week")?)?,
            day_count(first.ok_or("missing occurrence")?)?,
        )?,
        "nextDayOfWeek" | "prevDayOfWeek" => navigate(
            date,
            dow(first.ok_or("missing day of week")?)?,
            name == "nextDayOfWeek",
        )?,
        "nextWeekday"
        | "prevWeekday"
        | "nextBusinessDay"
        | "prevBusinessDay"
        | "addBusinessDays"
        | "subtractBusinessDays" => {
            let forward = matches!(name, "nextWeekday" | "nextBusinessDay" | "addBusinessDays");
            let target = if matches!(name, "addBusinessDays" | "subtractBusinessDays") {
                day_count(first.ok_or("missing number of days")?)?
            } else {
                1
            };
            if target == 0 {
                return Ok(value);
            }
            let forward = if target < 0 { !forward } else { forward };
            let mut left = target.checked_abs().ok_or("day count overflow")?;
            let mut next = date;
            while left > 0 {
                next = step(next, if forward { 1 } else { -1 })?;
                let date_at = Date {
                    date: next,
                    zone: current.zone,
                };
                let available = if matches!(name, "nextWeekday" | "prevWeekday") {
                    weekday(next)
                } else {
                    business(date_at, calendar, strict)?
                };
                if available {
                    left -= 1;
                }
            }
            next
        }
        _ => return Err(format!("unknown date operation: {name}")),
    };
    value.with_day(selected)
}
pub fn count_between<T: BusinessValue>(
    a: T,
    b: T,
    calendar: Option<&Calendar>,
    strict: bool,
) -> Result<Decimal, String> {
    let (a, b) = (a.day(), b.day());
    super::compare_checked(&a, &b, chrono::Local::now())?;
    let (mut current, end) = if a.date <= b.date {
        (a.date, b.date)
    } else {
        (b.date, a.date)
    };
    let mut count = 0_u64;
    loop {
        if business(
            Date {
                date: current,
                zone: a.zone,
            },
            calendar,
            strict,
        )? {
            count += 1;
        }
        if current == end {
            break;
        }
        current = step(current, 1)?;
    }
    Ok(Decimal::from(count))
}
