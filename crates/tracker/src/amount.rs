use alloy::primitives::U256;
use core::fmt;
use serde::{Deserialize, Serialize, de::Deserializer, ser::SerializeStruct};

/// A U256 amount that serializes with both the raw value and a human-readable decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(U256);

impl Amount {
    /// Create a new amount from a U256.
    pub const fn new(value: U256) -> Self {
        Self(value)
    }

    /// Get the raw U256 value.
    pub const fn raw(&self) -> U256 {
        self.0
    }
}

impl From<U256> for Amount {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Serialize for Amount {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Amount", 2)?;
        state.serialize_field("raw", &self.0)?;
        state.serialize_field("decimal", &self.0.to_string())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            raw: U256,
        }
        Helper::deserialize(deserializer).map(|helper| Self(helper.raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let amount = Amount::new(U256::from(123_456_789u64));
        let json = serde_json::to_string(&amount).unwrap();
        let deserialized: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(amount, deserialized);
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn roundtrip_zero() {
        let amount = Amount::new(U256::ZERO);
        let json = serde_json::to_string(&amount).unwrap();
        let deserialized: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(amount, deserialized);
    }

    #[test]
    fn roundtrip_max() {
        let amount = Amount::new(U256::MAX);
        let json = serde_json::to_string(&amount).unwrap();
        let deserialized: Amount = serde_json::from_str(&json).unwrap();
        assert_eq!(amount, deserialized);
    }
}
