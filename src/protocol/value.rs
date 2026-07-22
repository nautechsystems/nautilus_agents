//! Strict protocol-native scalar values.

use std::{
    cmp::Ordering,
    fmt::{self, Write as _},
    str::FromStr,
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const DECIMAL_MAX_PRECISION: usize = 16;
const IDENTIFIER_MAX_BYTES: usize = 255;

/// Reports invalid protocol scalar values.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    /// An identifier is empty.
    #[error("identifier must not be empty")]
    EmptyIdentifier,
    /// An identifier exceeds 255 UTF-8 bytes.
    #[error("identifier exceeds 255 UTF-8 bytes")]
    IdentifierTooLong,
    /// An identifier contains a control character.
    #[error("identifier contains a control character")]
    IdentifierControlCharacter,
    /// A quantity is empty.
    #[error("quantity must not be empty")]
    EmptyQuantity,
    /// A quantity includes a sign.
    #[error("quantity must not include a sign")]
    QuantitySign,
    /// A quantity uses exponent notation.
    #[error("quantity must not use exponent notation")]
    QuantityExponent,
    /// A quantity contains an invalid character or decimal point.
    #[error("quantity must contain only digits and at most one decimal point")]
    QuantityCharacter,
    /// A quantity uses a redundant leading zero.
    #[error("quantity must not contain redundant leading zeroes")]
    QuantityLeadingZero,
    /// A quantity has an empty fractional component.
    #[error("quantity must include digits after the decimal point")]
    QuantityEmptyFraction,
    /// A quantity uses a redundant trailing zero.
    #[error("quantity must not contain redundant trailing zeroes")]
    QuantityTrailingZero,
    /// A quantity is zero.
    #[error("quantity must be strictly positive")]
    QuantityZero,
    /// A quantity exceeds the protocol decimal precision.
    #[error("quantity exceeds {DECIMAL_MAX_PRECISION} decimal places")]
    QuantityPrecision,
    /// A timestamp string is empty.
    #[error("timestamp must not be empty")]
    EmptyTimestamp,
    /// A timestamp is not a canonical unsigned decimal string.
    #[error("timestamp must be a canonical unsigned decimal string")]
    TimestampFormat,
    /// A timestamp does not fit in an unsigned 64-bit value.
    #[error("timestamp exceeds the unsigned 64-bit range")]
    TimestampRange,
    /// A content digest does not use the required SHA-256 form.
    #[error("content digest must use sha256 followed by 64 lowercase hexadecimal digits")]
    DigestFormat,
    /// A field path is not a valid RFC 6901 JSON Pointer.
    #[error("field path must be an RFC 6901 JSON Pointer")]
    FieldPathFormat,
}

macro_rules! identifier {
    ($name:ident, $docs:literal) => {
        #[doc = $docs]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(
            #[schemars(
                length(min = 1, max = 255),
                regex(pattern = r"^[^\u0000-\u001f\u007f-\u009f]+$")
            )]
            String,
        );

        impl $name {
            /// Creates a checked identifier.
            pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
                Self::parse(value.into())
            }

            /// Parses a non-empty identifier with no control characters.
            pub fn parse(value: impl AsRef<str>) -> Result<Self, ValueError> {
                let value = value.as_ref();
                if value.is_empty() {
                    return Err(ValueError::EmptyIdentifier);
                }

                if value.len() > IDENTIFIER_MAX_BYTES {
                    return Err(ValueError::IdentifierTooLong);
                }

                if value.chars().any(char::is_control) {
                    return Err(ValueError::IdentifierControlCharacter);
                }
                Ok(Self(value.to_owned()))
            }

            /// Returns the identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

identifier!(
    InstrumentId,
    "Identifies an instrument without exposing engine types."
);
identifier!(
    PositionId,
    "Identifies a position without exposing engine types."
);

/// Stores a strictly positive canonical decimal quantity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Quantity(
    #[schemars(regex(pattern = r"^(?:[1-9][0-9]*|(?:0|[1-9][0-9]*)\.[0-9]{0,15}[1-9])$"))] String,
);

impl Quantity {
    /// Creates a checked canonical decimal quantity.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        Self::parse(value.into())
    }

    /// Parses a strictly positive canonical decimal quantity.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValueError> {
        let value = value.as_ref();
        validate_quantity(value)?;
        Ok(Self(value.to_owned()))
    }

    /// Returns the canonical decimal text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn compare(&self, other: &Self) -> Ordering {
        let scale = self.scale().max(other.scale());
        decimal_digits(self.as_str(), scale).cmp_decimal(&decimal_digits(other.as_str(), scale))
    }

    pub(crate) fn is_multiple_of(&self, increment: &Self) -> bool {
        let scale = self.scale().max(increment.scale());
        let value = decimal_digits(self.as_str(), scale);
        let divisor = decimal_digits(increment.as_str(), scale);
        decimal_remainder(value, &divisor)
            .iter()
            .all(|digit| *digit == 0)
    }

    fn scale(&self) -> usize {
        self.0
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.len())
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Quantity {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for Quantity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

impl PartialOrd for Quantity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other)
    }
}

/// Stores unsigned Unix nanoseconds and serializes them as a decimal JSON string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, JsonSchema)]
#[schemars(
    with = "String",
    extend("pattern" = "^(?:0|[1-9][0-9]*)$")
)]
pub struct TimestampNs(u64);

impl TimestampNs {
    /// Creates an unsigned Unix nanosecond timestamp.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Parses canonical unsigned Unix nanoseconds.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValueError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(ValueError::EmptyTimestamp);
        }

        if !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(ValueError::TimestampFormat);
        }
        value
            .parse::<u64>()
            .map(Self)
            .map_err(|_| ValueError::TimestampRange)
    }

    /// Returns the timestamp as an unsigned integer.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TimestampNs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for TimestampNs {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for TimestampNs {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimestampNs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Stores a lowercase SHA-256 content digest.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ContentDigest(#[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))] String);

impl ContentDigest {
    /// Creates a digest from 32 SHA-256 bytes.
    #[must_use]
    pub fn new(bytes: [u8; 32]) -> Self {
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        for byte in bytes {
            write!(value, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Self(value)
    }

    /// Parses a lowercase SHA-256 digest.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValueError> {
        let value = value.as_ref();
        let valid = value.len() == 71
            && value.starts_with("sha256:")
            && value[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));

        if !valid {
            return Err(ValueError::DigestFormat);
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the prefixed digest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ContentDigest {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Stores an RFC 6901 JSON Pointer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct FieldPath(#[schemars(regex(pattern = "^(?:|(?:/(?:[^~/]|~[01])*)+)$"))] String);

impl FieldPath {
    /// Creates a checked JSON Pointer.
    pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
        Self::parse(value.into())
    }

    /// Parses an RFC 6901 JSON Pointer.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, ValueError> {
        let value = value.as_ref();
        if !value.is_empty() && !value.starts_with('/') {
            return Err(ValueError::FieldPathFormat);
        }
        let mut chars = value.chars();
        while let Some(ch) = chars.next() {
            if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(ValueError::FieldPathFormat);
            }
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the JSON Pointer text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FieldPath {
    type Err = ValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for FieldPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

fn validate_quantity(value: &str) -> Result<(), ValueError> {
    if value.is_empty() {
        return Err(ValueError::EmptyQuantity);
    }

    if value.starts_with(['+', '-']) {
        return Err(ValueError::QuantitySign);
    }

    if value.contains(['e', 'E']) {
        return Err(ValueError::QuantityExponent);
    }

    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || value.bytes().filter(|byte| *byte == b'.').count() > 1
    {
        return Err(ValueError::QuantityCharacter);
    }
    let (integer, fraction) = value
        .split_once('.')
        .map_or((value, None), |(integer, fraction)| {
            (integer, Some(fraction))
        });

    if integer.is_empty() {
        return Err(ValueError::QuantityCharacter);
    }

    if integer.len() > 1 && integer.starts_with('0') {
        return Err(ValueError::QuantityLeadingZero);
    }

    if let Some(fraction) = fraction {
        if fraction.is_empty() {
            return Err(ValueError::QuantityEmptyFraction);
        }

        if fraction.len() > DECIMAL_MAX_PRECISION {
            return Err(ValueError::QuantityPrecision);
        }

        if fraction.ends_with('0') {
            return Err(ValueError::QuantityTrailingZero);
        }
    }

    if integer == "0" && fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte == b'0')) {
        return Err(ValueError::QuantityZero);
    }
    Ok(())
}

trait DecimalOrder {
    fn cmp_decimal(&self, other: &[u8]) -> Ordering;
}

impl DecimalOrder for [u8] {
    fn cmp_decimal(&self, other: &[u8]) -> Ordering {
        let left = trim_zeroes(self);
        let right = trim_zeroes(other);
        left.len().cmp(&right.len()).then_with(|| left.cmp(right))
    }
}

fn decimal_digits(value: &str, scale: usize) -> Vec<u8> {
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    let mut digits = Vec::with_capacity(integer.len() + scale);
    digits.extend(integer.bytes().map(|byte| byte - b'0'));
    digits.extend(fraction.bytes().map(|byte| byte - b'0'));
    digits.resize(integer.len() + scale, 0);
    trim_zeroes(&digits).to_vec()
}

fn decimal_remainder(value: Vec<u8>, divisor: &[u8]) -> Vec<u8> {
    let mut remainder = Vec::new();
    for digit in value {
        remainder.push(digit);
        remainder = trim_zeroes(&remainder).to_vec();
        while remainder.as_slice().cmp_decimal(divisor) != Ordering::Less {
            subtract_decimal(&mut remainder, divisor);
        }
    }
    remainder
}

fn subtract_decimal(value: &mut Vec<u8>, subtrahend: &[u8]) {
    let mut borrow = 0_i8;
    let mut value_index = value.len();
    let mut subtrahend_index = subtrahend.len();
    while value_index > 0 {
        value_index -= 1;
        let subtrahend_digit = if subtrahend_index > 0 {
            subtrahend_index -= 1;
            subtrahend[subtrahend_index] as i8
        } else {
            0
        };
        let difference = value[value_index] as i8 - subtrahend_digit - borrow;
        if difference < 0 {
            value[value_index] = (difference + 10) as u8;
            borrow = 1;
        } else {
            value[value_index] = difference as u8;
            borrow = 0;
        }
    }
    *value = trim_zeroes(value).to_vec();
}

fn trim_zeroes(value: &[u8]) -> &[u8] {
    let first_nonzero = value
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(value.len());
    &value[first_nonzero..]
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_identifiers_boundaries_and_exact_json() {
        let instrument = InstrumentId::new("BTCUSDT.BINANCE").unwrap();
        assert_eq!(
            serde_json::to_string(&instrument).unwrap(),
            r#""BTCUSDT.BINANCE""#
        );
        assert_eq!(InstrumentId::new(""), Err(ValueError::EmptyIdentifier));
        assert_eq!(
            InstrumentId::new("x".repeat(256)),
            Err(ValueError::IdentifierTooLong)
        );
        assert_eq!(
            InstrumentId::new("BTC\nUSDT"),
            Err(ValueError::IdentifierControlCharacter)
        );
        assert_eq!(
            InstrumentId::new("BTC\u{0085}USDT"),
            Err(ValueError::IdentifierControlCharacter)
        );
    }

    #[rstest]
    fn test_quantity_exact_json_and_ordering() {
        let quantity = Quantity::new("12.345").unwrap();
        assert_eq!(serde_json::to_string(&quantity).unwrap(), r#""12.345""#);
        assert!(quantity > Quantity::new("12.3449").unwrap());
        assert!(
            Quantity::new("1.25")
                .unwrap()
                .is_multiple_of(&Quantity::new("0.25").unwrap())
        );
        assert!(
            !Quantity::new("1.2")
                .unwrap()
                .is_multiple_of(&Quantity::new("0.25").unwrap())
        );
    }

    #[rstest]
    fn test_quantity_rejects_noncanonical_inputs_with_exact_errors() {
        let cases = [
            ("", ValueError::EmptyQuantity),
            ("-1", ValueError::QuantitySign),
            ("+1", ValueError::QuantitySign),
            ("1e3", ValueError::QuantityExponent),
            ("1..2", ValueError::QuantityCharacter),
            ("01", ValueError::QuantityLeadingZero),
            ("1.", ValueError::QuantityEmptyFraction),
            ("1.20", ValueError::QuantityTrailingZero),
            ("0", ValueError::QuantityZero),
            ("0.00000000000000001", ValueError::QuantityPrecision),
        ];
        for (value, expected) in cases {
            assert_eq!(Quantity::new(value), Err(expected), "value: {value}");
        }
    }

    #[rstest]
    fn test_timestamp_exact_json_and_boundaries() {
        let timestamp = TimestampNs::new(1_712_400_000_000_000_123);
        assert_eq!(
            serde_json::to_string(&timestamp).unwrap(),
            r#""1712400000000000123""#
        );
        assert_eq!(
            serde_json::from_str::<TimestampNs>(r#""1712400000000000123""#).unwrap(),
            timestamp
        );
        assert_eq!(TimestampNs::parse("01"), Err(ValueError::TimestampFormat));
        assert_eq!(
            TimestampNs::parse("18446744073709551616"),
            Err(ValueError::TimestampRange)
        );
    }

    #[rstest]
    fn test_digest_exact_json_and_validation() {
        let digest = ContentDigest::new([0xab; 32]);
        let expected = format!("sha256:{}", "ab".repeat(32));
        assert_eq!(digest.as_str(), expected);
        assert_eq!(
            serde_json::to_string(&digest).unwrap(),
            format!(r#""{expected}""#)
        );
        assert_eq!(
            ContentDigest::parse(format!("sha256:{}", "AB".repeat(32))),
            Err(ValueError::DigestFormat)
        );
    }

    #[rstest]
    fn test_field_path_exact_json_and_validation() {
        let path = FieldPath::new("/payload/positions/0/position_id").unwrap();
        assert_eq!(
            serde_json::to_string(&path).unwrap(),
            r#""/payload/positions/0/position_id""#
        );
        assert!(FieldPath::new("").is_ok());
        assert!(FieldPath::new("/a~0b~1c").is_ok());
        assert_eq!(FieldPath::new("payload"), Err(ValueError::FieldPathFormat));
        assert_eq!(FieldPath::new("/a~2b"), Err(ValueError::FieldPathFormat));
    }
}
