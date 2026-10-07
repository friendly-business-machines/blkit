use rust_decimal::{Decimal, MathematicalOps, RoundingStrategy, prelude::ToPrimitive};

pub fn arithmetic(op: &str, left: Decimal, right: Decimal) -> Result<Decimal, String> {
    let result = match op {
        "+" => left.checked_add(right),
        "-" => left.checked_sub(right),
        "*" => left.checked_mul(right),
        "/" if !right.is_zero() => left.checked_div(right),
        "%" if !right.is_zero() => left.checked_rem(right),
        "**" if !(left.is_zero() && right.is_sign_negative())
            && !(left.is_sign_negative() && right.fract() != Decimal::ZERO) =>
        {
            left.checked_powd(right).map(|value| {
                // The transcendental approximation can miss an exact integer square root.
                let integer = value.round();
                if right == Decimal::new(5, 1) && integer.checked_mul(integer) == Some(left) {
                    integer
                } else {
                    value
                }
            })
        }
        "/" | "%" | "**" => None,
        _ => return Err(format!("unsupported Number operator: {op}")),
    };
    result
        .ok_or_else(|| format!("Number {op} failed: division by zero, invalid power, or overflow"))
}

pub fn round(value: Decimal, scale: Decimal, mode: &str) -> Result<Decimal, String> {
    let places = scale
        .to_i32()
        .filter(|_| scale.fract().is_zero())
        .filter(|places| (-28..=28).contains(places))
        .ok_or_else(|| String::from("invalid Number rounding scale"))?;
    let strategy = match mode {
        "round" | "roundHalfUp" => RoundingStrategy::MidpointAwayFromZero,
        "roundHalfDown" => RoundingStrategy::MidpointTowardZero,
        "roundHalfEven" => RoundingStrategy::MidpointNearestEven,
        "roundUp" => RoundingStrategy::AwayFromZero,
        "roundDown" => RoundingStrategy::ToZero,
        "floor" => RoundingStrategy::ToNegativeInfinity,
        "ceiling" => RoundingStrategy::ToPositiveInfinity,
        _ => return Err(format!("unknown rounding mode: {mode}")),
    };
    if places >= 0 {
        return Ok(value.round_dp_with_strategy(places as u32, strategy));
    }
    let factor = Decimal::TEN
        .checked_powu((-places) as u64)
        .ok_or_else(|| String::from("unrepresentable Number rounding scale"))?;
    let remainder = arithmetic("%", value, factor)?;
    if remainder.is_zero() {
        return Ok(value);
    }
    let quotient = arithmetic("/", arithmetic("-", value, remainder)?, factor)?;
    let twice = arithmetic("*", remainder.abs(), Decimal::TWO)?;
    let advance = match strategy {
        RoundingStrategy::AwayFromZero => true,
        RoundingStrategy::ToZero => false,
        RoundingStrategy::ToNegativeInfinity => remainder.is_sign_negative(),
        RoundingStrategy::ToPositiveInfinity => remainder.is_sign_positive(),
        RoundingStrategy::MidpointAwayFromZero => twice >= factor,
        RoundingStrategy::MidpointTowardZero => twice > factor,
        RoundingStrategy::MidpointNearestEven => {
            twice > factor
                || (twice == factor && !arithmetic("%", quotient, Decimal::TWO)?.is_zero())
        }
        _ => return Err("unsupported Number rounding strategy".into()),
    };
    let rounded = if advance {
        arithmetic(
            "+",
            quotient,
            if remainder.is_sign_negative() {
                -Decimal::ONE
            } else {
                Decimal::ONE
            },
        )?
    } else {
        quotient
    };
    arithmetic("*", rounded, factor)
}

fn checked_sqrt(value: Decimal) -> Option<Decimal> {
    if value.is_zero() {
        return Some(Decimal::ZERO);
    }
    let mut guess = value
        .checked_div(Decimal::TWO)
        .filter(|half| !half.is_zero())
        .unwrap_or(value);
    for _ in 0..128 {
        let next = guess
            .checked_add(value.checked_div(guess)?)?
            .checked_div(Decimal::TWO)?;
        if next == guess {
            return Some(next);
        }
        guess = next;
    }
    None
}

pub fn math(name: &str, args: &[Decimal]) -> Result<Decimal, String> {
    let valid_arity = match name {
        "abs" | "sqrt" | "exp" | "ln" => args.len() == 1,
        "modulo" => args.len() == 2,
        "log" => matches!(args.len(), 1 | 2),
        "clamp" => args.len() == 3,
        _ => false,
    };
    if !valid_arity {
        return Err(format!("invalid Number {name} argument count"));
    }
    let value = args[0];
    let result = match name {
        "abs" => Some(value.abs()),
        "modulo" => {
            let divisor = args[1];
            if divisor.is_zero() {
                None
            } else {
                value.checked_rem(divisor).and_then(|remainder| {
                    if !remainder.is_zero()
                        && remainder.is_sign_negative() != divisor.is_sign_negative()
                    {
                        remainder.checked_add(divisor)
                    } else {
                        Some(remainder)
                    }
                })
            }
        }
        "sqrt" if value >= Decimal::ZERO => checked_sqrt(value),
        "exp" => value.checked_exp(),
        "ln" if value > Decimal::ZERO => value.checked_ln(),
        "log" if value > Decimal::ZERO => {
            let base = args.get(1).copied().unwrap_or(Decimal::TEN);
            if base <= Decimal::ZERO || base == Decimal::ONE {
                None
            } else if base == Decimal::TEN {
                value.checked_log10()
            } else {
                let mut exact = None;
                if base > Decimal::ONE {
                    let mut power = Decimal::ONE;
                    for exponent in 0..=96 {
                        if power == value {
                            exact = Some(Decimal::from(exponent));
                            break;
                        }
                        if power > value {
                            break;
                        }
                        let Some(next) = power.checked_mul(base) else {
                            break;
                        };
                        power = next;
                    }
                }
                exact.or_else(|| value.checked_ln()?.checked_div(base.checked_ln()?))
            }
        }
        "clamp" if args[1] <= args[2] => Some(value.clamp(args[1], args[2])),
        _ => None,
    };
    result.ok_or_else(|| format!("invalid or unrepresentable Number {name}"))
}

pub fn number(text: &str, separators: Option<(&str, &str)>) -> Result<Decimal, String> {
    let (grouping, decimal) = if let Some((grouping, decimal)) = separators {
        let valid = |separator: &str| {
            separator.len() == 1
                && separator.as_bytes()[0].is_ascii_punctuation()
                && !matches!(separator, "+" | "-")
        };
        if !valid(grouping) || !valid(decimal) || grouping == decimal {
            return Err("invalid Number separators".into());
        }
        (Some(grouping), decimal)
    } else {
        (None, ".")
    };
    let (sign, digits) = if let Some(rest) = text.strip_prefix(['+', '-']) {
        (&text[..1], rest)
    } else {
        ("", text)
    };
    let mut parts = digits.split(decimal);
    let integer = parts.next().unwrap_or("");
    let fractional = parts.next();
    if parts.next().is_some()
        || fractional
            .is_some_and(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("invalid Number text".into());
    }
    let integer = if let Some(group) = grouping {
        let chunks: Vec<_> = integer.split(group).collect();
        if !chunks[0].bytes().all(|b| b.is_ascii_digit())
            || chunks[0].is_empty()
            || (chunks.len() > 1
                && (chunks[0].len() > 3
                    || chunks[1..]
                        .iter()
                        .any(|part| part.len() != 3 || !part.bytes().all(|b| b.is_ascii_digit()))))
        {
            return Err("invalid Number grouping".into());
        }
        chunks.concat()
    } else if integer.is_empty() || !integer.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid Number text".into());
    } else {
        integer.to_owned()
    };
    let canonical = if let Some(fractional) = fractional {
        format!("{sign}{integer}.{fractional}")
    } else {
        format!("{sign}{integer}")
    };
    Decimal::from_str_exact(&canonical).map_err(|_| "unrepresentable Number text".into())
}

pub fn aggregate(name: &str, values: &[Decimal]) -> Result<Decimal, String> {
    let sum = || {
        values
            .iter()
            .try_fold(Decimal::ZERO, |total, &value| arithmetic("+", total, value))
    };
    let product = || {
        values
            .iter()
            .try_fold(Decimal::ONE, |total, &value| arithmetic("*", total, value))
    };
    match name {
        "sum" => sum(),
        "product" => product(),
        "min" => values
            .iter()
            .copied()
            .min()
            .ok_or_else(|| "min requires values".into()),
        "max" => values
            .iter()
            .copied()
            .max()
            .ok_or_else(|| "max requires values".into()),
        "mean" if !values.is_empty() => arithmetic("/", sum()?, Decimal::from(values.len() as u64)),
        "median" if !values.is_empty() => {
            let mut sorted = values.to_vec();
            sorted.sort();
            let middle = sorted.len() / 2;
            if sorted.len() % 2 == 1 {
                Ok(sorted[middle])
            } else {
                arithmetic(
                    "/",
                    arithmetic("+", sorted[middle - 1], sorted[middle])?,
                    Decimal::TWO,
                )
            }
        }
        "stddev" if values.len() >= 2 => {
            let mean = arithmetic("/", sum()?, Decimal::from(values.len() as u64))?;
            let squares = values.iter().try_fold(Decimal::ZERO, |total, &value| {
                let distance = arithmetic("-", value, mean)?;
                arithmetic("+", total, arithmetic("*", distance, distance)?)
            })?;
            let variance = arithmetic("/", squares, Decimal::from((values.len() - 1) as u64))?;
            math("sqrt", &[variance])
        }
        "mode" if !values.is_empty() => {
            let mut frequencies = std::collections::BTreeMap::new();
            for &value in values {
                *frequencies.entry(value).or_insert(0usize) += 1;
            }
            Ok(frequencies
                .into_iter()
                .max_by_key(|(value, count)| (*count, std::cmp::Reverse(*value)))
                .unwrap()
                .0)
        }
        _ => Err(format!(
            "{name} requires a nonempty list{}",
            if name == "stddev" {
                " of at least two values"
            } else {
                ""
            }
        )),
    }
}

pub fn predicate(name: &str, value: Decimal) -> Result<bool, String> {
    match name {
        "odd" | "even" if value.fract().is_zero() => {
            let odd = value
                .checked_rem(Decimal::TWO)
                .is_some_and(|remainder| !remainder.is_zero());
            Ok(if name == "odd" { odd } else { !odd })
        }
        "isPositive" => Ok(value > Decimal::ZERO),
        "isNegative" => Ok(value < Decimal::ZERO),
        "isZero" => Ok(value.is_zero()),
        _ => Err(format!("invalid integral Number for {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_values_round_away_from_zero_at_negative_scales() {
        let tiny = Decimal::new(1, 28);
        assert_eq!(round(tiny, -Decimal::ONE, "roundUp"), Ok(Decimal::TEN));
        assert_eq!(round(-tiny, -Decimal::ONE, "floor"), Ok(-Decimal::TEN));
        assert_eq!(round(tiny, -Decimal::ONE, "roundDown"), Ok(Decimal::ZERO));
    }

    #[test]
    fn sqrt_accepts_largest_representable_number() {
        assert!(math("sqrt", &[Decimal::MAX]).is_ok());
        assert_eq!(
            math("sqrt", &[Decimal::new(1, 28)]),
            Ok(Decimal::new(1, 14))
        );
    }

    #[test]
    fn public_math_rejects_wrong_arity_without_panicking() {
        for (name, values) in [
            ("sqrt", &[][..]),
            ("modulo", &[Decimal::ONE][..]),
            ("clamp", &[Decimal::ONE, Decimal::ONE][..]),
            ("log", &[Decimal::ONE, Decimal::TWO, Decimal::TEN][..]),
        ] {
            assert!(math(name, values).is_err(), "{name}: {values:?}");
        }
    }

    #[test]
    fn arithmetic_errors_are_returned() {
        assert!(arithmetic("+", Decimal::MAX, Decimal::ONE).is_err());
        assert!(arithmetic("/", Decimal::ONE, Decimal::ZERO).is_err());
        assert!(arithmetic("**", Decimal::ZERO, -Decimal::ONE).is_err());
        assert!(arithmetic("**", -Decimal::ONE, Decimal::new(5, 1)).is_err());
    }
}
