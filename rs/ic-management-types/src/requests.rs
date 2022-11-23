use crate::MinNakamotoCoefficients;
use ic_base_types::PrincipalId;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct MembershipReplaceRequest {
    pub target: ReplaceTarget,
    pub heal: bool,
    pub optimize: Option<usize>,
    pub exclude: Option<Vec<String>>,
    pub include: Option<Vec<PrincipalId>>,
    pub min_nakamoto_coefficients: Option<MinNakamotoCoefficients>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplaceTarget {
    /// Subnet targeted for replacements
    Subnet(PrincipalId),
    /// Nodes on the same subnet that need to be replaced for other reasons
    Nodes {
        nodes: Vec<PrincipalId>,
        motivation: String,
    },
}

#[derive(Serialize, Deserialize)]
pub struct SubnetCreateRequest {
    pub size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SubnetExtendRequest {
    pub subnet: PrincipalId,
    pub size: usize,
    pub exclude: Option<Vec<String>>,
    pub include: Option<Vec<PrincipalId>>,
}
