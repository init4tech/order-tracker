use core::fmt;
use serde::{Deserialize, Serialize, de::Deserializer, ser::SerializeStruct};

/// A unix timestamp (seconds) that serializes with both the raw integer and an ISO 8601 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Create a new timestamp from unix seconds.
    pub const fn new(secs: u64) -> Self {
        Self(secs)
    }

    /// Get the raw unix seconds value.
    pub const fn as_secs(&self) -> u64 {
        self.0
    }
}

impl From<u64> for Timestamp {
    fn from(secs: u64) -> Self {
        Self(secs)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match jiff::Timestamp::from_second(self.0 as i64) {
            Ok(ts) => write!(formatter, "{ts}"),
            Err(_) => write!(formatter, "{}s", self.0),
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Timestamp", 2)?;
        state.serialize_field("secs_since_epoch", &self.0)?;
        let human = match jiff::Timestamp::from_second(self.0 as i64) {
            Ok(ts) => ts.to_string(),
            Err(_) => format!("{}s", self.0),
        };
        state.serialize_field("utc", &human)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            secs_since_epoch: u64,
        }
        Helper::deserialize(deserializer).map(|helper| Self(helper.secs_since_epoch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let ts = Timestamp::new(1_700_000_000);
        let json = serde_json::to_string(&ts).unwrap();
        let deserialized: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, deserialized);
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn roundtrip_zero() {
        let ts = Timestamp::new(0);
        let json = serde_json::to_string(&ts).unwrap();
        let deserialized: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, deserialized);
    }
}
