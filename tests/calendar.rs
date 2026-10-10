use blkit::temporal::Calendar;
use blkit_core as blkit;
use serde_json::{Value, json};

#[test]
fn calendar_json_orders_entries_and_roundtrips_bounds_and_names() {
    let input = json!({
        "validFrom": "2025-01-01",
        "validTo": "2025-12-31",
        "entries": [
            {"value": "2025-04-21", "name": "Easter Monday"},
            {"value": {"start": "2025-04-18", "end": "2025-04-20", "includeStart": true, "includeEnd": false}, "name": "Easter weekend"},
            {"value": "2025-04-18"}
        ]
    });
    let calendar: Calendar = serde_json::from_value(input).unwrap();
    let output: Value = serde_json::to_value(&calendar).unwrap();
    assert_eq!(output["entries"][0]["value"], "2025-04-18");
    assert_eq!(output["entries"][1]["name"], "Easter weekend");
    assert_eq!(output["entries"][1]["value"]["includeEnd"], false);
    assert_eq!(output["entries"][2]["name"], "Easter Monday");
    assert_eq!(
        serde_json::from_value::<Calendar>(output.clone()).unwrap(),
        calendar
    );
}

#[test]
fn calendar_equality_uses_entries_as_a_set_and_includes_validity() {
    let original = serde_json::json!({"validFrom":"2025-01-01","validTo":"2025-12-31","entries":[{"value":"2025-04-18","name":"Good Friday"}]});
    let first: Calendar = serde_json::from_value(original.clone()).unwrap();
    let mut duplicates = original.clone();
    duplicates["entries"]
        .as_array_mut()
        .unwrap()
        .push(original["entries"][0].clone());
    let second: Calendar = serde_json::from_value(duplicates).unwrap();
    assert_eq!(first, second);
    let mut changed_bounds = original;
    changed_bounds["validTo"] = serde_json::json!("2025-11-30");
    assert_ne!(first, serde_json::from_value(changed_bounds).unwrap());
}

#[test]
fn calendar_json_rejects_invalid_dates_zones_bounds_and_ranges() {
    let valid = json!({"validFrom":"2025-01-01", "validTo":"2025-12-31", "entries":[{"value":"2025-04-18"}]});
    for (field, replacement) in [
        ("validFrom", json!("2026-01-01")),
        ("validTo", json!("2025-02-01")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = replacement;
        assert!(serde_json::from_value::<Calendar>(invalid).is_err());
    }
    for value in [
        json!("2025-02-30"),
        json!("2026-01-01"),
        json!("2025-04-18+01:00"),
        json!({"start":"2025-04-20", "end":"2025-04-18", "includeStart":true,"includeEnd":true}),
        json!({"start":"2025-04-18", "end":"2025-04-20T12:00:00", "includeStart":true,"includeEnd":true}),
    ] {
        let mut invalid = valid.clone();
        invalid["entries"][0]["value"] = value;
        assert!(
            serde_json::from_value::<Calendar>(invalid.clone()).is_err(),
            "{invalid}"
        );
    }
}
