use blkit::temporal::{Date, DateTime, Time};
use blkit_core as blkit;

#[test]
fn temporal_values_round_trip_with_distinct_zone_kinds() {
    for (input, output) in [
        ("2026-10-02", "2026-10-02"),
        ("2026-10-02+05:30", "2026-10-02+05:30"),
        ("2026-10-02[Europe/Paris]", "2026-10-02[Europe/Paris]"),
    ] {
        let value: Date = input.parse().unwrap();
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            format!("{output:?}")
        );
        assert_eq!(
            serde_json::from_str::<Date>(&format!("{input:?}")).unwrap(),
            value
        );
    }
    for (input, output) in [
        ("24:00:00", "00:00:00"),
        ("24:00:00+02:00", "00:00:00+02:00"),
        ("11:45:30[Europe/Paris]", "11:45:30[Europe/Paris]"),
    ] {
        let value: Time = input.parse().unwrap();
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            format!("{output:?}")
        );
        assert_eq!(
            serde_json::from_str::<Time>(&format!("{input:?}")).unwrap(),
            value
        );
    }
    for input in [
        "2026-10-02T09:30:00",
        "2026-10-02T09:30:00+02:00",
        "2026-10-02T09:30:00[Europe/Paris]",
    ] {
        let value: DateTime = input.parse().unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), format!("{input:?}"));
        assert_eq!(
            serde_json::from_str::<DateTime>(&format!("{input:?}")).unwrap(),
            value
        );
    }
}

#[test]
fn temporal_values_reject_invalid_dates_times_zones_and_dst() {
    for invalid in [
        "2026-02-30",
        "2026-10-02[No/Such_Zone]",
        "2026-10-02+02:00[Europe/Paris]",
    ] {
        assert!(invalid.parse::<Date>().is_err(), "{invalid}");
    }
    for invalid in [
        "24:00:01",
        "24:00:00.1",
        "23:59:60",
        "09:30:00[No/Such_Zone]",
        "09:30:00Z[UTC]",
    ] {
        assert!(invalid.parse::<Time>().is_err(), "{invalid}");
    }
    for invalid in [
        "2026-10-02 09:30:00",
        "2025-03-30T02:30:00[Europe/Paris]", // DST gap
        "2025-10-26T02:30:00[Europe/Paris]", // DST fold
        "2025-03-28T14:30:00+01:00[Europe/Paris]",
    ] {
        assert!(invalid.parse::<DateTime>().is_err(), "{invalid}");
    }
}
