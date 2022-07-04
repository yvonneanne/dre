use crate::network::Node;
use core::hash::Hash;
use counter::Counter;
use log::info;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Display, Formatter};
use std::iter::{FromIterator, IntoIterator};
use std::str::FromStr;
use strum::VariantNames;
use strum_macros::{EnumString, EnumVariantNames, ToString};

#[derive(
    ToString, EnumString, EnumVariantNames, Hash, Eq, PartialEq, Ord, PartialOrd, Clone, Serialize, Deserialize, Debug,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    NodeProvider,
    DataCenter,
    DataCenterOwner,
    City,
    Country,
    Continent,
}

impl Feature {
    pub fn variants() -> Vec<Self> {
        Feature::VARIANTS
            .iter()
            .map(|f| Feature::from_str(f).unwrap())
            .collect()
    }
}

#[derive(Eq, PartialEq, Clone, Serialize, Deserialize, Debug)]
pub struct NodeFeatures {
    pub feature_map: HashMap<Feature, String>,
}

impl NodeFeatures {
    pub fn get(&self, feature: &Feature) -> Option<String> {
        self.feature_map.get(feature).cloned()
    }

    #[cfg(test)]
    fn new_test_feature_set(value: &str) -> Self {
        let mut result = HashMap::new();
        for feature in Feature::variants() {
            result.insert(feature, value.to_string());
        }
        NodeFeatures { feature_map: result }
    }

    #[cfg(test)]
    fn with_feature_value(&self, feature: &Feature, value: &str) -> Self {
        let mut feature_map = self.feature_map.clone();
        feature_map.insert(feature.clone(), value.to_string());
        NodeFeatures { feature_map }
    }
}

impl FromIterator<(Feature, &'static str)> for NodeFeatures {
    fn from_iter<I: IntoIterator<Item = (Feature, &'static str)>>(iter: I) -> Self {
        Self {
            feature_map: HashMap::from_iter(iter.into_iter().map(|x| (x.0, String::from(x.1)))),
        }
    }
}

impl FromIterator<(Feature, std::string::String)> for NodeFeatures {
    fn from_iter<I: IntoIterator<Item = (Feature, std::string::String)>>(iter: I) -> Self {
        Self {
            feature_map: HashMap::from_iter(iter),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
/// This struct keeps the Nakamoto coefficients for each feature that we track
/// for the IC nodes https://crosstower.com/resources/education/nakamoto-coefficient/
/// For instance: [Feature::NodeProvider], [Feature::DataCenter], etc...
/// For a complete reference check [Feature]
pub struct NakamotoScore {
    coefficients: BTreeMap<Feature, f64>,
    controlled_nodes: BTreeMap<Feature, usize>,
    avg_linear: f64,
    avg_log2: f64,
    min: f64,
}

impl NakamotoScore {
    /// Build a new NakamotoScore object from a slice of [NodeFeatures].
    pub fn new_from_slice_node_features(slice_node_features: &[NodeFeatures]) -> Self {
        let mut features_to_nodes_map = BTreeMap::new();

        for feature in Feature::variants() {
            features_to_nodes_map.insert(feature, Vec::new());
        }

        // Convert a Vec<HashMap<Feature, Value>> into a Vec<HashMap<Feature,
        // Vec<Values>>
        for node_features in slice_node_features.iter() {
            for feature in Feature::variants() {
                let curr = features_to_nodes_map.get_mut(&feature).unwrap();
                curr.push(node_features.get(&feature));
            }
        }

        let nakamoto_calc = features_to_nodes_map.iter().map(|value| {
            // Turns a Vec<Features> into a Vec<(Feature, Number)>
            // where "Number" is the count of objects with the feature
            let counter: Vec<usize> = value.1.iter().collect::<Counter<_>>().iter().map(|x| *x.1).collect();

            (value.0.clone(), Self::nakamoto(&counter))
        });

        let scores = nakamoto_calc
            .clone()
            .map(|(f, n)| (f, n.0 as f64))
            .collect::<BTreeMap<Feature, f64>>();

        let controlled_nodes = nakamoto_calc
            .map(|(f, n)| (f, n.1))
            .collect::<BTreeMap<Feature, usize>>();

        NakamotoScore {
            coefficients: scores.clone(),
            controlled_nodes,
            avg_linear: scores.values().sum::<f64>() / scores.len() as f64,
            avg_log2: scores.values().map(|x| x.log2()).sum::<f64>() / scores.len() as f64,
            min: scores
                .values()
                .map(|x| if x.is_finite() { *x } else { 0. })
                .fold(1.0 / 0.0, |acc, x| if x < acc { x } else { acc }),
        }
    }

    /// Build a new NakamotoScore object from a slice of [Node]s.
    pub fn new_from_nodes(nodes: &[Node]) -> Self {
        Self::new_from_slice_node_features(&nodes.iter().map(|n| n.features.clone()).collect::<Vec<_>>())
    }

    /// The Nakamoto Coefficient represents the number of actors that would have
    /// to collude together to attack a subnet if they wanted to.
    /// This function takes a vector of numbers, where each number is the count
    /// of nodes in control of an actor.
    /// Returns:
    /// 1) a value between 1 and the total number of actors, to indicate
    ///    how many top actors would be needed to break the consensus
    ///    requirements
    /// 2) the number of nodes that the top actors control
    fn nakamoto(values: &[usize]) -> (usize, usize) {
        let mut values = values.to_owned();
        let total_subnet_nodes: usize = values.iter().sum();

        // The number of non-malicious actors that the consensus requires => 2f + 1
        // so at most 1/3 of the subnet nodes (actors) can be malicious (actually 1/3 -
        // 1) Source: https://dfinity.slack.com/archives/C01D7R95YJE/p1648480036670009?thread_ts=1648129258.551759&cid=C01D7R95YJE
        // > We use different thresholds in different places, depending on the security
        // we need. > Most things are fine with f+1, so >1/3rd, but some other
        // things like certification / CUPs > need to use 2f+1 (even if we only
        // assume that f can be corrupt) because we want to be more > resilient
        // against non-deterministic execution.
        let max_malicious_nodes = total_subnet_nodes / 3;

        // Reverse sort, go from actor with most to fewest repetitions.
        // The ultimate nakamoto coefficient is the number of different actors necessary
        // to reach max_malicious_actors
        values.sort_by(|a, b| b.cmp(a));

        let mut sum_actors: usize = 0;
        let mut sum_nodes: usize = 0;
        for actor_nodes in values {
            sum_actors += 1;
            sum_nodes = sum_nodes.saturating_add(actor_nodes);
            if sum_nodes > max_malicious_nodes {
                // Adding the current actor would break the consensus requirements, so stop
                // here.
                break;
            }
        }
        (sum_actors, sum_nodes)
    }

    /// An average of the linear nakamoto scores over all features
    pub fn score_avg_linear(&self) -> f64 {
        self.avg_linear
    }

    /// An average of the log2 nakamoto scores over all features
    pub fn score_avg_log2(&self) -> f64 {
        self.avg_log2
    }

    /// A minimum Nakamoto score over all features
    pub fn score_min(&self) -> f64 {
        self.min
    }

    /// Get a Map with all the features and the corresponding Nakamoto score
    pub fn scores_individual(&self) -> BTreeMap<Feature, f64> {
        self.coefficients.clone()
    }

    /// Get the Nakamoto score for a single feature
    pub fn score_feature(&self, feature: &Feature) -> Option<f64> {
        self.coefficients.get(feature).copied()
    }

    /// Get an upper bound on the number of nodes that are under control of the
    /// top actors For instance:
    /// - Most critical features Continent and Country both have Nakamoto score
    ///   == 1
    /// - Top continent actor(s) control 5 nodes
    /// - Top country actor(s) control 7 nodes
    /// In that case we would return 5 + 7 = 12
    /// However, if Country has Nakamoto score == 2, then we would return only 5
    pub fn control_power_critical_features(&self) -> Option<usize> {
        match self
            .coefficients
            .iter()
            .min_by(|x, y| x.1.partial_cmp(y.1).expect("partial_cmp failed"))
        {
            Some((_, score)) => {
                let critical_feats = self
                    .coefficients
                    .iter()
                    .filter(|(_, s)| *s <= score)
                    .map(|(f, _)| f)
                    .collect::<Vec<_>>();
                Some(
                    critical_feats
                        .iter()
                        .map(|f| self.controlled_nodes.get(f).unwrap_or(&0))
                        .sum::<usize>(),
                )
            }
            None => None,
        }
    }

    /// Return the number of nodes that the top actors control
    pub fn controlled_nodes(&self, feature: &Feature) -> Option<usize> {
        self.controlled_nodes.get(feature).copied()
    }
}

impl Ord for NakamotoScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("partial_cmp failed")
    }
}

impl PartialOrd for NakamotoScore {
    /// By default, the higher value will take the precedence
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Prefer improving the overall most-critical Feature
        info!("cmp self  {}", self);
        info!("cmp other {}", other);

        // Prefer higher "minimum" score across all features
        let mut cmp = self.score_min().partial_cmp(&other.score_min());

        if cmp != Some(Ordering::Equal) {
            return cmp;
        }

        // Compare the count of below-average coefficients
        // an prefer candidates that decrease the number of low-value coefficients
        let c1 = self
            .coefficients
            .values()
            .filter(|c| **c < self.score_avg_linear())
            .count();
        let c2 = other
            .coefficients
            .values()
            .filter(|c| **c < self.score_avg_linear())
            .count();
        cmp = c2.partial_cmp(&c1);

        if cmp != Some(Ordering::Equal) {
            return cmp;
        }

        // If the worst feature is the same for both candidates
        // and prefer candidates that maximizes all features
        for feature in Feature::variants() {
            let c1 = self.coefficients.get(&feature).unwrap_or(&1.0);
            let c2 = other.coefficients.get(&feature).unwrap_or(&1.0);
            if *c1 < self.score_avg_linear() || *c2 < self.score_avg_linear() {
                // Ensure that the new candidate does not decrease the critical features (that
                // are below the average)
                cmp = c2.partial_cmp(c1);

                if cmp != Some(Ordering::Equal) {
                    return cmp;
                }
            }
        }

        // Try to pick the candidate that *reduces* the number of nodes
        // controlled by the top actors
        cmp = other
            .control_power_critical_features()
            .partial_cmp(&self.control_power_critical_features());

        if cmp != Some(Ordering::Equal) {
            return cmp;
        }

        // Then try to increase the log2 avg
        cmp = self.score_avg_log2().partial_cmp(&other.score_avg_log2());

        if cmp != Some(Ordering::Equal) {
            return cmp;
        }

        // And finally try to increase the linear average
        self.score_avg_linear().partial_cmp(&other.score_avg_linear())
    }
}

impl PartialEq for NakamotoScore {
    fn eq(&self, other: &Self) -> bool {
        self.coefficients == other.coefficients && self.controlled_nodes == other.controlled_nodes
    }
}

impl Eq for NakamotoScore {}

impl Display for NakamotoScore {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NakamotoScore: min {:0.2} crit feat {} crit nodes {} avg log2 {:0.2} avg linear {:0.2} all coeff {:?}",
            self.min,
            self.coefficients
                .values()
                .filter(|c| **c < self.score_avg_linear())
                .count(),
            self.control_power_critical_features().unwrap_or(0),
            self.avg_log2,
            self.avg_linear,
            self.coefficients.values().enumerate().collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::network::{Subnet, SubnetChangeRequest};
    use ic_base_types::PrincipalId;
    use itertools::Itertools;
    use regex::Regex;

    use super::*;
    use super::{Node, NodeFeatures};

    #[test]
    fn computes_nakamoto_scores() {
        assert_eq!((0, 0), NakamotoScore::nakamoto(&[])); // empty vector
        assert_eq!((1, 1), NakamotoScore::nakamoto(&[1])); // one actor controls 1 node
        assert_eq!((1, 3), NakamotoScore::nakamoto(&[3])); // one actor controls 3 nodes
        for actors in 1..100 {
            // If 3..100 actors have 1 nodes each, then > 1/3 of the nodes needs to be
            // malicious
            assert_eq!(
                (1 + (actors / 3), 1 + (actors / 3)),
                NakamotoScore::nakamoto(&std::iter::repeat(1).take(actors).collect::<Vec<usize>>())
            );
        }
        // Included above as well, but more explicit for readability: 5/13 nodes need to
        // be malicious
        assert_eq!(
            (5, 5),
            NakamotoScore::nakamoto(&std::iter::repeat(1).take(13).collect::<Vec<usize>>())
        );
        assert_eq!((1, 3), NakamotoScore::nakamoto(&[1, 2, 3])); // one actor controls 3/6 nodes
        assert_eq!((1, 3), NakamotoScore::nakamoto(&[2, 3, 1])); // one actor controls 3/6 nodes
        assert_eq!((1, 3), NakamotoScore::nakamoto(&[3, 2, 1])); // one actor controls 3/6 nodes
        assert_eq!((2, 4), NakamotoScore::nakamoto(&[1, 2, 1, 2, 1])); // two top actors control 4/7 nodes
        assert_eq!((1, 5), NakamotoScore::nakamoto(&[1, 1, 2, 3, 5, 1])); // one top actor controls 5/13 nodes
        assert_eq!((2, 8), NakamotoScore::nakamoto(&[1, 1, 2, 3, 5, 1, 2])); // two top actors control 8/15 nodes
    }

    #[test]
    fn score_from_features() {
        let features = vec![NodeFeatures::new_test_feature_set("foo")];
        let score = NakamotoScore::new_from_slice_node_features(&features);

        let score_expected = NakamotoScore {
            coefficients: BTreeMap::from([
                (Feature::City, 1.),
                (Feature::Country, 1.),
                (Feature::Continent, 1.),
                (Feature::DataCenterOwner, 1.),
                (Feature::NodeProvider, 1.),
                (Feature::DataCenter, 1.),
            ]),
            controlled_nodes: BTreeMap::from([
                (Feature::City, 1),
                (Feature::Country, 1),
                (Feature::Continent, 1),
                (Feature::DataCenterOwner, 1),
                (Feature::NodeProvider, 1),
                (Feature::DataCenter, 1),
            ]),
            avg_linear: 1.,
            avg_log2: 0.,
            min: 1.,
        };
        assert_eq!(score, score_expected);
    }

    /// Generate a new Vec<Node> of len num_nodes, out of which
    /// num_dfinity_nodes are DFINITY-owned
    fn new_test_nodes(feat_prefix: &str, num_nodes: usize, num_dfinity_nodes: usize) -> Vec<Node> {
        let mut subnet_nodes = Vec::new();
        for i in 0..num_nodes {
            let dfinity_owned = i < num_dfinity_nodes;
            let node_features = NodeFeatures::new_test_feature_set(&format!("{} {}", feat_prefix, i));
            let node = Node::new_test_node(i as u64, node_features, dfinity_owned);
            subnet_nodes.push(node);
        }
        subnet_nodes
    }

    /// Generate a new Vec<Node> and override some feature values
    fn new_test_nodes_with_overrides(
        feat_prefix: &str,
        node_number_start: usize,
        num_nodes: usize,
        num_dfinity_nodes: usize,
        feature_to_override: (&Feature, &[&str]),
    ) -> Vec<Node> {
        let mut subnet_nodes = Vec::new();
        for i in 0..num_nodes {
            let dfinity_owned = i < num_dfinity_nodes;
            let (override_feature, override_val) = feature_to_override;
            let node_features = match override_val.get(i) {
                Some(override_val) => NodeFeatures::new_test_feature_set(&format!("{} {}", feat_prefix, i))
                    .with_feature_value(override_feature, override_val),
                None => NodeFeatures::new_test_feature_set(&format!("feat {}", i)),
            };
            let node = Node::new_test_node((node_number_start + i) as u64, node_features, dfinity_owned);
            subnet_nodes.push(node);
        }
        subnet_nodes
    }

    /// Generate a new test subnet with num_nodes, out of which
    /// num_dfinity_nodes are DFINITY-owned
    fn new_test_subnet(subnet_num: u64, num_nodes: usize, num_dfinity_nodes: usize) -> Subnet {
        Subnet {
            id: PrincipalId::new_subnet_test_id(subnet_num),
            nodes: new_test_nodes("feat", num_nodes, num_dfinity_nodes),
        }
    }

    /// Generate a new test subnet with feature overrides
    fn new_test_subnet_with_overrides(
        subnet_num: u64,
        node_number_start: usize,
        num_nodes: usize,
        num_dfinity_nodes: usize,
        feature_to_override: (&Feature, &[&str]),
    ) -> Subnet {
        Subnet {
            id: PrincipalId::new_subnet_test_id(subnet_num),
            nodes: new_test_nodes_with_overrides(
                "feat",
                node_number_start,
                num_nodes,
                num_dfinity_nodes,
                feature_to_override,
            ),
        }
    }

    #[test]
    fn test_business_rules_pass() {
        // If there is exactly one DFINITY-owned node in a subnet ==> pass
        new_test_subnet(0, 7, 1).check_business_rules().unwrap();
        new_test_subnet(0, 12, 1).check_business_rules().unwrap();
        new_test_subnet(0, 13, 1).check_business_rules().unwrap();
        new_test_subnet(0, 13, 2).check_business_rules().unwrap();
        new_test_subnet(0, 14, 1).check_business_rules().unwrap();
        new_test_subnet(0, 25, 1).check_business_rules().unwrap();
        new_test_subnet(0, 25, 2).check_business_rules().unwrap();
        new_test_subnet(0, 26, 1).check_business_rules().unwrap();
        new_test_subnet(0, 26, 2).check_business_rules().unwrap();
        new_test_subnet(0, 27, 2).check_business_rules().unwrap();
        new_test_subnet(0, 38, 2).check_business_rules().unwrap();
        new_test_subnet(0, 38, 3).check_business_rules().unwrap();
        new_test_subnet(0, 39, 3).check_business_rules().unwrap();
        new_test_subnet(0, 40, 3).check_business_rules().unwrap();
        new_test_subnet(0, 51, 3).check_business_rules().unwrap();
        new_test_subnet(0, 51, 4).check_business_rules().unwrap();
        new_test_subnet(0, 52, 4).check_business_rules().unwrap();
        new_test_subnet(0, 53, 4).check_business_rules().unwrap();
    }

    #[test]
    fn test_business_rules_fail() {
        // If there are no DFINITY-owned node in a small subnet ==> fail with an
        // expected error message
        assert_eq!(
            new_test_subnet(0, 0, 0).check_business_rules().unwrap_err().to_string(),
            "DFINITY-owned node missing".to_string()
        );
        assert_eq!(
            new_test_subnet(0, 2, 0).check_business_rules().unwrap_err().to_string(),
            "DFINITY-owned node missing".to_string()
        );
    }

    #[test]
    fn extend_feature_set_group() {
        let subnet_initial = new_test_subnet(0, 12, 1);
        let nodes_available = new_test_nodes("spare", 1, 0);

        let extended_subnet = subnet_initial.new_extended_subnet(1, &nodes_available).unwrap();
        assert_eq!(
            extended_subnet.nodes,
            subnet_initial
                .nodes
                .iter()
                .chain(nodes_available.iter())
                .cloned()
                .collect::<Vec<_>>()
        );
    }

    fn json_file_read_checked<T>(file_path: &std::path::PathBuf) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let file =
            std::fs::File::open(file_path).unwrap_or_else(|_| panic!("Error opening the file {}", file_path.display()));
        let reader = std::io::BufReader::new(file);

        serde_json::from_reader(reader).expect("Input JSON was not well-formatted")
    }

    #[test]
    fn subnet_usa_dominance() {
        let subnet_initial = new_test_subnet_with_overrides(
            0,
            0,
            13,
            1,
            (
                &Feature::Country,
                &[
                    "US", "US", "US", "US", "US", "US", "US", "US", "US", "CH", "BE", "SG", "SI",
                ],
            ),
        );
        assert_eq!(
            subnet_initial.check_business_rules().unwrap_err().to_string(),
            "Feature 'country' controls 9 of nodes, which is >= 8 (2/3 of all) nodes".to_string()
        );
        let nodes_available =
            new_test_nodes_with_overrides("spare", 13, 3, 0, (&Feature::Country, &["US", "RO", "JP"]));

        println!(
            "initial {} Countries {:?}",
            subnet_initial,
            subnet_initial
                .nodes
                .iter()
                .map(|n| n.get_feature(&Feature::Country))
                .collect::<Vec<_>>()
        );

        let subnet_change_req = SubnetChangeRequest::new(subnet_initial, nodes_available, Vec::new(), Vec::new(), 0);
        let subnet_change = subnet_change_req.optimize(2).unwrap();
        let optimized_subnet = subnet_change.after();

        let countries_after = optimized_subnet
            .nodes
            .iter()
            .map(|n| n.get_feature(&Feature::Country))
            .sorted()
            .collect::<Vec<_>>();

        println!("optimized {} Countries {:?}", optimized_subnet, countries_after);
        assert_eq!(optimized_subnet.nakamoto_score().score_min(), 1.);

        // Two US nodes were removed
        assert_eq!(
            countries_after,
            vec!["BE", "CH", "JP", "RO", "SG", "SI", "US", "US", "US", "US", "US", "US", "US"]
        );
    }

    #[test]
    fn subnet_optimize_node_providers() {
        let subnet_initial = new_test_subnet_with_overrides(
            0,
            0,
            7,
            1,
            (
                &Feature::NodeProvider,
                &["NP1", "NP2", "NP2", "NP2", "NP3", "NP4", "NP5"],
            ),
        );
        assert_eq!(
            subnet_initial.check_business_rules().unwrap_err().to_string(),
            "A single Node Provider can halt a subnet".to_string()
        );
        let nodes_available =
            new_test_nodes_with_overrides("spare", 7, 2, 0, (&Feature::NodeProvider, &["NP6", "NP7"]));

        println!(
            "initial {} NPs {:?}",
            subnet_initial,
            subnet_initial
                .nodes
                .iter()
                .map(|n| n.get_feature(&Feature::NodeProvider))
                .collect::<Vec<_>>()
        );

        let subnet_change_req = SubnetChangeRequest::new(subnet_initial, nodes_available, Vec::new(), Vec::new(), 0);
        let subnet_change = subnet_change_req.optimize(2).unwrap();
        let optimized_subnet = subnet_change.after();

        let nps_after = optimized_subnet
            .nodes
            .iter()
            .map(|n| n.get_feature(&Feature::NodeProvider))
            .sorted()
            .collect::<Vec<_>>();

        println!("optimized {} NPs {:?}", optimized_subnet, nps_after);
        assert_eq!(optimized_subnet.nakamoto_score().score_min(), 3.);

        // Check that the selected nodes are providing the maximum uniformness (use all
        // NPs)
        assert_eq!(nps_after, vec!["NP1", "NP2", "NP3", "NP4", "NP5", "NP6", "NP7"]);
    }

    #[test]
    fn subnet_optimize_prefer_non_dfinity() {
        let subnet_initial = new_test_subnet_with_overrides(
            0,
            0,
            7,
            1,
            (
                &Feature::NodeProvider,
                &["NP1", "NP2", "NP2", "NP3", "NP4", "NP4", "NP5"],
            ),
        );
        subnet_initial.check_business_rules().unwrap();

        // There are 2 spare nodes, but both are DFINITY
        let nodes_available =
            new_test_nodes_with_overrides("spare", 7, 2, 2, (&Feature::NodeProvider, &["NP6", "NP7"]));

        println!(
            "initial {} NPs {:?}",
            subnet_initial,
            subnet_initial
                .nodes
                .iter()
                .map(|n| n.get_feature(&Feature::NodeProvider))
                .collect::<Vec<_>>()
        );

        let subnet_change_req = SubnetChangeRequest::new(subnet_initial, nodes_available, Vec::new(), Vec::new(), 0);
        let subnet_change = subnet_change_req.optimize(2).unwrap();
        let optimized_subnet = subnet_change.after();

        let nps_after = optimized_subnet
            .nodes
            .iter()
            .map(|n| n.get_feature(&Feature::NodeProvider))
            .sorted()
            .collect::<Vec<_>>();

        println!("optimized {} NPs {:?}", optimized_subnet, nps_after);
        assert_eq!(optimized_subnet.nakamoto_score().score_min(), 2.);
        // The nodes (NPs) are unchanged
        assert_eq!(nps_after, vec!["NP1", "NP2", "NP2", "NP3", "NP4", "NP4", "NP5"]);
    }

    #[test]
    fn subnet_uzr34_extend() {
        let mut d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("test_data");

        // Read the subnet snapshot from a file
        let subnet_all = json_file_read_checked::<mercury_management_types::Subnet>(&d.join("subnet-uzr34.json"));
        // Convert the subnet snapshot to the "Subnet" struct
        let subnet_all: Subnet = Subnet::from(subnet_all);
        let re_unhealthy_nodes = Regex::new(r"^(gp7wd|e4ysi|qhz4y|2fbvp)-.+$").unwrap();
        let subnet_healthy: Subnet = Subnet {
            id: subnet_all.id,
            nodes: subnet_all
                .nodes
                .iter()
                .cloned()
                .filter(|n| !re_unhealthy_nodes.is_match(&n.id.to_string()))
                .collect(),
        };

        let available_nodes =
            json_file_read_checked::<Vec<mercury_management_types::Node>>(&d.join("available-nodes.json"));

        let available_nodes = available_nodes
            .iter()
            .sorted_by(|a, b| a.principal.cmp(&b.principal))
            .filter(|n| n.subnet == None && n.proposal.is_none())
            .map(Node::from)
            .collect::<Vec<_>>();

        subnet_healthy
            .check_business_rules()
            .expect("Check business rules failed");

        println!("Initial subnet {}", subnet_healthy);
        println!("Check business rules: {:?}", subnet_healthy.check_business_rules());
        let nakamoto_score_before = subnet_healthy.nakamoto_score();
        println!("NakamotoScore before {}", nakamoto_score_before);

        let extended_subnet = subnet_healthy.new_extended_subnet(4, &available_nodes).unwrap();
        println!("{}", extended_subnet);
        let nakamoto_score_after = extended_subnet.nakamoto_score();
        println!("NakamotoScore after {}", nakamoto_score_after);

        // Check against the close-to-optimal values obtained by data analysis
        assert!(nakamoto_score_after.score_min() >= 1.0);
        assert!(nakamoto_score_after.control_power_critical_features().unwrap() <= 24);
        assert!(nakamoto_score_after.score_avg_linear() >= 3.0);
        assert!(nakamoto_score_after.score_avg_log2() >= 1.32);
    }
}
