use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashSet},
};

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use serde_json::Value;

use crate::temporal::{Calendar, CalendarEntry, DTDuration, Date, DateTime, Time, YMDuration};

/// A typed table cell. Authored cells cannot contain null; missing cells are internal only.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    String(String),
    Enum { kind: String, variant: String },
    Number(Decimal),
    Date(Date),
    Time(Time),
    DateTime(DateTime),
    DTDuration(DTDuration),
    YMDuration(YMDuration),
    Calendar(Calendar),
    CalendarEntry(CalendarEntry),
    List(Vec<Cell>),
    Dictionary(BTreeMap<String, Cell>),
    Table(Box<Table>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellType {
    Null,
    Bool,
    String,
    Enum(String),
    Number,
    Date,
    Time,
    DateTime,
    DTDuration,
    YMDuration,
    Calendar,
    CalendarEntry,
    List(Box<Option<CellType>>),
    Dictionary,
    Table,
}

fn merge_type(inferred: &mut Option<CellType>, next: CellType) -> Result<(), String> {
    match (inferred.as_mut(), next) {
        (_, CellType::Null) => {}
        (None, kind) => *inferred = Some(kind),
        (Some(CellType::List(current)), CellType::List(next)) => {
            if let Some(next) = *next {
                merge_type(current, next)?;
            }
        }
        (Some(current), next) if *current == next => {}
        _ => return Err("incompatible table cell types".into()),
    }
    Ok(())
}

impl Cell {
    fn from_nullable_json(value: Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Array(values) => values
                .into_iter()
                .map(Self::from_nullable_json)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            Value::Object(values) => values
                .into_iter()
                .map(|(key, value)| Ok((key, Self::from_nullable_json(value)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()
                .map(Self::Dictionary),
            value => Self::try_from(value),
        }
    }

    fn contains_null(&self) -> bool {
        match self {
            Self::Null => true,
            Self::List(values) => values.iter().any(Self::contains_null),
            Self::Dictionary(values) => values.values().any(Self::contains_null),
            Self::Table(_) => false,
            _ => false,
        }
    }

    pub fn cell_type(&self) -> Result<CellType, String> {
        Ok(match self {
            Self::Null => CellType::Null,
            Self::Bool(_) => CellType::Bool,
            Self::String(_) => CellType::String,
            Self::Enum { kind, .. } => CellType::Enum(kind.clone()),
            Self::Number(_) => CellType::Number,
            Self::Date(_) => CellType::Date,
            Self::Time(_) => CellType::Time,
            Self::DateTime(_) => CellType::DateTime,
            Self::DTDuration(_) => CellType::DTDuration,
            Self::YMDuration(_) => CellType::YMDuration,
            Self::Calendar(_) => CellType::Calendar,
            Self::CalendarEntry(_) => CellType::CalendarEntry,
            Self::List(items) => {
                let mut element = None;
                for item in items {
                    merge_type(&mut element, item.cell_type()?)?;
                }
                CellType::List(Box::new(element))
            }
            Self::Dictionary(_) => CellType::Dictionary,
            Self::Table(_) => CellType::Table,
        })
    }
}

impl Serialize for Cell {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => value.serialize(serializer),
            Self::String(value) => value.serialize(serializer),
            Self::Enum { variant, .. } => variant.serialize(serializer),
            Self::Number(value) => {
                let number: Value =
                    serde_json::from_str(&value.to_string()).map_err(serde::ser::Error::custom)?;
                number.serialize(serializer)
            }
            Self::Date(value) => value.serialize(serializer),
            Self::Time(value) => value.serialize(serializer),
            Self::DateTime(value) => value.serialize(serializer),
            Self::DTDuration(value) => value.serialize(serializer),
            Self::YMDuration(value) => value.serialize(serializer),
            Self::Calendar(value) => value.serialize(serializer),
            Self::CalendarEntry(value) => value.serialize(serializer),
            Self::List(value) => value.serialize(serializer),
            Self::Dictionary(value) => value.serialize(serializer),
            Self::Table(value) => value.serialize(serializer),
        }
    }
}

impl TryFrom<Value> for Cell {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Null => Err("null is not an authored table cell".into()),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::String(value) => Ok(Self::String(value)),
            Value::Number(value) => Decimal::from_str_exact(&value.to_string())
                .map(Self::Number)
                .map_err(|error| error.to_string()),
            Value::Array(values) => values
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            Value::Object(values) => values
                .into_iter()
                .map(|(key, value)| Ok((key, Self::try_from(value)?)))
                .collect::<Result<BTreeMap<_, _>, _>>()
                .map(Self::Dictionary),
        }
    }
}

impl From<&str> for Cell {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<String> for Cell {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<bool> for Cell {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<Decimal> for Cell {
    fn from(value: Decimal) -> Self {
        Self::Number(value)
    }
}
impl From<Date> for Cell {
    fn from(value: Date) -> Self {
        Self::Date(value)
    }
}
impl From<Time> for Cell {
    fn from(value: Time) -> Self {
        Self::Time(value)
    }
}
impl From<DateTime> for Cell {
    fn from(value: DateTime) -> Self {
        Self::DateTime(value)
    }
}
impl From<DTDuration> for Cell {
    fn from(value: DTDuration) -> Self {
        Self::DTDuration(value)
    }
}
impl From<YMDuration> for Cell {
    fn from(value: YMDuration) -> Self {
        Self::YMDuration(value)
    }
}
impl From<Calendar> for Cell {
    fn from(value: Calendar) -> Self {
        Self::Calendar(value)
    }
}
impl From<CalendarEntry> for Cell {
    fn from(value: CalendarEntry) -> Self {
        Self::CalendarEntry(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
    InOrder(Vec<Option<Cell>>),
}

fn compare_cells(
    left: &Option<Cell>,
    right: &Option<Cell>,
    clock: chrono::DateTime<chrono::Local>,
) -> Result<Ordering, String> {
    match (left, right) {
        (None, None) => Ok(Ordering::Equal),
        (None, _) => Ok(Ordering::Greater),
        (_, None) => Ok(Ordering::Less),
        (Some(Cell::Number(a)), Some(Cell::Number(b))) => Ok(a.cmp(b)),
        (Some(Cell::String(a)), Some(Cell::String(b))) => Ok(a.cmp(b)),
        (Some(Cell::Bool(a)), Some(Cell::Bool(b))) => Ok(a.cmp(b)),
        (Some(Cell::Date(a)), Some(Cell::Date(b))) => crate::temporal::compare_checked(a, b, clock),
        (Some(Cell::Time(a)), Some(Cell::Time(b))) => crate::temporal::compare_checked(a, b, clock),
        (Some(Cell::DateTime(a)), Some(Cell::DateTime(b))) => {
            crate::temporal::compare_checked(a, b, clock)
        }
        (Some(Cell::DTDuration(a)), Some(Cell::DTDuration(b))) => Ok(a.cmp(b)),
        (Some(Cell::YMDuration(a)), Some(Cell::YMDuration(b))) => Ok(a.cmp(b)),
        _ => Err("table cells are not comparable".into()),
    }
}

fn join_keys_equal(
    left: &Option<Cell>,
    right: &Option<Cell>,
    clock: chrono::DateTime<chrono::Local>,
) -> Result<bool, String> {
    match (left, right) {
        (None, _) | (_, None) => Ok(false),
        (Some(Cell::Date(a)), Some(Cell::Date(b))) => {
            Ok(crate::temporal::compare_checked(a, b, clock)? == Ordering::Equal)
        }
        (Some(Cell::Time(a)), Some(Cell::Time(b))) => {
            Ok(crate::temporal::compare_checked(a, b, clock)? == Ordering::Equal)
        }
        (Some(Cell::DateTime(a)), Some(Cell::DateTime(b))) => {
            Ok(crate::temporal::compare_checked(a, b, clock)? == Ordering::Equal)
        }
        _ => Ok(left == right),
    }
}

/// An immutable ordered table with inferred column types; `None` cells are reserved for checked internal joins/unions.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    columns: Vec<String>,
    column_types: Vec<Option<CellType>>,
    rows: Vec<Vec<Option<Cell>>>,
}

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let rows: Vec<BTreeMap<&str, &Option<Cell>>> = self
            .rows
            .iter()
            .map(|row| {
                self.columns
                    .iter()
                    .zip(row)
                    .map(|(name, value)| (name.as_str(), value))
                    .collect()
            })
            .collect();
        let mut table = serializer.serialize_struct("Table", 2)?;
        table.serialize_field("columns", &self.columns)?;
        table.serialize_field("rows", &rows)?;
        table.end()
    }
}

impl<'de> Deserialize<'de> for Table {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTable {
            columns: Vec<String>,
            rows: Vec<BTreeMap<String, Value>>,
        }
        let RawTable { columns, rows } = RawTable::deserialize(deserializer)?;
        let mut seen = HashSet::new();
        if columns.iter().any(|name| !seen.insert(name)) {
            return Err(serde::de::Error::custom("duplicate table column"));
        }
        let positional = rows
            .into_iter()
            .map(|mut row| {
                if row.len() != columns.len() || columns.iter().any(|name| !row.contains_key(name))
                {
                    return Err(serde::de::Error::custom(
                        "table row keys do not match columns",
                    ));
                }
                columns
                    .iter()
                    .map(|name| {
                        let value = row.remove(name).ok_or_else(|| {
                            serde::de::Error::custom("table row keys do not match columns")
                        })?;
                        if value.is_null() {
                            Ok(None)
                        } else {
                            Cell::from_nullable_json(value)
                                .map(Some)
                                .map_err(serde::de::Error::custom)
                        }
                    })
                    .collect::<Result<Vec<_>, D::Error>>()
            })
            .collect::<Result<Vec<_>, D::Error>>()?;
        Table::from_optional(columns, positional).map_err(serde::de::Error::custom)
    }
}

impl Table {
    /// Construct from positional rows, retaining header order.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Cell>>) -> Result<Self, String> {
        if rows.iter().flatten().any(Cell::contains_null) {
            return Err("null is not an authored table cell".into());
        }
        Self::from_optional(
            columns,
            rows.into_iter()
                .map(|row| row.into_iter().map(Some).collect())
                .collect(),
        )
    }

    /// Construct with missing cells produced by a nullable source or a table operation.
    pub fn from_optional(
        columns: Vec<String>,
        rows: Vec<Vec<Option<Cell>>>,
    ) -> Result<Self, String> {
        let mut seen = HashSet::new();
        if columns.iter().any(|name| !seen.insert(name)) {
            return Err("duplicate table column".into());
        }
        let mut column_types = vec![None; columns.len()];
        let mut checked_rows = Vec::with_capacity(rows.len());
        for (row_index, mut row) in rows.into_iter().enumerate() {
            if row.len() != columns.len() {
                return Err(format!("table row {} has wrong width", row_index + 1));
            }
            for (index, value) in row.iter_mut().enumerate() {
                if matches!(value, Some(Cell::Null)) {
                    *value = None;
                }
                if let Some(value) = value {
                    let kind = value.cell_type()?;
                    merge_type(&mut column_types[index], kind).map_err(|_| {
                        format!("incompatible values in table column {}", columns[index])
                    })?;
                }
            }
            checked_rows.push(row);
        }
        Ok(Self {
            columns,
            column_types,
            rows: checked_rows,
        })
    }

    /// Adopt dictionaries with identical keys; dictionary key ordering is canonical.
    pub fn from_dicts(rows: Vec<BTreeMap<String, Cell>>) -> Result<Self, String> {
        let columns: Vec<String> = rows
            .first()
            .map(|row| row.keys().cloned().collect())
            .unwrap_or_default();
        let positional = rows
            .into_iter()
            .map(|row| {
                if row.len() != columns.len() || row.keys().ne(columns.iter()) {
                    return Err("table dictionaries must have identical keys".into());
                }
                Ok(row.into_values().collect())
            })
            .collect::<Result<Vec<_>, String>>()?;
        Self::new(columns, positional)
    }

    /// Resolve a one-based (or negative-from-end) row index without truncation.
    pub fn same_rows(&self, other: &Self) -> bool {
        if self.columns.len() != other.columns.len() || self.rows.len() != other.rows.len() {
            return false;
        }
        let positions = self
            .columns
            .iter()
            .map(|name| other.columns.iter().position(|column| column == name))
            .collect::<Option<Vec<_>>>();
        positions.is_some_and(|positions| {
            self.rows.iter().zip(&other.rows).all(|(left, right)| {
                left.iter()
                    .zip(&positions)
                    .all(|(cell, &position)| cell == &right[position])
            })
        })
    }

    pub fn row_index(&self, index: Decimal) -> Result<usize, String> {
        if !index.fract().is_zero() || index.is_zero() {
            return Err("table row index must be a nonzero integer".into());
        }
        let length = i64::try_from(self.rows.len()).map_err(|_| "table has too many rows")?;
        let position: i64 = index
            .try_into()
            .map_err(|_| "table row index out of bounds")?;
        let offset = if position < 0 {
            length.checked_add(position)
        } else {
            position.checked_sub(1)
        };
        offset
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&value| value < self.rows.len())
            .ok_or_else(|| "table row index out of bounds".into())
    }

    /// Expand an inclusive row range, bounding both output growth and arithmetic.
    pub fn row_range(&self, start: Decimal, end: Decimal) -> Result<Vec<usize>, String> {
        let start = self.row_index(start)?;
        let end = self.row_index(end)?;
        if start.abs_diff(end) > 1_000_000 {
            return Err("table range exceeds resource limit".into());
        }
        if start <= end {
            Ok((start..=end).collect())
        } else {
            Ok((end..=start).rev().collect())
        }
    }

    pub fn select(&self, indices: &[usize], columns: &[String]) -> Result<Self, String> {
        let mut positions = Vec::new();
        let mut names = Vec::new();
        for name in columns {
            let position = self
                .columns
                .iter()
                .position(|column| column == name)
                .ok_or_else(|| format!("unknown table column: {name}"))?;
            if !positions.contains(&position) {
                positions.push(position);
                names.push(name.clone());
            }
        }
        let mut rows = Vec::with_capacity(indices.len());
        let mut seen = HashSet::new();
        for &index in indices {
            let row = self
                .rows
                .get(index)
                .ok_or("table row index out of bounds")?;
            if seen.insert(index) {
                rows.push(
                    positions
                        .iter()
                        .map(|&position| row[position].clone())
                        .collect(),
                );
            }
        }
        Ok(Self {
            columns: names,
            column_types: positions
                .iter()
                .map(|&position| self.column_types[position].clone())
                .collect(),
            rows,
        })
    }

    pub fn join(&self, other: &Self, keys: &[String], mode: &str) -> Result<Self, String> {
        if !matches!(mode, "inner" | "left" | "right" | "outer" | "cross")
            || (mode == "cross") != keys.is_empty()
        {
            return Err("join requires valid mode and keys".into());
        }
        let mut selected = HashSet::new();
        let positions = keys
            .iter()
            .map(|name| {
                if !selected.insert(name) {
                    return Err(format!("duplicate join key: {name}"));
                }
                let left = self
                    .columns
                    .iter()
                    .position(|column| column == name)
                    .ok_or_else(|| format!("unknown left join key: {name}"))?;
                let right = other
                    .columns
                    .iter()
                    .position(|column| column == name)
                    .ok_or_else(|| format!("unknown right join key: {name}"))?;
                Ok((left, right))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let left_nonkeys = (0..self.columns.len())
            .filter(|&index| !positions.iter().any(|&(left, _)| left == index))
            .collect::<Vec<_>>();
        let right_nonkeys = (0..other.columns.len())
            .filter(|&index| !positions.iter().any(|&(_, right)| right == index))
            .collect::<Vec<_>>();
        if right_nonkeys.iter().any(|&index| {
            left_nonkeys
                .iter()
                .any(|&left| self.columns[left] == other.columns[index])
        }) {
            return Err("join non-key columns collide".into());
        }
        let mut columns = keys.to_vec();
        columns.extend(
            left_nonkeys
                .iter()
                .map(|&index| self.columns[index].clone()),
        );
        columns.extend(
            right_nonkeys
                .iter()
                .map(|&index| other.columns[index].clone()),
        );
        if columns.len() > 1024 {
            return Err("table join exceeds resource limit".into());
        }
        let mut column_types = Vec::with_capacity(columns.len());
        for &(left, right) in &positions {
            let mut kind = self.column_types[left].clone();
            if let Some(other) = &other.column_types[right] {
                merge_type(&mut kind, other.clone())?;
            }
            column_types.push(kind);
        }
        column_types.extend(
            left_nonkeys
                .iter()
                .map(|&index| self.column_types[index].clone()),
        );
        column_types.extend(
            right_nonkeys
                .iter()
                .map(|&index| other.column_types[index].clone()),
        );
        if self
            .rows
            .len()
            .checked_mul(other.rows.len())
            .is_none_or(|size| size > 1_000_000)
        {
            return Err("table join exceeds resource limit".into());
        }
        let mut rows = Vec::new();
        let mut matched_right = vec![false; other.rows.len()];
        let clock = chrono::Local::now();
        for left in &self.rows {
            let mut matched = false;
            for (right_index, right) in other.rows.iter().enumerate() {
                if mode == "cross"
                    || positions.iter().try_fold(true, |equal, &(a, b)| {
                        if !equal {
                            Ok(false)
                        } else {
                            join_keys_equal(&left[a], &right[b], clock)
                        }
                    })?
                {
                    matched = true;
                    matched_right[right_index] = true;
                    if rows.len() >= 1_000_000 {
                        return Err("table join exceeds resource limit".into());
                    }
                    let mut row = positions
                        .iter()
                        .map(|&(index, _)| left[index].clone())
                        .collect::<Vec<_>>();
                    row.extend(left_nonkeys.iter().map(|&index| left[index].clone()));
                    row.extend(right_nonkeys.iter().map(|&index| right[index].clone()));
                    rows.push(row);
                }
            }
            if !matched && matches!(mode, "left" | "outer") {
                if rows.len() >= 1_000_000 {
                    return Err("table join exceeds resource limit".into());
                }
                let mut row = positions
                    .iter()
                    .map(|&(index, _)| left[index].clone())
                    .collect::<Vec<_>>();
                row.extend(left_nonkeys.iter().map(|&index| left[index].clone()));
                row.extend(vec![None; right_nonkeys.len()]);
                rows.push(row);
            }
        }
        if matches!(mode, "right" | "outer") {
            for (index, right) in other.rows.iter().enumerate() {
                if !matched_right[index] {
                    if rows.len() >= 1_000_000 {
                        return Err("table join exceeds resource limit".into());
                    }
                    let mut row = positions
                        .iter()
                        .map(|&(_, position)| right[position].clone())
                        .collect::<Vec<_>>();
                    row.extend(vec![None; left_nonkeys.len()]);
                    row.extend(
                        right_nonkeys
                            .iter()
                            .map(|&position| right[position].clone()),
                    );
                    rows.push(row);
                }
            }
        }
        let mut joined = Self::from_optional(columns, rows)?;
        for (actual, expected) in joined.column_types.iter_mut().zip(column_types) {
            if let Some(expected) = expected {
                merge_type(actual, expected)?;
            }
        }
        Ok(joined)
    }

    pub fn aggregate<F>(
        &self,
        keys: &[String],
        outputs: &[String],
        mut evaluate: F,
    ) -> Result<Self, String>
    where
        F: FnMut(&Self) -> Result<Vec<Option<Cell>>, String>,
    {
        if keys.is_empty() || outputs.is_empty() {
            return Err("groupBy requires keys and agg requires outputs".into());
        }
        let mut seen = HashSet::new();
        let indices = keys
            .iter()
            .map(|name| {
                if !seen.insert(name) {
                    return Err(format!("duplicate group key: {name}"));
                }
                self.columns
                    .iter()
                    .position(|column| column == name)
                    .ok_or_else(|| format!("unknown table column: {name}"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for name in outputs {
            if !seen.insert(name) {
                return Err(format!("duplicate group output: {name}"));
            }
        }
        if seen.len() > 1024 {
            return Err("table aggregation exceeds resource limit".into());
        }
        let clock = chrono::Local::now();
        let mut groups: Vec<(Vec<Option<Cell>>, Vec<usize>)> = Vec::new();
        let mut comparisons = 0usize;
        for (position, row) in self.rows.iter().enumerate() {
            let values = indices
                .iter()
                .map(|&index| row[index].clone())
                .collect::<Vec<_>>();
            let mut existing = false;
            for (group, rows) in &mut groups {
                comparisons = comparisons
                    .checked_add(1)
                    .ok_or("table aggregation exceeds resource limit")?;
                if comparisons > 1_000_000 {
                    return Err("table aggregation exceeds resource limit".into());
                }
                let mut same = true;
                for (a, b) in group.iter().zip(&values) {
                    if a.is_none() && b.is_none() {
                        continue;
                    }
                    if !join_keys_equal(a, b, clock)? {
                        same = false;
                        break;
                    }
                }
                if same {
                    rows.push(position);
                    existing = true;
                    break;
                }
            }
            if !existing {
                groups.push((values, vec![position]));
            }
        }
        let mut columns = keys.to_vec();
        columns.extend_from_slice(outputs);
        let mut rows = Vec::with_capacity(groups.len());
        for (values, indices) in groups {
            let selected = self.select(&indices, &self.columns)?;
            let produced = evaluate(&selected)?;
            if produced.len() != outputs.len() {
                return Err("aggregate output has wrong width".into());
            }
            let mut row = values;
            row.extend(produced);
            rows.push(row);
        }
        let mut result = Self::from_optional(columns, rows)?;
        for (actual, &index) in result
            .column_types
            .iter_mut()
            .take(indices.len())
            .zip(&indices)
        {
            if let Some(known) = &self.column_types[index] {
                merge_type(actual, known.clone())?;
            }
        }
        Ok(result)
    }

    pub fn union(&self, others: &[Self], mode: &str) -> Result<Self, String> {
        if !matches!(mode, "error" | "all" | "common") {
            return Err(format!("invalid table union mode: {mode}"));
        }
        let tables = std::iter::once(self).chain(others);
        let mut columns = self.columns.clone();
        let mut size = 0usize;
        for table in tables.clone() {
            size = size
                .checked_add(table.rows.len())
                .ok_or("table union exceeds resource limit")?;
            if size > 1_000_000 {
                return Err("table union exceeds resource limit".into());
            }
            if mode == "error"
                && (table.columns.len() != self.columns.len()
                    || table
                        .columns
                        .iter()
                        .any(|name| !self.columns.contains(name)))
            {
                return Err("table union columns must match".into());
            }
            if mode == "all" {
                for name in &table.columns {
                    if !columns.contains(name) {
                        columns.push(name.clone());
                    }
                }
            }
        }
        if mode == "common" {
            columns.retain(|name| tables.clone().all(|table| table.columns.contains(name)));
        }
        if columns.len() > 1024 {
            return Err("table union exceeds resource limit".into());
        }
        let mut column_types = vec![None; columns.len()];
        let mut rows = Vec::with_capacity(size);
        for table in tables {
            for (index, name) in columns.iter().enumerate() {
                if let Some(position) = table.columns.iter().position(|column| column == name)
                    && let Some(kind) = &table.column_types[position]
                {
                    merge_type(&mut column_types[index], kind.clone())
                        .map_err(|_| format!("incompatible table column: {name}"))?;
                }
            }
            let positions = columns
                .iter()
                .map(|name| table.columns.iter().position(|column| column == name))
                .collect::<Vec<_>>();
            for row in &table.rows {
                rows.push(
                    positions
                        .iter()
                        .map(|position| position.and_then(|index| row[index].clone()))
                        .collect(),
                );
            }
        }
        let mut result = Self::from_optional(columns, rows)?;
        for (actual, expected) in result.column_types.iter_mut().zip(column_types) {
            if let Some(expected) = expected {
                merge_type(actual, expected)?;
            }
        }
        Ok(result)
    }

    pub fn sort(&self, keys: &[(String, SortOrder)]) -> Result<Self, String> {
        let rank_work = keys.iter().try_fold(0usize, |work, (_, order)| {
            let size = match order {
                SortOrder::InOrder(values) => self.rows.len().checked_mul(values.len())?,
                _ => 0,
            };
            work.checked_add(size)
        });
        if keys.is_empty()
            || keys.len() > 1024
            || self
                .rows
                .len()
                .checked_mul(keys.len())
                .is_none_or(|size| size > 1_000_000)
            || rank_work.is_none_or(|size| size > 1_000_000)
            || keys.iter().any(
                |(_, order)| matches!(order, SortOrder::InOrder(values) if values.len() > 1024),
            )
        {
            return Err("table sort exceeds resource limit or has no keys".into());
        }
        let keys = keys
            .iter()
            .map(|(name, order)| {
                self.columns
                    .iter()
                    .position(|column| column == name)
                    .map(|index| (index, order))
                    .ok_or_else(|| format!("unknown table column: {name}"))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let clock = chrono::Local::now();
        let mut ranks = Vec::with_capacity(keys.len());
        for &(index, key) in &keys {
            let ranked = match key {
                SortOrder::InOrder(values) => values.as_slice(),
                _ => &[],
            };
            let mut reference = None;
            // Supported scalar compatibility is transitive; check ranks too, even without rows.
            for cell in self.rows.iter().map(|row| &row[index]).chain(ranked) {
                if cell.is_some() {
                    let first = reference.get_or_insert(cell);
                    compare_cells(first, cell, clock)?;
                }
            }
            ranks.push(if matches!(key, SortOrder::InOrder(_)) {
                self.rows
                    .iter()
                    .map(|row| ranked.iter().position(|value| value == &row[index]))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            });
        }
        let compare = |left: usize, right: usize| -> Result<Ordering, String> {
            for (position, &(index, key)) in keys.iter().enumerate() {
                let a = &self.rows[left][index];
                let b = &self.rows[right][index];
                let compared = match key {
                    SortOrder::InOrder(_) => {
                        match (ranks[position][left], ranks[position][right]) {
                            (Some(a), Some(b)) => a.cmp(&b),
                            (Some(_), None) => Ordering::Less,
                            (None, Some(_)) => Ordering::Greater,
                            (None, None) => compare_cells(a, b, clock)?,
                        }
                    }
                    SortOrder::Asc => compare_cells(a, b, clock)?,
                    SortOrder::Desc => {
                        let value = compare_cells(a, b, clock)?;
                        if a.is_none() || b.is_none() {
                            value
                        } else {
                            value.reverse()
                        }
                    }
                };
                if !compared.is_eq() {
                    return Ok(compared);
                }
            }
            Ok(Ordering::Equal)
        };
        // Merge indices stably with O(n log n) fallible comparisons and O(n) scratch space.
        let mut order: Vec<_> = (0..self.rows.len()).collect();
        let mut scratch = Vec::with_capacity(order.len());
        let mut width = 1;
        while width < order.len() {
            scratch.clear();
            for start in (0..order.len()).step_by(width * 2) {
                let middle = (start + width).min(order.len());
                let end = (middle + width).min(order.len());
                let (mut left, mut right) = (start, middle);
                while left < middle && right < end {
                    if compare(order[left], order[right])?.is_gt() {
                        scratch.push(order[right]);
                        right += 1;
                    } else {
                        scratch.push(order[left]);
                        left += 1;
                    }
                }
                scratch.extend_from_slice(&order[left..middle]);
                scratch.extend_from_slice(&order[right..end]);
            }
            std::mem::swap(&mut order, &mut scratch);
            width *= 2;
        }
        let mut sorted = self.clone();
        sorted.rows = order
            .into_iter()
            .map(|index| self.rows[index].clone())
            .collect();
        Ok(sorted)
    }

    pub fn rename(&self, changes: &[(String, String)]) -> Result<Self, String> {
        let mut renamed = self.clone();
        let mut sources = HashSet::new();
        for (from, to) in changes {
            if !sources.insert(from) {
                return Err(format!("duplicate rename source: {from}"));
            }
            let index = self
                .columns
                .iter()
                .position(|name| name == from)
                .ok_or_else(|| format!("unknown table column: {from}"))?;
            renamed.columns[index] = to.clone();
        }
        let mut names = HashSet::new();
        if renamed.columns.iter().any(|name| !names.insert(name)) {
            return Err("duplicate table column after rename".into());
        }
        Ok(renamed)
    }

    pub fn distinct(&self) -> Self {
        let mut unique = self.clone();
        unique.rows.clear();
        for row in &self.rows {
            if !unique.rows.contains(row) {
                unique.rows.push(row.clone());
            }
        }
        unique
    }

    pub fn with_column(&self, name: &str, values: Vec<Option<Cell>>) -> Result<Self, String> {
        if values.len() != self.rows.len() {
            return Err("table column length must equal row count".into());
        }
        let mut columns = self.columns.clone();
        let index = if let Some(index) = columns.iter().position(|column| column == name) {
            index
        } else {
            columns.push(name.into());
            columns.len() - 1
        };
        let rows = self
            .rows
            .iter()
            .zip(values)
            .map(|(row, value)| {
                let mut row = row.clone();
                if index == row.len() {
                    row.push(value);
                } else {
                    row[index] = value;
                }
                row
            })
            .collect();
        Self::from_optional(columns, rows)
    }

    pub fn row_dict(&self, index: usize) -> Result<BTreeMap<String, Value>, String> {
        let row = self
            .rows
            .get(index)
            .ok_or("table row index out of bounds")?;
        self.columns
            .iter()
            .zip(row)
            .map(|(name, cell)| {
                let value = cell
                    .as_ref()
                    .map_or(Ok(Value::Null), serde_json::to_value)
                    .map_err(|error| error.to_string())?;
                Ok((name.clone(), value))
            })
            .collect()
    }

    pub fn row_dicts(&self) -> Result<Vec<BTreeMap<String, Value>>, String> {
        (0..self.rows.len())
            .map(|index| self.row_dict(index))
            .collect()
    }

    pub fn project<T: serde::de::DeserializeOwned>(&self, column: &str) -> Result<Vec<T>, String> {
        let position = self
            .columns
            .iter()
            .position(|name| name == column)
            .ok_or_else(|| format!("unknown table column: {column}"))?;
        self.rows
            .iter()
            .map(|row| {
                let value = row[position]
                    .as_ref()
                    .map_or(Ok(Value::Null), serde_json::to_value)
                    .map_err(|error| error.to_string())?;
                serde_json::from_value(value)
                    .map_err(|error| format!("invalid table column {column}: {error}"))
            })
            .collect()
    }

    pub fn to_dict(&self) -> Result<BTreeMap<String, Value>, String> {
        if self.rows.len() != 1 {
            return Err("toDict requires exactly one table row".into());
        }
        self.row_dict(0)
    }

    pub fn to_value<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        if self.rows.len() != 1 || self.columns.len() != 1 {
            return Err("toValue requires one table cell".into());
        }
        let value = self
            .row_dict(0)?
            .into_values()
            .next()
            .ok_or("missing table cell")?;
        serde_json::from_value(value).map_err(|error| format!("invalid table cell: {error}"))
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }
    pub fn column_types(&self) -> &[Option<CellType>] {
        &self.column_types
    }
    pub fn rows(&self) -> &[Vec<Option<Cell>>] {
        &self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn table_enum_cells_preserve_type_identity_while_serializing_as_strings() {
        let status = Cell::Enum {
            kind: "Status".into(),
            variant: "ready".into(),
        };
        let table = Table::new(vec!["state".into()], vec![vec![status.clone()]]).unwrap();
        assert_eq!(
            table.column_types(),
            &[Some(CellType::Enum("Status".into()))]
        );
        assert_eq!(
            serde_json::to_value(table).unwrap(),
            serde_json::json!({"columns":["state"],"rows":[{"state":"ready"}]})
        );
        for other in [
            Cell::from("ready"),
            Cell::Enum {
                kind: "Phase".into(),
                variant: "ready".into(),
            },
        ] {
            assert!(
                Table::new(
                    vec!["state".into()],
                    vec![vec![status.clone()], vec![other]]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn table_preserves_nullable_cells_on_round_trip() {
        let table = Table {
            columns: vec!["missing".into()],
            column_types: vec![None],
            rows: vec![vec![None]],
        };
        let json = serde_json::to_value(table).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"columns": ["missing"], "rows": [{"missing": null}]})
        );
        let decoded = serde_json::from_value::<Table>(json).unwrap();
        assert_eq!(decoded.rows()[0][0], None);
        assert_eq!(decoded.column_types(), &[None]);
        assert!(Table::new(vec!["missing".into()], vec![vec![Cell::Null]]).is_err());
        let nested: Table = serde_json::from_value(
            serde_json::json!({"columns":["details"],"rows":[{"details":{"optional":null}}]}),
        )
        .unwrap();
        assert!(
            matches!(nested.rows()[0][0], Some(Cell::Dictionary(ref fields)) if fields.get("optional") == Some(&Cell::Null))
        );
    }

    #[test]
    fn table_from_optional_normalizes_only_top_level_nulls() {
        let nested_list = Cell::List(vec![Cell::Null, Cell::from(Decimal::ONE)]);
        let nested_dict = Cell::Dictionary(BTreeMap::from([("missing".into(), Cell::Null)]));
        let table = Table::from_optional(
            vec!["key".into(), "list".into(), "dict".into()],
            vec![vec![
                Some(Cell::Null),
                Some(nested_list.clone()),
                Some(nested_dict.clone()),
            ]],
        )
        .unwrap();
        assert_eq!(
            table.rows()[0],
            vec![None, Some(nested_list), Some(nested_dict)]
        );
        assert_eq!(table.column_types()[0], None);
        let decoded: Table = serde_json::from_value(serde_json::to_value(&table).unwrap()).unwrap();
        assert_eq!(table, decoded);
        let replaced = table.with_column("key", vec![Some(Cell::Null)]).unwrap();
        assert_eq!(replaced.rows()[0][0], None);
    }

    #[test]
    fn table_optional_null_join_is_unchanged_by_json_round_trip() {
        let table = Table::from_optional(vec!["key".into()], vec![vec![Some(Cell::Null)]]).unwrap();
        let decoded: Table = serde_json::from_value(serde_json::to_value(&table).unwrap()).unwrap();
        for source in [&table, &decoded] {
            assert!(
                source
                    .join(source, &["key".into()], "inner")
                    .unwrap()
                    .rows()
                    .is_empty()
            );
            assert_eq!(
                source
                    .join(source, &["key".into()], "outer")
                    .unwrap()
                    .rows()
                    .len(),
                2
            );
        }
    }

    #[test]
    fn table_explicit_rows_keep_order_and_infer_types() {
        let table = Table::new(
            vec!["region".into(), "rate".into()],
            vec![
                vec![Cell::from("domestic"), Cell::from(Decimal::new(599, 2))],
                vec![Cell::from("europe"), Cell::from(Decimal::new(1599, 2))],
            ],
        )
        .unwrap();
        assert_eq!(table.columns(), &["region", "rate"]);
        assert_eq!(
            table.column_types(),
            &[Some(CellType::String), Some(CellType::Number)]
        );
        assert_eq!(table.rows()[0][1], Some(Cell::from(Decimal::new(599, 2))));
        assert_eq!(table.rows()[1][0], Some(Cell::from("europe")));
    }

    #[test]
    fn table_empty_schemas_keep_names_without_types() {
        let named = Table::new(vec!["rate".into(), "region".into()], vec![]).unwrap();
        assert_eq!(named.columns(), &["rate", "region"]);
        assert_eq!(named.column_types(), &[None, None]);
        assert!(named.rows().is_empty());
        let empty = Table::new(vec![], vec![]).unwrap();
        assert!(empty.columns().is_empty());
        assert!(empty.column_types().is_empty());
        assert!(empty.rows().is_empty());
    }

    #[test]
    fn table_rejects_duplicate_headers_and_wrong_width_or_mixed_columns() {
        assert!(Table::new(vec!["a".into(), "a".into()], vec![]).is_err());
        assert!(Table::new(vec![], vec![vec![Cell::from("extra")]]).is_err());
        assert!(Table::new(vec!["a".into()], vec![vec![]]).is_err());
        assert!(
            Table::new(
                vec!["a".into()],
                vec![vec![Cell::from("x")], vec![Cell::from(Decimal::ONE)]]
            )
            .is_err()
        );
    }

    #[test]
    fn table_clone_does_not_mutate_original_rows_or_schema() {
        let original = Table::new(vec!["a".into()], vec![vec![Cell::from("before")]]).unwrap();
        let cloned = original.clone();
        let mut extracted = cloned.rows().to_vec();
        extracted[0][0] = Some(Cell::from("after"));
        assert_eq!(original.columns(), &["a"]);
        assert_eq!(original.rows()[0][0], Some(Cell::from("before")));
        assert_eq!(cloned.rows()[0][0], Some(Cell::from("before")));
    }

    #[test]
    fn table_infers_nested_list_types_and_rejects_incompatible_elements() {
        let table = Table::new(
            vec!["list".into()],
            vec![
                vec![Cell::List(vec![])],
                vec![Cell::List(vec![Cell::from(Decimal::ONE)])],
            ],
        )
        .unwrap();
        assert_eq!(
            table.column_types(),
            &[Some(CellType::List(Box::new(Some(CellType::Number))))]
        );
        assert!(
            Table::new(
                vec!["list".into()],
                vec![
                    vec![Cell::List(vec![Cell::from(Decimal::ONE)])],
                    vec![Cell::List(vec![Cell::from("different")])],
                ]
            )
            .is_err()
        );
        assert!(
            Table::new(
                vec!["list".into()],
                vec![vec![Cell::List(vec![
                    Cell::from(true),
                    Cell::from(Decimal::ONE)
                ])],]
            )
            .is_err()
        );
    }

    #[test]
    fn table_from_dicts_adopts_uniform_keys_and_canonical_order() {
        let rows = vec![
            BTreeMap::from([
                ("rate".into(), Cell::from(Decimal::new(599, 2))),
                ("region".into(), Cell::from("eu")),
            ]),
            BTreeMap::from([
                ("rate".into(), Cell::from(Decimal::new(899, 2))),
                ("region".into(), Cell::from("us")),
            ]),
        ];
        let table = Table::from_dicts(rows.clone()).unwrap();
        assert_eq!(table.columns(), &["rate", "region"]);
        assert_eq!(
            table.column_types(),
            &[Some(CellType::Number), Some(CellType::String)]
        );
        assert_eq!(table.rows()[1][1], Some(Cell::from("us")));
        assert_eq!(rows[0]["region"], Cell::from("eu"));
        assert!(
            Table::from_dicts(vec![
                rows[0].clone(),
                BTreeMap::from([("rate".into(), Cell::from(Decimal::ONE))])
            ])
            .is_err()
        );
        assert!(
            Table::from_dicts(vec![
                rows[0].clone(),
                BTreeMap::from([
                    ("rate".into(), Cell::from("bad")),
                    ("region".into(), Cell::from("us"))
                ])
            ])
            .is_err()
        );
        assert!(Table::from_dicts(vec![]).unwrap().columns().is_empty());
    }

    #[test]
    fn table_keeps_temporal_types_distinct_from_strings_and_numbers_exact() {
        let date: crate::temporal::Date = "2026-10-02".parse().unwrap();
        let table = Table::new(
            vec!["date".into(), "money".into()],
            vec![vec![
                Cell::from(date),
                Cell::from(Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()),
            ]],
        )
        .unwrap();
        assert_eq!(
            table.column_types(),
            &[Some(CellType::Date), Some(CellType::Number)]
        );
        assert_eq!(
            table.rows()[0][1],
            Some(Cell::from(
                Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
            ))
        );
        assert!(
            Table::new(
                vec!["date".into()],
                vec![vec![Cell::from(date)], vec![Cell::from("2026-10-02")]]
            )
            .is_err()
        );
    }

    #[test]
    fn table_aggregate_preserves_first_seen_groups_and_output_order() {
        let source = Table::from_optional(
            vec!["region".into(), "rate".into()],
            vec![
                vec![Some(Cell::from("b")), Some(Cell::from(Decimal::from(10)))],
                vec![Some(Cell::from("a")), None],
                vec![Some(Cell::from("b")), Some(Cell::from(Decimal::from(20)))],
            ],
        )
        .unwrap();
        let grouped = source
            .aggregate(&["region".into()], &["count".into()], |group| {
                Ok(vec![Some(Cell::from(Decimal::from(
                    group.rows().len() as u64
                )))])
            })
            .unwrap();
        assert_eq!(grouped.columns(), &["region", "count"]);
        assert_eq!(grouped.rows().len(), 2);
        assert_eq!(
            grouped.rows()[0],
            &[Some(Cell::from("b")), Some(Cell::from(Decimal::from(2)))]
        );
        assert_eq!(
            grouped.rows()[1],
            &[Some(Cell::from("a")), Some(Cell::from(Decimal::ONE))]
        );
        assert_eq!(source.rows().len(), 3);
        assert!(
            source
                .aggregate(&[], &["count".into()], |_| Ok(vec![Some(Cell::from(
                    Decimal::ONE
                ))]))
                .is_err()
        );
        assert!(
            source
                .aggregate(&["region".into()], &["region".into()], |_| Ok(vec![Some(
                    Cell::from(Decimal::ONE)
                )]))
                .is_err()
        );
        let empty = Table::new(vec!["region".into()], vec![]).unwrap();
        assert_eq!(
            empty
                .aggregate(&["region".into()], &["n".into()], |_| Ok(vec![Some(
                    Cell::from(Decimal::ONE)
                )]))
                .unwrap()
                .columns(),
            &["region", "n"]
        );
    }

    #[test]
    fn table_join_multikey_outer_preserves_duplicates_and_marks_missing_cells() {
        let left = Table::new(
            vec!["id".into(), "region".into(), "amount".into()],
            vec![
                vec![
                    Cell::from(Decimal::ONE),
                    Cell::from("eu"),
                    Cell::from(Decimal::from(10)),
                ],
                vec![
                    Cell::from(Decimal::ONE),
                    Cell::from("eu"),
                    Cell::from(Decimal::from(20)),
                ],
                vec![
                    Cell::from(Decimal::from(2)),
                    Cell::from("us"),
                    Cell::from(Decimal::from(30)),
                ],
            ],
        )
        .unwrap();
        let right = Table::new(
            vec!["region".into(), "id".into(), "fee".into()],
            vec![
                vec![
                    Cell::from("eu"),
                    Cell::from(Decimal::ONE),
                    Cell::from(Decimal::from(1)),
                ],
                vec![
                    Cell::from("eu"),
                    Cell::from(Decimal::ONE),
                    Cell::from(Decimal::from(2)),
                ],
                vec![
                    Cell::from("other"),
                    Cell::from(Decimal::from(3)),
                    Cell::from(Decimal::from(3)),
                ],
            ],
        )
        .unwrap();
        let joined = left
            .join(&right, &["region".into(), "id".into()], "outer")
            .unwrap();
        assert_eq!(joined.columns(), &["region", "id", "amount", "fee"]);
        assert_eq!(joined.rows().len(), 6);
        assert_eq!(joined.rows()[0][2], Some(Cell::from(Decimal::from(10))));
        assert_eq!(joined.rows()[3][3], Some(Cell::from(Decimal::from(2))));
        assert_eq!(joined.rows()[4][3], None);
        assert_eq!(joined.rows()[5][2], None);
        assert_eq!(joined.rows()[5][0], Some(Cell::from("other")));
        assert_eq!(left.rows().len(), 3);
        assert!(left.join(&right, &[], "inner").is_err());
        assert!(left.join(&right, &["id".into()], "cross").is_err());
        assert!(left.join(&right, &["missing".into()], "inner").is_err());
        assert!(left.join(&right, &["id".into()], "unknown").is_err());
    }

    #[test]
    fn table_join_reports_incompatible_temporal_key_zones() {
        let naive: Date = "2026-10-02".parse().unwrap();
        let zoned: Date = "2026-10-02Z".parse().unwrap();
        let left = Table::new(vec!["day".into()], vec![vec![naive.into()]]).unwrap();
        let right = Table::new(vec!["day".into()], vec![vec![zoned.into()]]).unwrap();
        assert!(left.join(&right, &["day".into()], "inner").is_err());
    }

    #[test]
    fn table_join_null_keys_do_not_match() {
        let left = Table::from_optional(
            vec!["id".into(), "left".into()],
            vec![vec![None, Some(Cell::from(Decimal::ONE))]],
        )
        .unwrap();
        let right = Table::from_optional(
            vec!["id".into(), "right".into()],
            vec![vec![None, Some(Cell::from(Decimal::from(2)))]],
        )
        .unwrap();
        assert!(
            left.join(&right, &["id".into()], "inner")
                .unwrap()
                .rows()
                .is_empty()
        );
        assert_eq!(
            left.join(&right, &["id".into()], "outer")
                .unwrap()
                .rows()
                .len(),
            2
        );
    }

    #[test]
    fn table_cross_join_bounds_cartesian_growth_and_checks_collisions() {
        let left = Table::new(vec!["id".into()], vec![vec![Cell::from(Decimal::ONE)]; 2]).unwrap();
        let right = Table::new(vec!["region".into()], vec![vec![Cell::from("eu")]; 3]).unwrap();
        assert_eq!(left.join(&right, &[], "cross").unwrap().rows().len(), 6);
        assert!(left.join(&left, &[], "cross").is_err());
        let big_left = Table::new(
            vec!["id".into()],
            vec![vec![Cell::from(Decimal::ONE)]; 1001],
        )
        .unwrap();
        let big_right =
            Table::new(vec!["region".into()], vec![vec![Cell::from("eu")]; 1001]).unwrap();
        assert!(big_left.join(&big_right, &[], "cross").is_err());
        let unmatched = Table::new(
            vec!["id".into()],
            vec![vec![Cell::from(Decimal::from(2))]; 1001],
        )
        .unwrap();
        assert!(big_left.join(&unmatched, &["id".into()], "inner").is_err());
    }

    #[test]
    fn table_union_preserves_duplicates_and_modes_without_mutation() {
        let first = Table::new(
            vec!["id".into(), "price".into()],
            vec![vec![
                Cell::from(Decimal::ONE),
                Cell::from(Decimal::from(10)),
            ]],
        )
        .unwrap();
        let duplicate = first.clone();
        let strict = first.union(&[duplicate], "error").unwrap();
        assert_eq!(strict.rows().len(), 2);
        let reversed = Table::new(
            vec!["price".into(), "id".into()],
            vec![vec![
                Cell::from(Decimal::from(20)),
                Cell::from(Decimal::from(2)),
            ]],
        )
        .unwrap();
        let reordered = first.union(&[reversed], "error").unwrap();
        assert_eq!(reordered.columns(), &["id", "price"]);
        assert_eq!(
            reordered.rows()[1],
            &[
                Some(Cell::from(Decimal::from(2))),
                Some(Cell::from(Decimal::from(20)))
            ]
        );
        let second = Table::new(
            vec!["id".into(), "other".into()],
            vec![vec![Cell::from(Decimal::from(2)), Cell::from("new")]],
        )
        .unwrap();
        let all = first.union(std::slice::from_ref(&second), "all").unwrap();
        assert_eq!(all.columns(), &["id", "price", "other"]);
        assert_eq!(all.rows()[0][2], None);
        assert_eq!(all.rows()[1][1], None);
        let shared = first.union(&[second], "common").unwrap();
        assert_eq!(shared.columns(), &["id"]);
        assert_eq!(shared.rows().len(), 2);
        assert_eq!(first.columns(), &["id", "price"]);
        assert!(first.union(&[], "error").unwrap() == first);
        assert!(first.union(std::slice::from_ref(&all), "error").is_err());
        assert!(first.union(&[all], "unknown").is_err());
        let bad = Table::new(vec!["id".into()], vec![vec![Cell::from("wrong")]]).unwrap();
        assert!(first.union(&[bad], "common").is_err());
    }

    #[test]
    fn table_sort_is_stable_multikey_and_ranked_with_unranked_fallback() {
        let input = Table::new(
            vec!["region".into(), "rate".into()],
            vec![
                vec![Cell::from("b"), Cell::from(Decimal::from(2))],
                vec![Cell::from("a"), Cell::from(Decimal::from(3))],
                vec![Cell::from("b"), Cell::from(Decimal::from(2))],
                vec![Cell::from("a"), Cell::from(Decimal::from(1))],
            ],
        )
        .unwrap();
        let sorted = input
            .sort(&[
                ("region".into(), SortOrder::Asc),
                ("rate".into(), SortOrder::Desc),
            ])
            .unwrap();
        assert_eq!(sorted.rows()[0][1], Some(Cell::from(Decimal::from(3))));
        assert_eq!(sorted.rows()[1][1], Some(Cell::from(Decimal::ONE)));
        assert_eq!(sorted.rows()[2], sorted.rows()[3]);
        let ranked = input
            .sort(&[(
                "region".into(),
                SortOrder::InOrder(vec![Some(Cell::from("b"))]),
            )])
            .unwrap();
        assert_eq!(ranked.rows()[0][0], Some(Cell::from("b")));
        assert_eq!(ranked.rows()[2][0], Some(Cell::from("a")));
        assert_eq!(input.rows()[0][0], Some(Cell::from("b")));
        assert!(input.sort(&[]).is_err());
        assert!(input.sort(&[("missing".into(), SortOrder::Asc)]).is_err());
        assert!(
            input
                .sort(&vec![("rate".into(), SortOrder::Asc); 1025])
                .is_err()
        );
        assert!(
            input
                .sort(&[(
                    "region".into(),
                    SortOrder::InOrder(vec![Some(Cell::from("b")); 1025])
                )])
                .is_err()
        );
    }

    #[test]
    fn table_sort_rejects_incompatible_ranked_values_including_unused_ranks() {
        let naive: Date = "2026-10-02".parse().unwrap();
        let zoned: Date = "2026-10-02Z".parse().unwrap();
        for (values, ranks) in [
            (
                vec![naive.into(), zoned.into()],
                vec![naive.into(), zoned.into()],
            ),
            (vec![naive.into()], vec![zoned.into()]),
            (vec![], vec![naive.into(), zoned.into()]),
        ] {
            let table = Table::new(
                vec!["day".into()],
                values.into_iter().map(|value| vec![value]).collect(),
            )
            .unwrap();
            let result = table.sort(&[(
                "day".into(),
                SortOrder::InOrder(ranks.into_iter().map(Some).collect()),
            )]);
            assert!(matches!(result, Err(message) if message.contains("temporal zones")));
        }
        let empty = Table::new(vec!["key".into()], vec![]).unwrap();
        assert!(
            empty
                .sort(&[(
                    "key".into(),
                    SortOrder::InOrder(vec![Some(Cell::from("a")), Some(Cell::from(true))]),
                )])
                .is_err()
        );
    }

    #[test]
    fn table_sort_checks_compatibility_beyond_the_first_key() {
        let naive: Date = "2026-10-02".parse().unwrap();
        let zoned: Date = "2026-10-02Z".parse().unwrap();
        let table = Table::new(
            vec!["id".into(), "day".into()],
            vec![
                vec![Cell::from("a"), naive.into()],
                vec![Cell::from("b"), zoned.into()],
            ],
        )
        .unwrap();
        assert!(
            table
                .sort(&[
                    ("id".into(), SortOrder::Asc),
                    ("day".into(), SortOrder::Desc)
                ])
                .is_err()
        );
    }

    #[test]
    fn table_sort_bounds_cumulative_rank_work() {
        let table = Table::new(vec!["key".into()], vec![vec![Cell::from("a")]; 1000]).unwrap();
        let key = (
            "key".into(),
            SortOrder::InOrder(vec![Some(Cell::from("a")); 600]),
        );
        assert!(
            matches!(table.sort(&[key.clone(), key]), Err(message) if message.contains("resource limit"))
        );
    }

    #[test]
    fn table_sort_preserves_ties_null_order_and_first_rank_occurrence() {
        let table = Table::from_optional(
            vec!["key".into(), "id".into()],
            vec![
                vec![Some(Cell::from("b")), Some(Cell::from("first"))],
                vec![None, Some(Cell::from("null-first"))],
                vec![Some(Cell::from("a")), Some(Cell::from("a"))],
                vec![Some(Cell::from("b")), Some(Cell::from("last"))],
                vec![None, Some(Cell::from("null-last"))],
                vec![Some(Cell::from("c")), Some(Cell::from("c"))],
            ],
        )
        .unwrap();
        for (order, expected) in [
            (
                SortOrder::Asc,
                vec!["a", "first", "last", "c", "null-first", "null-last"],
            ),
            (
                SortOrder::Desc,
                vec!["c", "first", "last", "a", "null-first", "null-last"],
            ),
            (
                SortOrder::InOrder(vec![
                    None,
                    Some(Cell::from("b")),
                    None,
                    Some(Cell::from("a")),
                    Some(Cell::from("b")),
                ]),
                vec!["null-first", "null-last", "first", "last", "a", "c"],
            ),
        ] {
            let sorted = table.sort(&[("key".into(), order)]).unwrap();
            assert_eq!(sorted.project::<String>("id").unwrap(), expected);
        }
    }

    #[test]
    fn table_sort_accepts_more_rows_than_the_list_sort_limit() {
        let table = Table::new(
            vec!["key".into()],
            (0..4096)
                .rev()
                .map(|value| vec![Cell::from(Decimal::from(value))])
                .collect(),
        )
        .unwrap();
        let sorted = table.sort(&[("key".into(), SortOrder::Asc)]).unwrap();
        assert_eq!(sorted.rows().len(), 4096);
        assert_eq!(sorted.rows()[0][0], Some(Cell::from(Decimal::ZERO)));
        assert_eq!(
            sorted.rows()[4095][0],
            Some(Cell::from(Decimal::from(4095)))
        );
        assert_eq!(table.rows()[0][0], Some(Cell::from(Decimal::from(4095))));
    }

    #[test]
    fn table_sort_ranking_matches_equal_temporal_instants() {
        let earlier: DateTime = "2026-10-01T00:00:00Z".parse().unwrap();
        let actual: DateTime = "2026-10-02T10:00:00Z".parse().unwrap();
        let ranked: DateTime = "2026-10-02T12:00:00+02:00".parse().unwrap();
        let table = Table::new(
            vec!["time".into()],
            vec![vec![earlier.into()], vec![actual.into()]],
        )
        .unwrap();
        let sorted = table
            .sort(&[("time".into(), SortOrder::InOrder(vec![Some(ranked.into())]))])
            .unwrap();
        assert_eq!(sorted.rows()[0][0], Some(actual.into()));
    }

    #[test]
    fn table_sort_rejects_incompatible_temporal_zones() {
        let naive: Date = "2026-10-02".parse().unwrap();
        let zoned: Date = "2026-10-02Z".parse().unwrap();
        let typed = Table::new(
            vec!["day".into()],
            vec![vec![naive.into()], vec![zoned.into()]],
        )
        .unwrap();
        assert!(typed.sort(&[("day".into(), SortOrder::Asc)]).is_err());
    }

    #[test]
    fn table_rename_swaps_without_mutating_and_rejects_collisions() {
        let original = Table::new(
            vec!["a".into(), "b".into()],
            vec![vec![Cell::from("left"), Cell::from("right")]],
        )
        .unwrap();
        let swapped = original
            .rename(&[("a".into(), "b".into()), ("b".into(), "a".into())])
            .unwrap();
        assert_eq!(swapped.columns(), &["b", "a"]);
        assert_eq!(swapped.rows(), original.rows());
        assert_eq!(original.columns(), &["a", "b"]);
        assert!(original.rename(&[("missing".into(), "c".into())]).is_err());
        assert!(original.rename(&[("a".into(), "b".into())]).is_err());
        assert!(
            original
                .rename(&[("a".into(), "c".into()), ("a".into(), "d".into())])
                .is_err()
        );
    }

    #[test]
    fn table_distinct_and_with_column_preserve_order_and_nulls() {
        let original = Table::from_optional(
            vec!["rate".into()],
            vec![
                vec![None],
                vec![Some(Cell::from(Decimal::from(20)))],
                vec![None],
            ],
        )
        .unwrap();
        let unique = original.distinct();
        assert_eq!(unique.rows().len(), 2);
        assert_eq!(unique.rows()[0][0], None);
        assert_eq!(unique.rows()[1][0], Some(Cell::from(Decimal::from(20))));
        let appended = unique
            .with_column("tax", vec![None, Some(Cell::from(Decimal::from(2)))])
            .unwrap();
        assert_eq!(appended.columns(), &["rate", "tax"]);
        assert_eq!(appended.rows()[0], &[None, None]);
        let replaced = appended
            .with_column("rate", vec![None, Some(Cell::from(Decimal::from(10)))])
            .unwrap();
        assert_eq!(replaced.columns(), &["rate", "tax"]);
        assert_eq!(replaced.rows()[1][0], Some(Cell::from(Decimal::from(10))));
        assert_eq!(original.rows().len(), 3);
        assert!(original.with_column("tax", vec![None]).is_err());
        assert!(
            original
                .with_column(
                    "tax",
                    vec![
                        Some(Cell::from("bad")),
                        Some(Cell::from(Decimal::ONE)),
                        None
                    ]
                )
                .is_err()
        );
    }
}
