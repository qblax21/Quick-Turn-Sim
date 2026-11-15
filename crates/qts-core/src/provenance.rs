// SPDX-License-Identifier: Apache-2.0
//! Where a parameter value came from and how far it can be trusted.
//!
//! Every physical parameter in a data file is a table
//! `{ value, source, confidence }`. The loader (in `qts-sim`) strips these
//! down to plain values for the model structs and keeps a [`Provenance`]
//! record for each, so reports can list what is still a placeholder.

use std::fmt;

/// How far a parameter value can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// A stand-in chosen to make the model run. Carries no information
    /// about any real vehicle.
    Placeholder,
    /// Derived from public class-typical figures or engineering judgement.
    Estimated,
    /// Taken from a measurement the project holds.
    Measured,
}

impl Confidence {
    /// Parses the lowercase name used in data files.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "placeholder" => Some(Confidence::Placeholder),
            "estimated" => Some(Confidence::Estimated),
            "measured" => Some(Confidence::Measured),
            _ => None,
        }
    }

    /// The lowercase name used in data files.
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Placeholder => "placeholder",
            Confidence::Estimated => "estimated",
            Confidence::Measured => "measured",
        }
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Provenance of one parameter in a data file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// Dotted path of the parameter inside its file, such as
    /// `suspension.front.spring_rate_n_per_m`.
    pub path: String,
    /// Free-text origin of the value.
    pub source: String,
    /// Trust level.
    pub confidence: Confidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_order_by_trust() {
        for c in [
            Confidence::Placeholder,
            Confidence::Estimated,
            Confidence::Measured,
        ] {
            assert_eq!(Confidence::parse(c.as_str()), Some(c));
        }
        assert_eq!(Confidence::parse("Measured"), None);
        assert!(Confidence::Placeholder < Confidence::Measured);
    }
}
