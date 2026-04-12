use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum StreamingType {
    Gather,
    Redistribute,
    Broadcast,
    LocalRedistribute,
    LocalBroadcast,
    LocalGather,
    LocalRoundrobin,
    SplitRedistribute,
    SplitBroadcast,
    RangeRedistribute,
    ListRedistribute,
    Hybrid,
    PartRedistributePartBroadcast,
    PartRedistributePartRoundrobin,
    PartRedistributePartLocal,
    PartLocalPartBroadcast,
    Unknown(String),
}

impl fmt::Display for StreamingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(s) => write!(f, "{}", s),
            other => write!(f, "{:?}", other),
        }
    }
}

impl FromStr for StreamingType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_uppercase().as_str() {
            "GATHER" => Self::Gather,
            "REDISTRIBUTE" => Self::Redistribute,
            "BROADCAST" => Self::Broadcast,
            "LOCAL REDISTRIBUTE" => Self::LocalRedistribute,
            "LOCAL BROADCAST" => Self::LocalBroadcast,
            "LOCAL GATHER" => Self::LocalGather,
            "LOCAL ROUNDROBIN" => Self::LocalRoundrobin,
            "SPLIT REDISTRIBUTE" => Self::SplitRedistribute,
            "SPLIT BROADCAST" => Self::SplitBroadcast,
            "RANGE REDISTRIBUTE" => Self::RangeRedistribute,
            "LIST REDISTRIBUTE" => Self::ListRedistribute,
            "HYBRID" => Self::Hybrid,
            "PART REDISTRIBUTE PART BROADCAST" => Self::PartRedistributePartBroadcast,
            "PART REDISTRIBUTE PART ROUNDROBIN" => Self::PartRedistributePartRoundrobin,
            "PART REDISTRIBUTE PART LOCAL" => Self::PartRedistributePartLocal,
            "PART LOCAL PART BROADCAST" => Self::PartLocalPartBroadcast,
            other => Self::Unknown(other.to_string()),
        })
    }
}
