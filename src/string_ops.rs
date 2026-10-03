use rust_decimal::{Decimal, prelude::ToPrimitive};
use unicode_segmentation::UnicodeSegmentation;

fn integer(value: Decimal) -> Result<i128, String> {
    if !value.fract().is_zero() {
        return Err("expected an integral Number".into());
    }
    value.to_i128().ok_or_else(|| "Number is too large".into())
}

fn count(value: Decimal) -> Result<usize, String> {
    usize::try_from(integer(value)?).map_err(|_| "expected a nonnegative count".into())
}

fn position(value: Decimal, len: usize) -> Result<usize, String> {
    let number = integer(value)?;
    let index = if number > 0 {
        number - 1
    } else {
        i128::try_from(len).unwrap_or(i128::MAX) + number
    };
    if number == 0 {
        return Err("position 0 is invalid".into());
    }
    usize::try_from(index)
        .ok()
        .filter(|&index| index < len)
        .ok_or_else(|| "position outside string".into())
}

pub fn string_length(text: &str) -> Decimal {
    Decimal::from(text.graphemes(true).count() as u64)
}

pub fn substring(text: &str, start: Decimal, length: Option<Decimal>) -> Result<String, String> {
    let graphemes: Vec<_> = text.graphemes(true).collect();
    let index = position(start, graphemes.len())?;
    let length = length
        .map(count)
        .transpose()?
        .unwrap_or(graphemes.len() - index);
    Ok(graphemes[index..graphemes.len().min(index.saturating_add(length))].concat())
}

pub fn char_at(text: &str, at: Decimal) -> Result<String, String> {
    let graphemes: Vec<_> = text.graphemes(true).collect();
    Ok(graphemes[position(at, graphemes.len())?].into())
}

pub fn index_of(text: &str, needle: &str) -> Decimal {
    if needle.is_empty() {
        return Decimal::ONE;
    }
    text.grapheme_indices(true)
        .enumerate()
        .find(|(_, (byte, _))| text[*byte..].starts_with(needle))
        .map_or(Decimal::ZERO, |(index, _)| {
            Decimal::from((index + 1) as u64)
        })
}

pub fn reverse(text: &str) -> String {
    text.graphemes(true).rev().collect()
}

pub fn pad_leading(text: &str, length: Decimal, pad: Option<&str>) -> Result<String, String> {
    padding(text, length, pad, true)
}

pub fn pad_trailing(text: &str, length: Decimal, pad: Option<&str>) -> Result<String, String> {
    padding(text, length, pad, false)
}

fn padding(
    text: &str,
    length: Decimal,
    pad: Option<&str>,
    leading: bool,
) -> Result<String, String> {
    let length = count(length)?;
    let pad = pad.unwrap_or(" ");
    if pad.graphemes(true).count() != 1 {
        return Err("pad text must be one visible character".into());
    }
    let missing = length.saturating_sub(text.graphemes(true).count());
    let bytes = missing
        .checked_mul(pad.len())
        .and_then(|n| n.checked_add(text.len()))
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or("padding size overflow")?;
    let mut result = String::new();
    result
        .try_reserve(bytes)
        .map_err(|_| "padding allocation failed".to_owned())?;
    if !leading {
        result.push_str(text);
    }
    for _ in 0..missing {
        result.push_str(pad);
    }
    if leading {
        result.push_str(text);
    }
    if missing > 0 && result.graphemes(true).count() != length {
        return Err("pad text does not preserve visible character boundaries".into());
    }
    Ok(result)
}

pub fn repeat(text: &str, times: Decimal) -> Result<String, String> {
    let times = count(times)?;
    let bytes = text
        .len()
        .checked_mul(times)
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or("repeat size overflow")?;
    let mut result = String::new();
    result
        .try_reserve(bytes)
        .map_err(|_| "repeat allocation failed".to_owned())?;
    for _ in 0..times {
        result.push_str(text);
    }
    Ok(result)
}

pub fn join(values: &[String], separator: &str) -> String {
    values.join(separator)
}

pub fn before<'a>(text: &'a str, needle: &str) -> &'a str {
    text.split_once(needle).map_or("", |(left, _)| left)
}

pub fn after<'a>(text: &'a str, needle: &str) -> &'a str {
    text.split_once(needle).map_or("", |(_, right)| right)
}

pub fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

pub fn scalar<T: serde::Serialize>(value: &T) -> Result<String, String> {
    Ok(
        match serde_json::to_value(value).map_err(|error| error.to_string())? {
            serde_json::Value::String(text) => text,
            other => other.to_string(),
        },
    )
}

pub trait SplitOn {
    fn separators(&self) -> Vec<&str>;
}

impl SplitOn for String {
    fn separators(&self) -> Vec<&str> {
        vec![self.as_str()]
    }
}

impl SplitOn for Vec<String> {
    fn separators(&self) -> Vec<&str> {
        self.iter().map(String::as_str).collect()
    }
}

pub fn split<D: SplitOn>(text: &str, delimiter: &D) -> Result<Vec<String>, String> {
    let separators = delimiter.separators();
    if separators.is_empty() || separators.iter().any(|s| s.is_empty()) {
        return Err("split requires nonempty delimiters".into());
    }
    let mut rest = text;
    let mut result = Vec::new();
    loop {
        let next = separators
            .iter()
            .enumerate()
            .filter_map(|(order, delimiter)| rest.find(delimiter).map(|at| (at, order)))
            .min();
        let Some((at, order)) = next else {
            break;
        };
        result.push(rest[..at].to_owned());
        rest = &rest[at + separators[order].len()..];
    }
    result.push(rest.to_owned());
    Ok(result)
}

fn regex(pattern: &str, flags: Option<&str>) -> Result<regex::Regex, String> {
    let flags = flags.unwrap_or("");
    if !flags.chars().all(|ch| matches!(ch, 'i' | 'm' | 's')) {
        return Err(format!("invalid regex flag: {flags}"));
    }
    regex::RegexBuilder::new(pattern)
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'))
        .build()
        .map_err(|e| format!("invalid regex: {e}"))
}

pub fn matches(text: &str, pattern: &str, flags: Option<&str>) -> Result<bool, String> {
    Ok(regex(pattern, flags)?.is_match(text))
}

pub fn replace(
    text: &str,
    pattern: &str,
    replacement: &str,
    flags: Option<&str>,
) -> Result<String, String> {
    Ok(regex(pattern, flags)?
        .replace_all(text, replacement)
        .into_owned())
}

pub fn extract(text: &str, pattern: &str, flags: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    let regex = regex(pattern, flags)?;
    Ok(regex
        .captures_iter(text)
        .map(|capture| {
            if regex.captures_len() == 1 {
                vec![capture.get(0).unwrap().as_str().to_owned()]
            } else {
                (1..regex.captures_len())
                    .filter_map(|index| capture.get(index).map(|group| group.as_str().to_owned()))
                    .collect()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn visible_characters_drive_positions_and_reverse() {
        let text = "e\u{301}x";
        assert_eq!(string_length(text), Decimal::from(2));
        assert_eq!(char_at(text, Decimal::from(1)).unwrap(), "e\u{301}");
        assert_eq!(char_at(text, Decimal::from(-1)).unwrap(), "x");
        assert_eq!(
            substring(text, Decimal::from(-2), Some(Decimal::from(1))).unwrap(),
            "e\u{301}"
        );
        assert_eq!(
            substring(text, Decimal::from(1), Some(Decimal::from(20))).unwrap(),
            text
        );
        assert_eq!(index_of(text, "x"), Decimal::from(2));
        assert_eq!(index_of(text, "\u{301}"), Decimal::ZERO);
        assert_eq!(index_of(text, ""), Decimal::ONE);
        assert_eq!(reverse(text), "xe\u{301}");
    }

    #[test]
    fn literal_operations_keep_empty_fields_and_unicode_whitespace() {
        assert_eq!(join(&["a".into(), "b".into()], ", "), "a, b");
        assert_eq!(join(&[], ","), "");
        assert_eq!(before("a:b", ":"), "a");
        assert_eq!(after("a:b", ":"), "b");
        assert_eq!(after("abc", "z"), "");
        assert!(is_blank("\u{2003}"));
        assert!(!is_blank(" a"));
        assert_eq!(split("a,b,", &",".to_string()).unwrap(), ["a", "b", ""]);
        assert_eq!(
            split("a,b;c", &vec![",".into(), ";".into()]).unwrap(),
            ["a", "b", "c"]
        );
        assert_eq!(split("a,,b", &",".to_string()).unwrap(), ["a", "", "b"]);
        assert!(split("abc", &"".to_string()).is_err());
        assert!(split("abc", &Vec::<String>::new()).is_err());
        assert!(split("abc", &vec!["a".into(), "".into()]).is_err());
    }

    #[test]
    fn regex_search_replace_and_grouped_extraction() {
        assert!(matches("ABC", "b", Some("i")).unwrap());
        assert!(!matches("abc", "z", None).unwrap());
        assert!(matches("a\nb", "^b", Some("m")).unwrap());
        assert!(matches("a\nb", "a.b", Some("s")).unwrap());
        assert_eq!(
            replace("order-123, order-456", r"order-(\d+)", "item-$1", None).unwrap(),
            "item-123, item-456"
        );
        assert_eq!(
            extract("ab a", "(a)(b)?", None).unwrap(),
            vec![vec!["a", "b"], vec!["a"]]
        );
        assert_eq!(extract("ab ab", "ab", None).unwrap(), [["ab"], ["ab"]]);
        assert!(extract("abc", "z", None).unwrap().is_empty());
        for flag in ["q", "ix"] {
            assert!(matches("x", "x", Some(flag)).is_err());
        }
        assert!(matches("x", "[", None).is_err());
        assert!(replace("x", "[", "y", None).is_err());
        assert!(extract("x", "[", None).is_err());
    }

    #[test]
    fn scalar_conversion_uses_serialized_text() {
        assert_eq!(scalar(&Decimal::from(123)).unwrap(), "123");
        assert_eq!(scalar(&true).unwrap(), "true");
        assert_eq!(scalar(&"hi").unwrap(), "hi");
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(scalar(&date).unwrap(), "2026-01-01");
        let datetime = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+02:00").unwrap();
        assert_eq!(scalar(&datetime).unwrap(), "2026-01-01T00:00:00+02:00");
    }

    #[test]
    fn scalar_serialization_failure_returns_error() {
        let mut map = std::collections::BTreeMap::new();
        map.insert(vec![1, 2], 3);
        assert!(scalar(&map).is_err());
    }

    #[test]
    fn invalid_positions_counts_and_padding_fail_without_panic() {
        for position in [
            Decimal::ZERO,
            Decimal::from(3),
            Decimal::from(-3),
            Decimal::new(15, 1),
        ] {
            assert!(char_at("ab", position).is_err());
            assert!(substring("ab", position, None).is_err());
        }
        assert!(substring("ab", Decimal::ONE, Some(Decimal::from(-1))).is_err());
        assert!(pad_leading("a", Decimal::from(3), Some("xy")).is_err());
        assert!(pad_trailing("a", Decimal::from(2), Some("\u{301}")).is_err());
        assert!(repeat("a", Decimal::from(-1)).is_err());
        assert!(repeat("ab", Decimal::MAX).is_err());
        assert_eq!(
            pad_leading("a", Decimal::from(3), Some("é")).unwrap(),
            "ééa"
        );
        assert_eq!(pad_trailing("abc", Decimal::from(2), None).unwrap(), "abc");
        assert_eq!(repeat("ab", Decimal::from(2)).unwrap(), "abab");
        assert_eq!(repeat("ab", Decimal::ZERO).unwrap(), "");
    }
}
