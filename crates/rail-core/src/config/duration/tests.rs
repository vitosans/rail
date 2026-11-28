use std::path::Path;
use std::time::Duration;

use super::parse;

#[test]
fn accepts_existing_unit_spellings() {
    for (spellings, expected) in [
        ("ns nsec nanos", Duration::from_nanos(1)),
        ("us µs usec", Duration::from_micros(1)),
        ("ms msec millis", Duration::from_millis(1)),
        ("s sec secs second seconds", Duration::from_secs(1)),
        ("m min mins minute minutes", Duration::from_secs(60)),
        ("h hr hrs hour hours", Duration::from_secs(3_600)),
        ("d day days", Duration::from_secs(86_400)),
        ("w wk wks week weeks", Duration::from_secs(604_800)),
        ("M month months", Duration::from_secs(2_630_016)),
        ("y yr yrs year years", Duration::from_secs(31_557_600)),
    ] {
        for unit in spellings.split_whitespace() {
            assert_eq!(parse(&format!("1{unit}")), Some(expected), "{unit}");
        }
    }
}

#[test]
fn combines_components_with_existing_precision_and_whitespace_rules() {
    for (input, expected) in [
        ("0", Duration::ZERO),
        ("2s1min3s", Duration::from_secs(65)),
        ("1\u{2003}2. 5 0 s\t3µs ", Duration::new(12, 500_003_000)),
        ("0.000000001s", Duration::from_nanos(1)),
        ("0.0000000001m", Duration::from_nanos(6)),
        ("0.0000000000000000000s", Duration::ZERO),
        ("0.25hrs", Duration::from_secs(900)),
        ("0.5M0.5y", Duration::from_secs(17_093_808)),
        ("18446744073709551615ns", Duration::from_nanos(u64::MAX)),
        ("18446744073709551615s999999999ns", Duration::MAX),
    ] {
        assert_eq!(parse(input), Some(expected), "{input:?}");
    }
}

#[test]
fn rejects_malformed_inexact_and_overflowing_components() {
    for input in [
        "",
        " ",
        " 0",
        "0 ",
        "00",
        "5",
        "-1s",
        "+1s",
        ".5s",
        "1.s",
        "1..0s",
        "1e2s",
        "1S",
        "1hs",
        "1μs",
        "1s,1m",
        "1h min",
        "1millisecond",
        "0.0ns",
        "0.1ns",
        "0.0001h",
        "0.0001µs",
        "0.0000000001s",
        "1.00000000000000000000s",
        "18446744073709551616s",
        "18446744073709551615ms",
        "18446744073709551615m",
        "1ns18446744073709551615ns",
        "18446744073709551615s1000000000ns",
        "18446744073709551615s1s",
    ] {
        assert_eq!(parse(input), None, "{input:?}");
    }
}

#[test]
fn config_loads_the_interval_and_reports_invalid_values_at_the_setting() {
    let path = Path::new("config.toml");
    let load =
        |value: &str| crate::config::parse(&format!("[general]\npoll_interval = {value}\n"), path);
    let interval = load(r#""5 seconds3µs""#).unwrap().general.poll_interval;
    assert_eq!(interval, Duration::new(5, 3_000));
    let too_short = load(r#""4.999999999s""#).unwrap_err().to_string();
    assert!(too_short.contains("at least 5s"), "{too_short}");
    for value in ["5", r#""1elephant""#] {
        let error = load(value).unwrap_err().to_string();
        assert!(error.contains("config.toml:2:"), "{error}");
        assert!(error.contains("expected a duration"), "{error}");
    }
}
