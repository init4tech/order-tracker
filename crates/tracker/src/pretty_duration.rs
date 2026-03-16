use core::fmt;
use jiff::{SignedDuration, Unit};
use serde::Serialize;
use std::time::Duration;

/// A duration that serializes into an ISO 8601 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrettyDuration(SignedDuration);

impl PrettyDuration {
    /// Create a new `PrettyDuration` from the given `duration`.
    pub const fn new(duration: Duration) -> Self {
        Self(SignedDuration::from_nanos_i128(duration.as_nanos() as i128))
    }
}

impl From<Duration> for PrettyDuration {
    fn from(duration: Duration) -> Self {
        Self::new(duration)
    }
}

impl fmt::Display for PrettyDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rounded = self
            .0
            .round(Unit::Millisecond)
            .expect("should round as milliseconds are a valid rounding unit");
        write!(formatter, "{rounded:#}")
    }
}

impl Serialize for PrettyDuration {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_string().serialize(serializer)
    }
}
