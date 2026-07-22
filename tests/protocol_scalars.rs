use nautilus_agents::protocol::value::{Quantity, TimestampNs, ValueError};
use proptest::prelude::*;
use rstest::rstest;

proptest! {
    #[test]
    fn timestamp_round_trip_preserves_every_unsigned_value(value in any::<u64>()) {
        let timestamp = TimestampNs::new(value);
        let expected = format!("\"{value}\"");

        let json = serde_json::to_string(&timestamp).unwrap();
        let decoded: TimestampNs = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(json, expected);
        prop_assert_eq!(decoded, timestamp);
    }

    #[test]
    fn positive_integer_quantity_round_trip_is_canonical(value in 1_u64..=u64::MAX) {
        let expected = value.to_string();
        let quantity = Quantity::parse(&expected).unwrap();

        let json = serde_json::to_string(&quantity).unwrap();
        let decoded: Quantity = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(json, format!("\"{expected}\""));
        prop_assert_eq!(decoded, quantity);
    }
}

#[rstest]
#[case("", ValueError::EmptyQuantity)]
#[case("-1", ValueError::QuantitySign)]
#[case("+1", ValueError::QuantitySign)]
#[case("1e3", ValueError::QuantityExponent)]
#[case("01", ValueError::QuantityLeadingZero)]
#[case("1.", ValueError::QuantityEmptyFraction)]
#[case("1.20", ValueError::QuantityTrailingZero)]
#[case("0", ValueError::QuantityZero)]
#[case("0.12345678901234567", ValueError::QuantityPrecision)]
fn invalid_quantity_has_exact_error(#[case] value: &str, #[case] expected: ValueError) {
    assert_eq!(Quantity::parse(value), Err(expected));
}
