use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::cmp::Ordering;

pub fn modes<T: PartialEq + Clone>(values: &[T]) -> Result<Vec<T>, String> {
    modes_checked(values, |left, right| Ok(left == right))
}

pub fn modes_checked<T: Clone>(
    values: &[T],
    mut equal: impl FnMut(&T, &T) -> Result<bool, String>,
) -> Result<Vec<T>, String> {
    if values.is_empty() {
        return Err("mode requires values".into());
    }
    // shortcut: bounded quadratic comparisons; use a fallible ordered or hashed algorithm for larger lists.
    if values.len() > 1024 {
        return Err("mode list too large".into());
    }
    let mut winners = Vec::new();
    let mut maximum = 0;
    for (index, value) in values.iter().enumerate() {
        if values[..index].iter().try_fold(false, |found, previous| {
            equal(value, previous).map(|same| found || same)
        })? {
            continue;
        }
        let count = values.iter().try_fold(0usize, |count, other| {
            equal(value, other).map(|same| count + usize::from(same))
        })?;
        if count > maximum {
            winners.clear();
            maximum = count;
        }
        if count == maximum {
            winners.push(value.clone());
        }
    }
    Ok(winners)
}

pub fn extreme<T: Clone>(
    values: &[T],
    maximum: bool,
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, String>,
) -> Result<T, String> {
    let first = values.first().ok_or("min/max requires values")?;
    let mut selected = first;
    for value in &values[1..] {
        let order = compare(value, selected)?;
        if if maximum {
            order.is_gt()
        } else {
            order.is_lt()
        } {
            selected = value;
        }
    }
    Ok(selected.clone())
}

// shortcut: at most 1024 combined values and priorities bound quadratic comparisons; use a bounded-work fallible sort if larger lists are required.
pub fn sort_checked<T>(
    values: Vec<T>,
    priorities: Option<&[T]>,
    descending: bool,
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, String>,
) -> Result<Vec<T>, String> {
    const MAX_SORT_ITEMS: usize = 1024;
    if values.len() > MAX_SORT_ITEMS
        || priorities.is_some_and(|items| items.len() > MAX_SORT_ITEMS - values.len())
    {
        return Err("sort list and priorities too large".into());
    }
    // Check all pairs, including priority-only pairs: insertion order need not compare them.
    let all: Vec<_> = values
        .iter()
        .chain(priorities.into_iter().flatten())
        .collect();
    for (index, left) in all.iter().enumerate() {
        for right in &all[index + 1..] {
            compare(left, right)?;
        }
    }
    let mut ranks = Vec::with_capacity(values.len());
    for value in &values {
        let mut rank = None;
        if let Some(priorities) = priorities {
            for (index, item) in priorities.iter().enumerate() {
                if compare(value, item)?.is_eq() {
                    rank = Some(index);
                    break;
                }
            }
        }
        ranks.push(rank);
    }
    let mut sorted: Vec<(Option<usize>, T)> = Vec::with_capacity(values.len());
    for (rank, value) in ranks.into_iter().zip(values) {
        let mut index = sorted.len();
        while index > 0 {
            let (previous_rank, previous) = &sorted[index - 1];
            let order = match (rank, *previous_rank) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) if descending => compare(previous, &value)?,
                (None, None) => compare(&value, previous)?,
            };
            if !order.is_lt() {
                break;
            }
            index -= 1;
        }
        sorted.insert(index, (rank, value));
    }
    Ok(sorted.into_iter().map(|(_, value)| value).collect())
}

pub fn position(len: usize, start: Decimal) -> Result<usize, String> {
    if start.fract() != Decimal::ZERO || start == Decimal::ZERO {
        return Err("invalid list position".into());
    }
    let size = Decimal::from(len);
    let offset = if start > Decimal::ZERO {
        start - Decimal::ONE
    } else {
        size + start
    };
    offset
        .to_usize()
        .filter(|&index| index < len)
        .ok_or("list position out of bounds".into())
}

pub fn sublist<T: Clone>(
    items: &[T],
    start: Decimal,
    length: Option<Decimal>,
) -> Result<Vec<T>, String> {
    let index = position(items.len(), start)?;
    let count = length
        .map(|value| -> Result<usize, String> {
            if value.fract() != Decimal::ZERO || value < Decimal::ZERO {
                return Err("invalid list length".into());
            }
            Ok(if value >= Decimal::from(items.len() - index) {
                items.len() - index
            } else {
                value.to_usize().ok_or("invalid list length")?
            })
        })
        .transpose()?
        .unwrap_or(items.len() - index);
    Ok(items[index..items.len().min(index.saturating_add(count))].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_modes_return_all_winners_in_first_occurrence_order() {
        assert_eq!(modes(&[true, false, true, false]), Ok(vec![true, false]));
        assert_eq!(modes(&["z", "a", "z"]), Ok(vec!["z"]));
        assert!(modes::<bool>(&[]).is_err());
    }

    #[test]
    fn modes_checked_rejects_excess_input_before_comparing() {
        let values = vec![1; 1025];
        let result = modes_checked(&values, |_, _| Err("comparison occurred".into()));
        assert!(matches!(result, Err(message) if message.contains("too large")));
        assert_eq!(modes(&vec![true; 1024]), Ok(vec![true]));
    }

    #[test]
    fn aggregate_min_max_propagates_temporal_comparison_errors() {
        let comparison = |left: &&str, right: &&str| {
            if left == right {
                Ok(Ordering::Equal)
            } else {
                Err("incompatible temporal zones".into())
            }
        };
        assert_eq!(extreme(&["a"], false, comparison).unwrap(), "a");
        assert!(extreme(&["a", "b"], false, comparison).is_err());
        assert!(extreme::<&str>(&[], false, comparison).is_err());
    }

    #[test]
    fn sort_checked_rejects_oversized_priorities_even_for_empty_source() {
        let priorities = vec![1; 100_000];
        let result = sort_checked(Vec::<i32>::new(), Some(&priorities), false, |_, _| {
            Err("comparison occurred".into())
        });
        assert!(matches!(result, Err(message) if message.contains("too large")));
    }

    #[test]
    fn sort_checked_rejects_oversized_source_before_comparing() {
        let result = sort_checked(vec![1; 100_000], None, false, |_, _| {
            Err("comparison occurred".into())
        });
        assert!(matches!(result, Err(message) if message.contains("too large")));
    }

    #[test]
    fn sort_checked_rejects_oversized_combined_source_and_priorities() {
        let priorities = vec![1; 600];
        let result = sort_checked(vec![1; 600], Some(&priorities), false, |_, _| {
            Err("comparison occurred".into())
        });
        assert!(matches!(result, Err(message) if message.contains("too large")));
    }

    #[test]
    fn sort_checked_propagates_errors_even_when_priorities_cover_every_value() {
        let error = sort_checked(vec![1, 2], Some(&[1, 2]), false, |a, b| {
            if a == b {
                Ok(std::cmp::Ordering::Equal)
            } else {
                Err("incompatible".into())
            }
        });
        assert_eq!(error, Err("incompatible".into()));
    }

    #[test]
    fn sort_checked_preserves_equal_comparator_values() {
        let values = vec![(2, "first"), (1, "middle"), (2, "last")];
        assert_eq!(
            sort_checked(values, None, true, |a, b| Ok(a.0.cmp(&b.0))).unwrap(),
            vec![(2, "first"), (2, "last"), (1, "middle")]
        );
    }

    #[test]
    fn position_checks_one_based_and_negative_from_end_bounds() {
        assert_eq!(position(3, Decimal::ONE).unwrap(), 0);
        assert_eq!(position(3, -Decimal::ONE).unwrap(), 2);
        assert_eq!(position(3, Decimal::from(-3)).unwrap(), 0);
        for invalid in [
            Decimal::ZERO,
            Decimal::from(4),
            Decimal::from(-4),
            Decimal::new(15, 1),
            Decimal::MAX,
        ] {
            assert!(position(3, invalid).is_err(), "{invalid}");
        }
        assert!(position(0, Decimal::ONE).is_err());
        assert!(position(0, -Decimal::ONE).is_err());
    }
}
