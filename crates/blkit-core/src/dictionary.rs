use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

pub fn reject_null<T: Serialize>(input: &T) -> Result<(), String> {
    fn check(value: &Value) -> Result<(), String> {
        match value {
            Value::Null => Err("null is not a dictionary value".into()),
            Value::Array(values) => values.iter().try_for_each(check),
            Value::Object(fields) => fields.values().try_for_each(check),
            _ => Ok(()),
        }
    }
    let value = serde_json::to_value(input).map_err(|error| error.to_string())?;
    check(&value)
}

fn object<T: Serialize>(dictionary: T) -> Result<Map<String, Value>, String> {
    serde_json::to_value(dictionary)
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .ok_or_else(|| "expected dictionary".into())
}

pub fn keys<T: Serialize>(dictionary: T) -> Result<Vec<String>, String> {
    let mut keys: Vec<_> = object(dictionary)?
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    keys.sort();
    Ok(keys)
}

pub fn values<T: Serialize, V: DeserializeOwned>(dictionary: T) -> Result<Vec<V>, String> {
    object(dictionary)?.into_values().map(decode).collect()
}

pub fn entries<T: Serialize, V: DeserializeOwned>(
    dictionary: T,
) -> Result<Vec<(String, V)>, String> {
    object(dictionary)?
        .into_iter()
        .map(|(key, value)| Ok((key, decode(value)?)))
        .collect()
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

pub fn size<T: Serialize>(dictionary: T) -> Result<u64, String> {
    Ok(object(dictionary)?.len() as u64)
}

pub fn has<T: Serialize>(dictionary: T, key: &str) -> Result<bool, String> {
    Ok(object(dictionary)?.contains_key(key))
}

pub fn get<T: Serialize, V: DeserializeOwned>(dictionary: T, path: &[String]) -> Result<V, String> {
    if path.is_empty() {
        return Err("empty dictionary path".into());
    }
    let mut value = Value::Object(object(dictionary)?);
    for key in path {
        value = value
            .as_object()
            .and_then(|map| map.get(key))
            .ok_or_else(|| format!("missing dictionary key: {key}"))?
            .clone();
    }
    decode(value)
}

pub fn cast<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    decode(value)
}

pub fn cast_number(value: Value) -> Result<rust_decimal::Decimal, String> {
    let Value::Number(number) = value else {
        return Err("expected numeric dictionary value".into());
    };
    rust_decimal::Decimal::from_str_exact(&number.to_string()).map_err(|error| error.to_string())
}

pub fn get_number<T: Serialize>(
    dictionary: T,
    path: &[String],
) -> Result<rust_decimal::Decimal, String> {
    cast_number(get(dictionary, path)?)
}

pub fn put<T: Serialize>(
    dictionary: T,
    path: &[String],
    value: Value,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    if path.is_empty() {
        return Err("empty dictionary path".into());
    }
    let mut map = object(dictionary)?;
    let mut target = &mut map;
    for key in &path[..path.len() - 1] {
        target = target
            .get_mut(key)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("missing dictionary path: {key}"))?;
    }
    target.insert(path[path.len() - 1].clone(), value);
    Ok(map.into_iter().collect())
}

pub fn merge<T: Serialize>(
    dictionaries: Vec<T>,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    let mut result = std::collections::BTreeMap::new();
    for dictionary in dictionaries {
        result.extend(object(dictionary)?);
    }
    Ok(result)
}

pub fn remove<T: Serialize>(
    dictionary: T,
    key: &str,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    let mut map = object(dictionary)?;
    map.remove(key);
    Ok(map.into_iter().collect())
}
