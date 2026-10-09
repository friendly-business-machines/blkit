use blkit::temporal::{DTDuration, YMDuration};
use blkit_core as blkit;
use rust_decimal::Decimal;

fn number(text: &str) -> Decimal {
    Decimal::from_str_exact(text).unwrap()
}

#[test]
fn days_time_normalization_preserves_representable_fractional_seconds() {
    for (input, expected) in [
        ("p1.5d", "P1DT12H"),
        ("PT90M", "PT1H30M"),
        ("PT3600S", "PT1H"),
        ("-P2DT3H45M10S", "-P2DT3H45M10S"),
        ("PT0.1234567891S", "PT0.1234567891S"),
        ("PT0S", "PT0S"),
    ] {
        let value: DTDuration = input.parse().unwrap();
        assert_eq!(value.to_string(), expected);
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            format!("{expected:?}")
        );
        assert_eq!(
            serde_json::from_str::<DTDuration>(&format!("{expected:?}")).unwrap(),
            value
        );
    }
    let value: DTDuration = "-P2DT3H45M10S".parse().unwrap();
    assert_eq!(value.days(), number("-2"));
    assert_eq!(value.hours(), number("-3"));
    assert_eq!(value.minutes(), number("-45"));
    assert_eq!(value.seconds(), number("-10"));
    assert_eq!(value.total_seconds(), number("-186310"));
    assert_eq!(
        "PT90M".parse::<DTDuration>().unwrap().total_hours(),
        number("1.5")
    );
}

#[test]
fn years_months_normalization_and_signed_components() {
    for (input, expected) in [
        ("P13M", "P1Y1M"),
        ("P1.5Y", "P1Y6M"),
        ("p1y0.25m", "P1Y0.25M"),
        ("-P2Y7M", "-P2Y7M"),
        ("P0M", "P0M"),
    ] {
        let value: YMDuration = input.parse().unwrap();
        assert_eq!(value.to_string(), expected);
        assert_eq!(
            serde_json::from_str::<YMDuration>(&serde_json::to_string(&value).unwrap()).unwrap(),
            value
        );
    }
    let value: YMDuration = "-P2Y7M".parse().unwrap();
    assert_eq!(value.years(), number("-2"));
    assert_eq!(value.months(), number("-7"));
    assert_eq!(value.total_months(), number("-31"));
    assert_eq!(
        "P1Y".parse::<YMDuration>().unwrap(),
        "P12M".parse().unwrap()
    );
}

#[test]
fn durations_scale_with_number_precision_and_checked_errors() {
    let hour: DTDuration = "PT1H".parse().unwrap();
    let seventh = hour.checked_div(number("7")).unwrap();
    assert_eq!(
        seventh.total_seconds(),
        number("514.28571428571428571428571429")
    );
    assert_eq!(seventh, seventh.to_string().parse().unwrap());
    assert_eq!(
        hour.checked_add("PT30M".parse().unwrap())
            .unwrap()
            .to_string(),
        "PT1H30M"
    );
    assert_eq!(
        hour.checked_sub("PT2H".parse().unwrap())
            .unwrap()
            .to_string(),
        "-PT1H"
    );
    assert_eq!(
        hour.checked_mul(number("2.5")).unwrap().to_string(),
        "PT2H30M"
    );
    assert!(hour.checked_div(Decimal::ZERO).is_err());
    assert!(hour.checked_mul(Decimal::MAX).is_err());
    let year: YMDuration = "P1Y".parse().unwrap();
    assert_eq!(year.checked_div(number("4")).unwrap().to_string(), "P3M");
    assert_eq!(
        year.checked_mul(number("1.5")).unwrap().to_string(),
        "P1Y6M"
    );
    assert!(year.checked_div(Decimal::ZERO).is_err());
}

#[test]
fn durations_reject_malformed_and_unrepresentable_values() {
    for invalid in [
        "",
        "P",
        "PT",
        "P1Y",
        "P1M",
        "PT1H1H",
        "PT1M2H",
        "P1DT",
        "PT1.2.3S",
        "PT1e2S",
        "PT-2S",
        "PT0.12345678901234567890123456789S",
        "P99999999999999999999999999999D",
    ] {
        assert!(invalid.parse::<DTDuration>().is_err(), "{invalid}");
    }
    for invalid in [
        "",
        "P",
        "P1D",
        "P1Y1Y",
        "P1M2Y",
        "P1Y2.3.4M",
        "P-1Y",
        "P99999999999999999999999999999Y",
    ] {
        assert!(invalid.parse::<YMDuration>().is_err(), "{invalid}");
    }
}
