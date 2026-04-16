//! `ActionName` newtype — a string that is guaranteed to be in the
//! approved SysKnife action catalogue.
//!
//! Construction (`ActionName::parse`) rejects unknown names at the
//! boundary so downstream code can rely on the type rather than
//! re-checking the allowed set.

use serde::Serialize;
use std::fmt;

use crate::planning_tools::propose_plan::KNOWN_ACTIONS;

/// A validated action name from the approved SysKnife action catalogue.
///
/// Can only be constructed through [`ActionName::parse`], which rejects
/// any string not present in [`KNOWN_ACTIONS`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ActionName(String);

impl ActionName {
    /// Parse a string into a validated `ActionName`.
    ///
    /// Returns `Err` if `name` is not in the approved action catalogue.
    pub fn parse(name: impl Into<String>) -> Result<Self, UnknownActionName> {
        let name = name.into();
        if KNOWN_ACTIONS.iter().any(|(n, _)| *n == name.as_str()) {
            Ok(Self(name))
        } else {
            Err(UnknownActionName(name))
        }
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ActionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Error returned when an unknown action name is passed to
/// [`ActionName::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownActionName(pub String);

impl fmt::Display for UnknownActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown action name: '{}'", self.0)
    }
}

impl std::error::Error for UnknownActionName {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_action_parses() {
        let name = ActionName::parse("GetSystemState").unwrap();
        assert_eq!(name.as_str(), "GetSystemState");
    }

    #[test]
    fn unknown_action_rejected() {
        let err = ActionName::parse("RunShellCommand").unwrap_err();
        assert_eq!(err.0, "RunShellCommand");
    }

    #[test]
    fn all_known_actions_parse() {
        for &(action, _) in KNOWN_ACTIONS {
            ActionName::parse(action)
                .unwrap_or_else(|_| panic!("KNOWN_ACTION '{action}' should parse"));
        }
    }

    #[test]
    fn display_shows_name() {
        let name = ActionName::parse("RebaseSystem").unwrap();
        assert_eq!(format!("{name}"), "RebaseSystem");
    }
}
