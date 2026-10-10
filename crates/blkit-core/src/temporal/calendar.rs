use std::{cmp::Ordering, collections::HashSet};

use rust_decimal::{Decimal, prelude::ToPrimitive};

use chrono::Local;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::{Date, DateTime, Zone, compare_checked};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CalendarPoint {
    Date(Date),
    DateTime(DateTime),
}

impl From<Date> for CalendarPoint {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}
impl From<DateTime> for CalendarPoint {
    fn from(value: DateTime) -> Self {
        Self::DateTime(value)
    }
}

impl CalendarPoint {
    pub fn zone(&self) -> Zone {
        match self {
            Self::Date(value) => value.zone,
            Self::DateTime(value) => value.zone,
        }
    }

    pub fn equals_point(&self, other: impl Into<CalendarPoint>) -> Result<bool, String> {
        Ok(self.compare(&other.into())?.is_eq())
    }

    pub fn compare(&self, other: &Self) -> Result<Ordering, String> {
        let clock = Local::now();
        match (self, other) {
            (Self::Date(a), Self::Date(b)) => compare_checked(a, b, clock),
            (Self::Date(a), Self::DateTime(b)) => compare_checked(a, b, clock),
            (Self::DateTime(a), Self::Date(b)) => compare_checked(a, b, clock),
            (Self::DateTime(a), Self::DateTime(b)) => compare_checked(a, b, clock),
        }
    }
}

fn include() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalendarRange {
    pub start: CalendarPoint,
    pub end: CalendarPoint,
    #[serde(default = "include")]
    pub include_start: bool,
    #[serde(default = "include")]
    pub include_end: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CalendarValue {
    Point(CalendarPoint),
    Range(CalendarRange),
}

impl CalendarRange {
    pub fn contains(&self, point: &CalendarPoint) -> Result<bool, String> {
        let lower = point.compare(&self.start)?;
        let upper = point.compare(&self.end)?;
        Ok((lower.is_gt() || lower.is_eq() && self.include_start)
            && (upper.is_lt() || upper.is_eq() && self.include_end))
    }
}

impl CalendarValue {
    pub fn contains(&self, point: &CalendarPoint) -> Result<bool, String> {
        match self {
            Self::Point(value) => value.equals_point(point.clone()),
            Self::Range(range) => range.contains(point),
        }
    }
    pub fn equals_range(
        &self,
        lower: Option<CalendarPoint>,
        upper: Option<CalendarPoint>,
        include_lower: bool,
        include_upper: bool,
    ) -> Result<bool, String> {
        let (Self::Range(range), Some(lower), Some(upper)) = (self, lower, upper) else {
            return Ok(false);
        };
        Ok(range.start.compare(&lower)?.is_eq()
            && range.end.compare(&upper)?.is_eq()
            && range.include_start == include_lower
            && range.include_end == include_upper)
    }
    pub fn equals_point(&self, other: impl Into<CalendarPoint>) -> Result<bool, String> {
        match self {
            Self::Point(value) => value.equals_point(other),
            Self::Range(_) => Ok(false),
        }
    }

    pub fn bounds(&self) -> (&CalendarPoint, &CalendarPoint) {
        match self {
            Self::Point(point) => (point, point),
            Self::Range(range) => (&range.start, &range.end),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarEntry {
    pub value: CalendarValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    valid_from: CalendarPoint,
    valid_to: CalendarPoint,
    entries: Vec<CalendarEntry>,
}

impl PartialEq for Calendar {
    fn eq(&self, other: &Self) -> bool {
        // ponytail: O(n²) entry-set comparison; index entries if calendars become large.
        self.valid_from == other.valid_from
            && self.valid_to == other.valid_to
            && self
                .entries
                .iter()
                .all(|entry| other.entries.contains(entry))
            && other
                .entries
                .iter()
                .all(|entry| self.entries.contains(entry))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalendarInput {
    valid_from: CalendarPoint,
    valid_to: CalendarPoint,
    entries: Vec<CalendarEntry>,
}

impl Calendar {
    pub fn valid_from(&self) -> &CalendarPoint {
        &self.valid_from
    }
    pub fn valid_to(&self) -> &CalendarPoint {
        &self.valid_to
    }

    pub fn new(
        valid_from: CalendarPoint,
        valid_to: CalendarPoint,
        mut entries: Vec<CalendarEntry>,
    ) -> Result<Self, String> {
        let naive = matches!(valid_from.zone(), Zone::Naive);
        let check_kind = |point: &CalendarPoint| {
            if matches!(point.zone(), Zone::Naive) != naive {
                Err("calendar cannot mix naive and zoned values".to_string())
            } else {
                Ok(())
            }
        };
        check_kind(&valid_to)?;
        if valid_from.compare(&valid_to)?.is_gt() {
            return Err("invalid calendar validity bounds".into());
        }
        for entry in &entries {
            let (start, end) = entry.value.bounds();
            check_kind(start)?;
            check_kind(end)?;
            if let CalendarValue::Range(range) = &entry.value {
                if std::mem::discriminant(start) != std::mem::discriminant(end) {
                    return Err("calendar range requires matching point types".into());
                }
                let order = start.compare(end)?;
                if order.is_gt() || order.is_eq() && !(range.include_start && range.include_end) {
                    return Err("empty or reversed calendar entry range".into());
                }
            }
            if start.compare(&valid_from)?.is_lt() || end.compare(&valid_to)?.is_gt() {
                return Err("calendar entry is outside validity bounds".into());
            }
        }
        entries.sort_by(|a, b| {
            a.value
                .bounds()
                .0
                .compare(b.value.bounds().0)
                .unwrap()
                .then_with(|| match (&a.value, &b.value) {
                    (CalendarValue::Point(_), CalendarValue::Range(_)) => Ordering::Less,
                    (CalendarValue::Range(_), CalendarValue::Point(_)) => Ordering::Greater,
                    _ => Ordering::Equal,
                })
        });
        Ok(Self {
            valid_from,
            valid_to,
            entries,
        })
    }

    pub fn count(&self) -> Decimal {
        Decimal::from(self.entries.len())
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> Vec<CalendarEntry> {
        self.entries.clone()
    }
    pub fn names(&self) -> Vec<String> {
        let mut found = HashSet::new();
        self.entries
            .iter()
            .filter_map(|entry| entry.name.as_ref())
            .filter(|name| found.insert((*name).clone()))
            .cloned()
            .collect()
    }
    pub fn find(&self, name: &str) -> Vec<CalendarEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.name.as_deref() == Some(name))
            .cloned()
            .collect()
    }
    pub fn valid_range(&self) -> CalendarRange {
        CalendarRange {
            start: self.valid_from.clone(),
            end: self.valid_to.clone(),
            include_start: true,
            include_end: true,
        }
    }
    pub fn entries_for(
        &self,
        point: impl Into<CalendarPoint>,
    ) -> Result<Vec<CalendarEntry>, String> {
        let point = point.into();
        self.entries
            .iter()
            .filter_map(|entry| {
                let covered = match &entry.value {
                    CalendarValue::Point(value) => value.compare(&point).map(|cmp| cmp.is_eq()),
                    CalendarValue::Range(range) => range.contains(&point),
                };
                match covered {
                    Ok(true) => Some(Ok(entry.clone())),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                }
            })
            .collect()
    }
    pub fn contains(&self, point: impl Into<CalendarPoint>) -> Result<bool, String> {
        Ok(!self.entries_for(point)?.is_empty())
    }
    pub fn entries_in(
        &self,
        lower: Option<CalendarPoint>,
        upper: Option<CalendarPoint>,
        include_lower: bool,
        include_upper: bool,
    ) -> Result<Vec<CalendarEntry>, String> {
        if let (Some(start), Some(end)) = (&lower, &upper) {
            let order = start.compare(end)?;
            if order.is_gt() || order.is_eq() && !(include_lower && include_upper) {
                return Ok(vec![]);
            }
        }
        self.entries
            .iter()
            .filter_map(|entry| {
                let (start, end) = entry.value.bounds();
                let (include_start, include_end) = match &entry.value {
                    CalendarValue::Point(_) => (true, true),
                    CalendarValue::Range(range) => (range.include_start, range.include_end),
                };
                let hits = (|| {
                    let after_start = match &lower {
                        Some(lower) => {
                            let cmp = end.compare(lower)?;
                            cmp.is_gt() || cmp.is_eq() && include_end && include_lower
                        }
                        None => true,
                    };
                    let before_end = match &upper {
                        Some(upper) => {
                            let cmp = start.compare(upper)?;
                            cmp.is_lt() || cmp.is_eq() && include_start && include_upper
                        }
                        None => true,
                    };
                    Ok::<_, String>(after_start && before_end)
                })();
                match hits {
                    Ok(true) => Some(Ok(entry.clone())),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                }
            })
            .collect()
    }
    pub fn overlaps(
        &self,
        lower: Option<CalendarPoint>,
        upper: Option<CalendarPoint>,
        include_lower: bool,
        include_upper: bool,
    ) -> Result<bool, String> {
        Ok(!self
            .entries_in(lower, upper, include_lower, include_upper)?
            .is_empty())
    }
    pub fn adjacent(
        &self,
        point: impl Into<CalendarPoint>,
        n: Decimal,
        forward: bool,
    ) -> Result<CalendarEntry, String> {
        let n = n
            .to_usize()
            .filter(|count| *count > 0 && Decimal::from(*count) == n)
            .ok_or("n must be a positive integer")?;
        let point = point.into();
        let mut candidates = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (start, end) = entry.value.bounds();
                let compared = if forward {
                    start.compare(&point)
                } else {
                    end.compare(&point)
                };
                match compared {
                    Ok(order) if (forward && order.is_gt()) || (!forward && order.is_lt()) => {
                        Some(Ok(entry))
                    }
                    Ok(_) => None,
                    Err(err) => Some(Err(err)),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !forward {
            candidates.sort_by(|a, b| b.value.bounds().1.compare(a.value.bounds().1).unwrap());
        }
        candidates
            .get(n - 1)
            .cloned()
            .cloned()
            .ok_or_else(|| "no such calendar entry".into())
    }
}

pub fn entry_name(entry: &CalendarEntry) -> Result<String, String> {
    entry
        .name
        .clone()
        .ok_or_else(|| "calendar entry has no name".into())
}

#[derive(Clone, Debug)]
pub enum CalendarTarget {
    Name(String),
    Pattern(regex::Regex),
    Point(CalendarPoint),
    Range(Option<CalendarPoint>, Option<CalendarPoint>, bool, bool),
    Any(Vec<CalendarTarget>),
}

impl From<String> for CalendarTarget {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}
impl From<Date> for CalendarTarget {
    fn from(value: Date) -> Self {
        Self::Point(value.into())
    }
}
impl From<DateTime> for CalendarTarget {
    fn from(value: DateTime) -> Self {
        Self::Point(value.into())
    }
}

impl CalendarTarget {
    pub fn pattern(text: &str) -> Result<Self, String> {
        regex::Regex::new(text)
            .map(Self::Pattern)
            .map_err(|err| err.to_string())
    }

    pub fn matches(&self, entry: &CalendarEntry, range_match: &str) -> Result<bool, String> {
        match self {
            Self::Name(name) => Ok(entry.name.as_ref() == Some(name)),
            Self::Pattern(pattern) => Ok(entry
                .name
                .as_ref()
                .is_some_and(|name| pattern.is_match(name))),
            Self::Point(point) => entry.value.equals_point(point.clone()),
            Self::Any(targets) => {
                for target in targets {
                    if target.matches(entry, range_match)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::Range(start, end, include_start, include_end) => {
                let (entry_start, entry_end) = entry.value.bounds();
                let (entry_include_start, entry_include_end) = match &entry.value {
                    CalendarValue::Point(_) => (true, true),
                    CalendarValue::Range(range) => (range.include_start, range.include_end),
                };
                let within = |point: &CalendarPoint,
                              lower: &Option<CalendarPoint>,
                              upper: &Option<CalendarPoint>,
                              include_lower: bool,
                              include_upper: bool|
                 -> Result<bool, String> {
                    let low = match lower {
                        Some(lower) => {
                            let order = point.compare(lower)?;
                            order.is_gt() || order.is_eq() && include_lower
                        }
                        None => true,
                    };
                    let high = match upper {
                        Some(upper) => {
                            let order = point.compare(upper)?;
                            order.is_lt() || order.is_eq() && include_upper
                        }
                        None => true,
                    };
                    Ok(low && high)
                };
                match range_match {
                    "equality" => entry.value.equals_range(
                        start.clone(),
                        end.clone(),
                        *include_start,
                        *include_end,
                    ),
                    "entryWithin" => Ok(within(
                        entry_start,
                        start,
                        end,
                        *include_start || !entry_include_start,
                        true,
                    )? && within(
                        entry_end,
                        start,
                        end,
                        true,
                        *include_end || !entry_include_end,
                    )?),
                    "entryEncloses" => {
                        let (Some(start), Some(end)) = (start, end) else {
                            return Ok(false);
                        };
                        Ok(within(
                            start,
                            &Some(entry_start.clone()),
                            &Some(entry_end.clone()),
                            entry_include_start || !include_start,
                            true,
                        )? && within(
                            end,
                            &Some(entry_start.clone()),
                            &Some(entry_end.clone()),
                            true,
                            entry_include_end || !include_end,
                        )?)
                    }
                    "overlap" => {
                        let low = match start {
                            Some(start) => {
                                let cmp = entry_end.compare(start)?;
                                cmp.is_gt() || cmp.is_eq() && entry_include_end && *include_start
                            }
                            None => true,
                        };
                        let high = match end {
                            Some(end) => {
                                let cmp = entry_start.compare(end)?;
                                cmp.is_lt() || cmp.is_eq() && entry_include_start && *include_end
                            }
                            None => true,
                        };
                        Ok(low && high)
                    }
                    _ => Err(format!("invalid rangeMatch: {range_match}")),
                }
            }
        }
    }
}

impl Calendar {
    pub fn filter(
        &self,
        target: &CalendarTarget,
        keep: bool,
        range_match: &str,
    ) -> Result<Self, String> {
        if !matches!(
            range_match,
            "equality" | "entryWithin" | "entryEncloses" | "overlap"
        ) {
            return Err(format!("invalid rangeMatch: {range_match}"));
        }
        let mut entries = Vec::new();
        for entry in &self.entries {
            if target.matches(entry, range_match)? == keep {
                entries.push(entry.clone());
            }
        }
        Self::new(self.valid_from.clone(), self.valid_to.clone(), entries)
    }

    pub fn merge(
        calendars: Vec<Self>,
        dedupe: Option<&str>,
        tiebreak: &str,
    ) -> Result<Self, String> {
        if !matches!(dedupe, None | Some("value" | "valueAndName")) {
            return Err("invalid dedupeBy".into());
        }
        if !matches!(tiebreak, "first" | "name") {
            return Err("invalid tiebreak".into());
        }
        let first = calendars
            .first()
            .ok_or("calendarMerge requires a nonempty list")?;
        let (mut from, mut to) = (first.valid_from.clone(), first.valid_to.clone());
        let mut entries: Vec<CalendarEntry> = Vec::new();
        for calendar in &calendars {
            if from.compare(&calendar.valid_from)?.is_gt() {
                from = calendar.valid_from.clone();
            }
            if to.compare(&calendar.valid_to)?.is_lt() {
                to = calendar.valid_to.clone();
            }
            for entry in &calendar.entries {
                let duplicate = dedupe.and_then(|mode| {
                    entries.iter().position(|other| {
                        other.value == entry.value && (mode == "value" || other.name == entry.name)
                    })
                });
                if let Some(index) = duplicate {
                    if tiebreak == "name" && entry.name < entries[index].name {
                        entries[index] = entry.clone();
                    }
                } else {
                    entries.push(entry.clone());
                }
            }
        }
        Self::new(from, to, entries)
    }
}

impl<'de> Deserialize<'de> for Calendar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = CalendarInput::deserialize(deserializer)?;
        Self::new(input.valid_from, input.valid_to, input.entries).map_err(D::Error::custom)
    }
}
