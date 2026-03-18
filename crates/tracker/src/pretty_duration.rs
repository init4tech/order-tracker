use core::fmt;
use jiff::{SignedDuration, Span, Unit};
use serde::{Deserialize, Serialize, de::Deserializer};
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

impl<'de> Deserialize<'de> for PrettyDuration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let string = String::deserialize(deserializer)?;
        let (negated, iso_str) = match string.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, string.as_str()),
        };
        let span: Span = iso_str.parse().map_err(serde::de::Error::custom)?;
        let mut nanos = i128::from(span.get_hours()) * 3_600_000_000_000
            + i128::from(span.get_minutes()) * 60_000_000_000
            + i128::from(span.get_seconds()) * 1_000_000_000
            + i128::from(span.get_milliseconds()) * 1_000_000
            + i128::from(span.get_microseconds()) * 1_000
            + i128::from(span.get_nanoseconds());
        if negated {
            nanos = -nanos;
        }
        Ok(Self(SignedDuration::from_nanos_i128(nanos)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_seconds() {
        let duration = PrettyDuration::new(Duration::from_secs(30));
        let json = serde_json::to_string(&duration).unwrap();
        let deserialized: PrettyDuration = serde_json::from_str(&json).unwrap();
        assert_eq!(duration, deserialized);
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn roundtrip_millis() {
        let duration = PrettyDuration::new(Duration::from_millis(1500));
        let json = serde_json::to_string(&duration).unwrap();
        let deserialized: PrettyDuration = serde_json::from_str(&json).unwrap();
        assert_eq!(duration, deserialized);
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }

    #[test]
    fn roundtrip_zero() {
        let duration = PrettyDuration::new(Duration::ZERO);
        let json = serde_json::to_string(&duration).unwrap();
        let deserialized: PrettyDuration = serde_json::from_str(&json).unwrap();
        assert_eq!(duration, deserialized);
    }

    #[test]
    fn negative_roundtrip_is_stable() {
        let json = "\"-PT5m\"";
        let deserialized: PrettyDuration = serde_json::from_str(json).unwrap();
        let reserialized = serde_json::to_string(&deserialized).unwrap();
        let deserialized2: PrettyDuration = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(deserialized, deserialized2);
    }
}
