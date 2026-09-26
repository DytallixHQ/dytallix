use crate::risk::pulseguard::graph::dag::DynamicDag;
use crate::risk::pulseguard::RiskScore;
use serde_json::json;

pub fn add_dummy_path(rs: &mut RiskScore) {
    rs.paths.push(json!({"hops":[{"addr":"A","edge":"Transfer"},{"addr":"B","edge":"Bridge"},{"addr":"C"}],"score":0.77}));
}

pub fn add_path_from_dag(rs: &mut RiskScore, dag: &DynamicDag, start: &str) {
    if let Some(p) = dag.suspicious_path(start, 4) {
        rs.paths.push(json!({"path":p,"heuristic":"bridge_chain"}));
    }
}
