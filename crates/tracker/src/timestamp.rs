use core::fmt;
use serde::Serialize;

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
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Timestamp", 2)?;
        state.serialize_field("secs", &self.0)?;
        let human = match jiff::Timestamp::from_second(self.0 as i64) {
            Ok(ts) => ts.to_string(),
            Err(_) => format!("{}s", self.0),
        };
        state.serialize_field("utc", &human)?;
        state.end()
    }
}
