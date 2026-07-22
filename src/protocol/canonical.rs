//! RFC 8785 canonical JSON and SHA-256 content digests.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::value::ContentDigest;

/// Serializes a value using the JSON Canonicalization Scheme from RFC 8785.
pub fn to_vec<T: Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    serde_json_canonicalizer::to_vec(value)
}

/// Computes an RFC 8785 canonical SHA-256 digest.
pub fn sha256<T: Serialize>(value: &T) -> serde_json::Result<ContentDigest> {
    let bytes = to_vec(value)?;
    Ok(sha256_bytes(&bytes))
}

/// Computes SHA-256 over bytes that are already canonical.
#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> ContentDigest {
    ContentDigest::new(Sha256::digest(bytes).into())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::ser::SerializeMap;

    use super::*;

    struct OrderedMap<'a>(&'a [(&'a str, serde_json::Value)]);

    impl Serialize for OrderedMap<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut map = serializer.serialize_map(Some(self.0.len()))?;
            for (key, value) in self.0 {
                map.serialize_entry(key, value)?;
            }
            map.end()
        }
    }

    #[rstest]
    fn test_canonical_rfc_style_vector() {
        let value = serde_json::json!({"b": false, "c": 12e1, "a": "Hello!"});
        assert_eq!(
            to_vec(&value).unwrap(),
            br#"{"a":"Hello!","b":false,"c":120}"#
        );
    }

    #[rstest]
    fn test_canonical_map_insertion_order_does_not_change_bytes() {
        let first = OrderedMap(&[("z", serde_json::json!(1)), ("a", serde_json::json!(2))]);
        let second = OrderedMap(&[("a", serde_json::json!(2)), ("z", serde_json::json!(1))]);
        assert_eq!(to_vec(&first).unwrap(), to_vec(&second).unwrap());
        assert_eq!(sha256(&first).unwrap(), sha256(&second).unwrap());
    }

    #[rstest]
    fn test_canonical_digest_changes_with_each_covered_field() {
        #[derive(Serialize)]
        struct Covered<'a> {
            instrument: &'a str,
            quantity: &'a str,
        }

        let baseline = Covered {
            instrument: "BTCUSDT.BINANCE",
            quantity: "0.5",
        };
        let changed_instrument = Covered {
            instrument: "ETHUSDT.BINANCE",
            quantity: "0.5",
        };
        let changed_quantity = Covered {
            instrument: "BTCUSDT.BINANCE",
            quantity: "0.25",
        };
        let baseline_digest = sha256(&baseline).unwrap();
        assert_ne!(baseline_digest, sha256(&changed_instrument).unwrap());
        assert_ne!(baseline_digest, sha256(&changed_quantity).unwrap());
    }
}
