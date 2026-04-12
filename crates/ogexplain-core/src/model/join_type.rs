use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum JoinType {
    Inner,
    Left,
    Full,
    Right,
    Semi,
    Anti,
    RightSemi,
    RightAnti,
    LeftAntiFull,
    RightAntiFull,
    LeftAntiSemiNotIn,
}

impl fmt::Display for JoinType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Inner => "Inner",
            Self::Left => "Left",
            Self::Full => "Full",
            Self::Right => "Right",
            Self::Semi => "Semi",
            Self::Anti => "Anti",
            Self::RightSemi => "Right Semi",
            Self::RightAnti => "Right Anti",
            Self::LeftAntiFull => "Left Anti Full",
            Self::RightAntiFull => "Right Anti Full",
            Self::LeftAntiSemiNotIn => "Left Anti Semi Not In",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for JoinType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "Inner" => Ok(Self::Inner),
            "Left" => Ok(Self::Left),
            "Full" => Ok(Self::Full),
            "Right" => Ok(Self::Right),
            "Semi" => Ok(Self::Semi),
            "Anti" => Ok(Self::Anti),
            "Right Semi" => Ok(Self::RightSemi),
            "Right Anti" => Ok(Self::RightAnti),
            "Left Anti Full" => Ok(Self::LeftAntiFull),
            "Right Anti Full" => Ok(Self::RightAntiFull),
            "Left Anti Semi Not In" => Ok(Self::LeftAntiSemiNotIn),
            other => Err(format!("unknown join type: {}", other)),
        }
    }
}
