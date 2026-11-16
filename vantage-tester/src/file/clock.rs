use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use vantage_core::{Result, VantageError, error};

/// A step's `clock:` move, `+<n><unit>`: `+90m`, `+3h`, `+2d`, `+1w`, `+1mo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ClockOffset {
    pub amount: u32,
    pub unit: ClockUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
}

impl FromStr for ClockOffset {
    type Err = VantageError;

    fn from_str(s: &str) -> Result<Self> {
        let bad = || error!("a clock offset is `+<n><m|h|d|w|mo>`, like `+2d`", got = s);
        let rest = s.trim().strip_prefix('+').ok_or_else(bad)?;
        let split = rest.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
        let (digits, unit) = rest.split_at(split);
        let amount: u32 = digits.parse().map_err(|_| bad())?;
        let unit = match unit {
            "m" => ClockUnit::Minutes,
            "h" => ClockUnit::Hours,
            "d" => ClockUnit::Days,
            "w" => ClockUnit::Weeks,
            "mo" => ClockUnit::Months,
            _ => return Err(bad()),
        };
        Ok(ClockOffset { amount, unit })
    }
}

impl TryFrom<String> for ClockOffset {
    type Error = VantageError;
    fn try_from(s: String) -> Result<Self> {
        s.parse()
    }
}

impl fmt::Display for ClockOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = match self.unit {
            ClockUnit::Minutes => "m",
            ClockUnit::Hours => "h",
            ClockUnit::Days => "d",
            ClockUnit::Weeks => "w",
            ClockUnit::Months => "mo",
        };
        write!(f, "+{}{unit}", self.amount)
    }
}
