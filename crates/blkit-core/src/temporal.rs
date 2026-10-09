//! Civil temporal values with explicit naive, fixed-offset, and IANA zone kinds.
#[path = "temporal/duration.rs"]
mod duration;
pub use duration::{DTDuration, YMDuration};
#[path = "temporal/business.rs"]
pub mod business;
#[path = "temporal/calendar.rs"]
mod calendar;
#[path = "temporal/financial.rs"]
pub mod financial;
#[allow(unused_imports)] // Build-script compilation does not use calendar values directly.
pub use calendar::{
    Calendar, CalendarEntry, CalendarPoint, CalendarRange, CalendarTarget, CalendarValue,
    entry_name,
};
use std::{cmp::Ordering, fmt, str::FromStr};

use chrono::{
    Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Timelike,
    Utc,
};
use chrono_tz::Tz;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Naive,
    Offset(FixedOffset),
    Iana(Tz),
}

#[derive(Clone, Copy, Debug)]
pub struct Date {
    date: NaiveDate,
    zone: Zone,
}

impl Date {
    pub fn date(self) -> NaiveDate {
        self.date
    }
    pub fn zone(self) -> Zone {
        self.zone
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Time {
    pub time: NaiveTime,
    pub zone: Zone,
}

#[derive(Clone, Copy, Debug)]
pub struct DateTime {
    datetime: NaiveDateTime,
    zone: Zone,
}

impl DateTime {
    pub fn datetime(self) -> NaiveDateTime {
        self.datetime
    }
    pub fn zone(self) -> Zone {
        self.zone
    }
}

fn parse_zone(text: &str) -> Result<Zone, String> {
    if text.is_empty() {
        return Ok(Zone::Naive);
    }
    if text == "Z" {
        return Ok(Zone::Offset(FixedOffset::east_opt(0).unwrap()));
    }
    if let Some(name) = text.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return name
            .parse::<Tz>()
            .map(Zone::Iana)
            .map_err(|_| format!("invalid timezone: {name}"));
    }
    let (sign, hours, minutes) = match text.as_bytes() {
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(|digit| digit.is_ascii_digit()) =>
        {
            (
                *sign,
                (h1 - b'0') * 10 + h2 - b'0',
                (m1 - b'0') * 10 + m2 - b'0',
            )
        }
        _ => return Err(format!("invalid offset: {text}")),
    };
    if minutes > 59 {
        return Err(format!("invalid offset: {text}"));
    }
    let seconds = (i32::from(hours) * 60 + i32::from(minutes)) * 60;
    let seconds = if sign == b'-' { -seconds } else { seconds };
    FixedOffset::east_opt(seconds)
        .map(Zone::Offset)
        .ok_or_else(|| format!("invalid offset: {text}"))
}

fn split_time_zone(text: &str) -> (&str, &str) {
    let boundary = text
        .char_indices()
        .find(|(i, ch)| *i >= 8 && matches!(ch, '+' | '-' | 'Z' | '['))
        .map_or(text.len(), |(i, _)| i);
    text.split_at(boundary)
}

fn parse_time(text: &str, end_of_day: bool) -> Result<NaiveTime, String> {
    if end_of_day && text == "24:00:00" {
        return Ok(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }
    if text.len() < 8
        || text.as_bytes()[2] != b':'
        || text.as_bytes()[5] != b':'
        || !text.as_bytes()[..2].iter().all(u8::is_ascii_digit)
    {
        return Err(format!("invalid time: {text}"));
    }
    let time = NaiveTime::parse_from_str(text, "%H:%M:%S%.f")
        .map_err(|_| format!("invalid time: {text}"))?;
    if time.nanosecond() >= 1_000_000_000 {
        return Err(format!("invalid leap second: {text}"));
    }
    Ok(time)
}

impl FromStr for Date {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (date, suffix) = text
            .get(..10)
            .zip(text.get(10..))
            .ok_or_else(|| format!("invalid date: {text}"))?;
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| format!("invalid date: {text}"))?;
        let zone = parse_zone(suffix)?;
        if let Zone::Iana(tz) = zone {
            let midnight = date.and_hms_opt(0, 0, 0).unwrap();
            if tz.from_local_datetime(&midnight).single().is_none() {
                return Err(format!("ambiguous or nonexistent local midnight: {text}"));
            }
        }
        Ok(Self { date, zone })
    }
}

impl FromStr for Time {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (time, suffix) = split_time_zone(text);
        Ok(Self {
            time: parse_time(time, true)?,
            zone: parse_zone(suffix)?,
        })
    }
}

impl FromStr for DateTime {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (date, time) = text
            .split_once('T')
            .ok_or_else(|| format!("invalid datetime: {text}"))?;
        if date.len() != 10 {
            return Err(format!("invalid datetime: {text}"));
        }
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| format!("invalid datetime: {text}"))?;
        let (time, suffix) = split_time_zone(time);
        let datetime = date.and_time(parse_time(time, false)?);
        let zone = parse_zone(suffix)?;
        if let Zone::Iana(tz) = zone
            && tz.from_local_datetime(&datetime).single().is_none()
        {
            return Err(format!("ambiguous or nonexistent local datetime: {text}"));
        }
        Ok(Self { datetime, zone })
    }
}

pub fn date_from_parts(year: Decimal, month: Decimal, day: Decimal) -> Result<Date, String> {
    let (year, month, day) = (
        year.to_i32()
            .filter(|value| (0..=9999).contains(value) && Decimal::from(*value) == year),
        month
            .to_u32()
            .filter(|value| Decimal::from(*value) == month),
        day.to_u32().filter(|value| Decimal::from(*value) == day),
    );
    let date = year
        .zip(month)
        .zip(day)
        .and_then(|((year, month), day)| NaiveDate::from_ymd_opt(year, month, day))
        .ok_or("invalid date components")?;
    Ok(Date {
        date,
        zone: Zone::Naive,
    })
}

pub fn time_from_parts(
    hour: Decimal,
    minute: Decimal,
    second: Decimal,
    offset: Option<DTDuration>,
) -> Result<Time, String> {
    let hour = hour
        .to_u32()
        .filter(|value| Decimal::from(*value) == hour)
        .ok_or("invalid hour")?;
    let minute = minute
        .to_u32()
        .filter(|value| Decimal::from(*value) == minute)
        .ok_or("invalid minute")?;
    let billion = Decimal::from(1_000_000_000);
    let seconds = second
        .checked_mul(billion)
        .and_then(|value| {
            value
                .to_u64()
                .filter(|integer| Decimal::from(*integer) == value)
        })
        .ok_or("invalid fractional second")?;
    if seconds >= 60_000_000_000 {
        return Err("invalid second".into());
    }
    let time = NaiveTime::from_hms_nano_opt(
        hour,
        minute,
        (seconds / 1_000_000_000) as u32,
        (seconds % 1_000_000_000) as u32,
    )
    .ok_or("invalid time components")?;
    let zone = match offset {
        Some(value) => Zone::Offset(fixed_offset(value)?),
        None => Zone::Naive,
    };
    Ok(Time { time, zone })
}

pub fn combine(date: Date, time: Time) -> Result<DateTime, String> {
    if date.zone != time.zone {
        return Err("datetime requires matching date/time zone kinds".into());
    }
    let datetime = date.date.and_time(time.time);
    if let Zone::Iana(zone) = date.zone
        && zone.from_local_datetime(&datetime).single().is_none()
    {
        return Err("ambiguous or nonexistent local datetime".into());
    }
    Ok(DateTime {
        datetime,
        zone: date.zone,
    })
}

pub fn extract_date(value: DateTime) -> Date {
    Date {
        date: value.datetime.date(),
        zone: value.zone,
    }
}

pub fn extract_time(value: DateTime) -> Time {
    Time {
        time: value.datetime.time(),
        zone: value.zone,
    }
}

pub fn today(clock: chrono::DateTime<Local>) -> Date {
    Date {
        date: clock.date_naive(),
        zone: Zone::Naive,
    }
}

pub fn now(clock: chrono::DateTime<Local>) -> DateTime {
    DateTime {
        datetime: clock.naive_local(),
        zone: Zone::Offset(clock.offset().fix()),
    }
}

pub trait TemporalParts {
    fn date(&self) -> Option<NaiveDate>;
    fn time(&self) -> Option<NaiveTime>;
    fn zone(&self) -> Zone;
}
impl TemporalParts for Date {
    fn date(&self) -> Option<NaiveDate> {
        Some(self.date)
    }
    fn time(&self) -> Option<NaiveTime> {
        None
    }
    fn zone(&self) -> Zone {
        self.zone
    }
}
impl TemporalParts for Time {
    fn date(&self) -> Option<NaiveDate> {
        None
    }
    fn time(&self) -> Option<NaiveTime> {
        Some(self.time)
    }
    fn zone(&self) -> Zone {
        self.zone
    }
}
impl TemporalParts for DateTime {
    fn date(&self) -> Option<NaiveDate> {
        Some(self.datetime.date())
    }
    fn time(&self) -> Option<NaiveTime> {
        Some(self.datetime.time())
    }
    fn zone(&self) -> Zone {
        self.zone
    }
}

pub fn number_property(value: &impl TemporalParts, field: &str) -> Result<Decimal, String> {
    let date = value.date();
    let time = value.time();
    let date = date.as_ref();
    let time = time.as_ref();
    let result = match field {
        "year" => date.map(|d| d.year() as i64),
        "month" => date.map(|d| d.month() as i64),
        "day" => date.map(|d| d.day() as i64),
        "dayOfYear" => date.map(|d| d.ordinal() as i64),
        "weekOfYear" => date.map(|d| ((d.ordinal() - 1) / 7 + 1) as i64),
        "isoWeekOfYear" => date.map(|d| d.iso_week().week() as i64),
        "quarter" => date.map(|d| ((d.month() - 1) / 3 + 1) as i64),
        "hour" => time.map(|t| t.hour() as i64),
        "minute" => time.map(|t| t.minute() as i64),
        "second" => {
            return time
                .map(|t| {
                    Decimal::from(t.second())
                        + Decimal::from(t.nanosecond()) / Decimal::from(1_000_000_000)
                })
                .ok_or_else(|| format!("invalid property: {field}"));
        }
        _ => None,
    };
    result
        .map(Decimal::from)
        .ok_or_else(|| format!("invalid property: {field}"))
}

pub fn text_property(value: &impl TemporalParts, field: &str) -> Result<String, String> {
    if field == "timezone" {
        return match value.zone() {
            Zone::Iana(zone) => Ok(zone.to_string()),
            _ => Err("no IANA timezone".into()),
        };
    }
    let date = value
        .date()
        .ok_or_else(|| format!("invalid property: {field}"))?;
    let text = match field {
        "dayName" => date.format("%A").to_string(),
        "dayNameShort" => date.format("%a").to_string(),
        "monthName" => date.format("%B").to_string(),
        "monthNameShort" => date.format("%b").to_string(),
        "isoYearWeek" => format!("{}W{}", date.iso_week().year(), date.iso_week().week()),
        "yearQuarter" => format!("{}Q{}", date.year(), (date.month() - 1) / 3 + 1),
        _ => return Err(format!("invalid property: {field}")),
    };
    Ok(text)
}

pub fn offset_property(
    value: &impl TemporalParts,
    clock: chrono::DateTime<Local>,
) -> Result<DTDuration, String> {
    let offset = match value.zone() {
        Zone::Naive => return Err("no offset on naive temporal value".into()),
        Zone::Offset(offset) => offset.local_minus_utc(),
        Zone::Iana(zone) => {
            let date = value
                .date()
                .unwrap_or_else(|| clock.with_timezone(&zone).date_naive());
            let datetime = date.and_time(
                value
                    .time()
                    .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
            );
            zone.from_local_datetime(&datetime)
                .single()
                .ok_or("ambiguous or nonexistent local time")?
                .offset()
                .fix()
                .local_minus_utc()
        }
    };
    Ok(DTDuration(Decimal::from(offset)))
}

fn fixed_offset(duration: DTDuration) -> Result<FixedOffset, String> {
    duration
        .0
        .to_i32()
        .filter(|value| Decimal::from(*value) == duration.0 && value % 60 == 0)
        .and_then(FixedOffset::east_opt)
        .ok_or_else(|| "invalid offset".into())
}

pub fn with_offset_datetime(value: DateTime, duration: DTDuration) -> Result<DateTime, String> {
    let offset = fixed_offset(duration)?;
    let utc = value
        .zone
        .instant(value.datetime)
        .ok_or("datetime has no resolvable instant")?;
    Ok(DateTime {
        datetime: utc.with_timezone(&offset).naive_local(),
        zone: Zone::Offset(offset),
    })
}

pub fn with_offset_time(
    value: Time,
    duration: DTDuration,
    clock: chrono::DateTime<Local>,
) -> Result<Time, String> {
    let offset = fixed_offset(duration)?;
    let date = match value.zone {
        Zone::Iana(zone) => clock.with_timezone(&zone).date_naive(),
        _ => clock.date_naive(),
    };
    let utc = value
        .zone
        .instant(date.and_time(value.time))
        .ok_or("time has no resolvable instant")?;
    Ok(Time {
        time: utc.with_timezone(&offset).time(),
        zone: Zone::Offset(offset),
    })
}

pub fn with_timezone(value: DateTime, name: &str) -> Result<DateTime, String> {
    let zone = name
        .parse::<Tz>()
        .map_err(|_| format!("invalid timezone: {name}"))?;
    let utc = value
        .zone
        .instant(value.datetime)
        .ok_or("datetime has no resolvable instant")?;
    Ok(DateTime {
        datetime: utc.with_timezone(&zone).naive_local(),
        zone: Zone::Iana(zone),
    })
}

pub trait StripZone: Sized {
    fn strip_zone(self, mode: &str) -> Self;
}
fn stripped(zone: Zone, mode: &str) -> Zone {
    match (zone, mode) {
        (Zone::Offset(_), "withoutOffset" | "withoutOffsetOrTimezone")
        | (Zone::Iana(_), "withoutTimezone" | "withoutOffsetOrTimezone") => Zone::Naive,
        _ => zone,
    }
}
impl StripZone for Date {
    fn strip_zone(self, mode: &str) -> Self {
        Self {
            zone: stripped(self.zone, mode),
            ..self
        }
    }
}
impl StripZone for DateTime {
    fn strip_zone(self, mode: &str) -> Self {
        Self {
            zone: stripped(self.zone, mode),
            ..self
        }
    }
}
pub fn strip_zone<T: StripZone>(value: T, mode: &str) -> T {
    value.strip_zone(mode)
}

fn elapsed(delta: DTDuration) -> Result<chrono::Duration, String> {
    let whole = delta.0.trunc().to_i64().ok_or("duration out of range")?;
    let fraction = (delta.0.fract() * Decimal::from(1_000_000_000))
        .to_i64()
        .filter(|value| Decimal::from(*value) == delta.0.fract() * Decimal::from(1_000_000_000))
        .ok_or("sub-nanosecond duration cannot be applied to a point")?;
    chrono::Duration::try_seconds(whole)
        .and_then(|duration| duration.checked_add(&chrono::Duration::nanoseconds(fraction)))
        .ok_or_else(|| "duration out of range".into())
}

fn change_month(date: NaiveDate, duration: YMDuration) -> Result<NaiveDate, String> {
    let shift = duration
        .0
        .to_i32()
        .filter(|value| Decimal::from(*value) == duration.0)
        .ok_or("fractional or out-of-range calendar month")?;
    let months = date
        .year()
        .checked_mul(12)
        .and_then(|value| value.checked_add(date.month0() as i32))
        .and_then(|value| value.checked_add(shift))
        .ok_or("calendar month overflow")?;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    if !(0..=9999).contains(&year) {
        return Err("date outside supported year range".into());
    }
    (1..=date.day())
        .rev()
        .find_map(|day| NaiveDate::from_ymd_opt(year, month, day))
        .ok_or_else(|| "invalid calendar month".into())
}

fn valid_date(value: Date) -> Result<Date, String> {
    if !(0..=9999).contains(&value.date.year()) {
        return Err("date outside supported year range".into());
    }
    if let Zone::Iana(zone) = value.zone
        && zone
            .from_local_datetime(&value.date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .is_none()
    {
        return Err("ambiguous or nonexistent local midnight".into());
    }
    Ok(value)
}

pub fn add_date_dt(value: Date, duration: DTDuration) -> Result<Date, String> {
    let seconds = duration.0.trunc().to_i64().ok_or("duration out of range")?;
    let days = seconds / 86_400;
    let date = value
        .date
        .checked_add_signed(chrono::Duration::try_days(days).ok_or("duration out of range")?)
        .ok_or("date outside supported range")?;
    valid_date(Date { date, ..value })
}

pub fn add_date_ym(value: Date, duration: YMDuration) -> Result<Date, String> {
    valid_date(Date {
        date: change_month(value.date, duration)?,
        ..value
    })
}

pub fn add_time_dt(value: Time, duration: DTDuration) -> Result<Time, String> {
    elapsed(duration)?;
    let fractional_day = (duration.0 % Decimal::from(86_400)) * Decimal::from(1_000_000_000);
    let nanos = fractional_day.to_i64().ok_or("duration out of range")?;
    let current = i64::from(value.time.num_seconds_from_midnight()) * 1_000_000_000
        + i64::from(value.time.nanosecond());
    let next = (current + nanos).rem_euclid(86_400_000_000_000);
    let time = NaiveTime::from_num_seconds_from_midnight_opt(
        (next / 1_000_000_000) as u32,
        (next % 1_000_000_000) as u32,
    )
    .ok_or("time outside supported range")?;
    Ok(Time { time, ..value })
}

pub fn add_datetime_dt(value: DateTime, duration: DTDuration) -> Result<DateTime, String> {
    let delta = elapsed(duration)?;
    let datetime = match value.zone {
        Zone::Naive => value.datetime.checked_add_signed(delta),
        zone => zone
            .instant(value.datetime)
            .and_then(|utc| utc.checked_add_signed(delta))
            .map(|utc| match zone {
                Zone::Offset(offset) => utc.with_timezone(&offset).naive_local(),
                Zone::Iana(zone) => utc.with_timezone(&zone).naive_local(),
                Zone::Naive => unreachable!(),
            }),
    }
    .ok_or("datetime outside supported range")?;
    Ok(DateTime { datetime, ..value })
}

pub fn add_datetime_ym(value: DateTime, duration: YMDuration) -> Result<DateTime, String> {
    let datetime = change_month(value.datetime.date(), duration)?.and_time(value.datetime.time());
    if let Zone::Iana(zone) = value.zone
        && zone.from_local_datetime(&datetime).single().is_none()
    {
        return Err("ambiguous or nonexistent local datetime".into());
    }
    Ok(DateTime { datetime, ..value })
}

pub fn compare_checked(
    left: &impl TemporalParts,
    right: &impl TemporalParts,
    clock: chrono::DateTime<Local>,
) -> Result<Ordering, String> {
    let point = |value: &dyn TemporalParts| {
        let date = value.date().unwrap_or_else(|| match value.zone() {
            Zone::Iana(zone) => clock.with_timezone(&zone).date_naive(),
            _ => clock.date_naive(),
        });
        date.and_time(
            value
                .time()
                .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
        )
    };
    compare_points((point(left), left.zone()), (point(right), right.zone()))
        .ok_or_else(|| "incompatible or unresolvable temporal zones".into())
}

fn point_difference(
    left: (NaiveDateTime, Zone),
    right: (NaiveDateTime, Zone),
) -> Result<DTDuration, String> {
    let difference = match (left.1, right.1) {
        (Zone::Naive, Zone::Naive) => left.0.signed_duration_since(right.0),
        (Zone::Naive, _) | (_, Zone::Naive) => {
            return Err("incompatible naive and zoned points".into());
        }
        _ => left
            .1
            .instant(left.0)
            .ok_or("unresolvable local datetime")?
            .signed_duration_since(
                right
                    .1
                    .instant(right.0)
                    .ok_or("unresolvable local datetime")?,
            ),
    };
    let seconds = difference.num_seconds();
    let remainder = difference - chrono::Duration::seconds(seconds);
    let nanos = remainder
        .num_nanoseconds()
        .ok_or("point difference out of range")?;
    let seconds = Decimal::from(seconds)
        .checked_add(Decimal::from(nanos) / Decimal::from(1_000_000_000))
        .ok_or("point difference out of range")?;
    Ok(DTDuration(seconds))
}

pub fn subtract_dates(left: Date, right: Date) -> Result<DTDuration, String> {
    point_difference(
        (left.date.and_hms_opt(0, 0, 0).unwrap(), left.zone),
        (right.date.and_hms_opt(0, 0, 0).unwrap(), right.zone),
    )
}

pub fn subtract_datetimes(left: DateTime, right: DateTime) -> Result<DTDuration, String> {
    point_difference((left.datetime, left.zone), (right.datetime, right.zone))
}

pub fn dt_between_dates(from: Date, to: Date) -> Result<DTDuration, String> {
    subtract_dates(to, from)
}

pub fn dt_between_datetimes(from: DateTime, to: DateTime) -> Result<DTDuration, String> {
    subtract_datetimes(to, from)
}

fn whole_months(
    from: (NaiveDateTime, Zone),
    to: (NaiveDateTime, Zone),
) -> Result<YMDuration, String> {
    let negative = point_difference(to, from)?.is_negative();
    let (start, end) = if negative { (to, from) } else { (from, to) };
    let end = match start.1 {
        Zone::Naive => end.0,
        Zone::Offset(offset) => end
            .1
            .instant(end.0)
            .ok_or("invalid zoned endpoint")?
            .with_timezone(&offset)
            .naive_local(),
        Zone::Iana(zone) => end
            .1
            .instant(end.0)
            .ok_or("invalid zoned endpoint")?
            .with_timezone(&zone)
            .naive_local(),
    };
    let start = start.0;
    let months = (end.year() - start.year()) * 12 + end.month() as i32 - start.month() as i32;
    let incomplete =
        end.day() < start.day() || end.day() == start.day() && end.time() < start.time();
    Ok(YMDuration(Decimal::from(
        (months - i32::from(incomplete)) * if negative { -1 } else { 1 },
    )))
}

pub fn ym_between_dates(from: Date, to: Date) -> Result<YMDuration, String> {
    whole_months(
        (from.date.and_hms_opt(0, 0, 0).unwrap(), from.zone),
        (to.date.and_hms_opt(0, 0, 0).unwrap(), to.zone),
    )
}

pub fn ym_between_datetimes(from: DateTime, to: DateTime) -> Result<YMDuration, String> {
    whole_months((from.datetime, from.zone), (to.datetime, to.zone))
}

impl Zone {
    fn instant(self, datetime: NaiveDateTime) -> Option<chrono::DateTime<Utc>> {
        match self {
            Self::Naive => None,
            Self::Offset(offset) => offset.from_local_datetime(&datetime).single(),
            Self::Iana(zone) => zone
                .from_local_datetime(&datetime)
                .single()
                .map(|dt| dt.fixed_offset()),
        }
        .map(|dt| dt.with_timezone(&Utc))
    }
}

fn compare_points(first: (NaiveDateTime, Zone), second: (NaiveDateTime, Zone)) -> Option<Ordering> {
    match (first.1, second.1) {
        (Zone::Naive, Zone::Naive) => first.0.partial_cmp(&second.0),
        (Zone::Naive, _) | (_, Zone::Naive) => None,
        _ => first
            .1
            .instant(first.0)?
            .partial_cmp(&second.1.instant(second.0)?),
    }
}

macro_rules! equality_by_comparison {
    ($($ty:ty),+) => {$ (
        impl PartialEq for $ty {
            fn eq(&self, other: &Self) -> bool {
                self.partial_cmp(other) == Some(Ordering::Equal)
            }
        }
    )+};
}
equality_by_comparison!(Date, Time, DateTime);
impl Eq for DateTime {}

impl PartialOrd for Date {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare_points(
            (self.date.and_hms_opt(0, 0, 0)?, self.zone),
            (other.date.and_hms_opt(0, 0, 0)?, other.zone),
        )
    }
}

impl PartialOrd for Time {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let date = Local::now().date_naive();
        compare_points(
            (date.and_time(self.time), self.zone),
            (date.and_time(other.time), other.zone),
        )
    }
}

impl PartialOrd for DateTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        compare_points((self.datetime, self.zone), (other.datetime, other.zone))
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Naive => Ok(()),
            Self::Offset(offset) if offset.local_minus_utc() == 0 => write!(f, "Z"),
            Self::Offset(offset) => write!(f, "{offset}"),
            Self::Iana(zone) => write!(f, "[{zone}]"),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.date.format("%Y-%m-%d"), self.zone)
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.time.format("%H:%M:%S%.f"), self.zone)
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.datetime.format("%Y-%m-%dT%H:%M:%S%.f"),
            self.zone
        )
    }
}

macro_rules! serde_text {
    ($($ty:ty),+) => {$ (
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_string())
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?.parse().map_err(D::Error::custom)
            }
        }
    )+};
}
serde_text!(Date, Time, DateTime);
