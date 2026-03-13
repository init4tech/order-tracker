use alloy::primitives::U256;
use core::fmt;
use serde::Serialize;

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
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Amount", 2)?;
        state.serialize_field("raw", &self.0)?;
        state.serialize_field("decimal", &self.0.to_string())?;
        state.end()
    }
}
