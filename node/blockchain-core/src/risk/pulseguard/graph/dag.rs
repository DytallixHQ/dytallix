use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EdgeType {
    Transfer,
    Approve,
    Swap,
    Bridge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub to: String,
    pub edge_type: EdgeType,
    pub timestamp: u64,
    pub amount: u128,
}

#[derive(Default, Debug)]
pub struct DynamicDag {
    pub adj: HashMap<String, Vec<Edge>>,
    pub last_seen: HashMap<String, u64>,
}

impl DynamicDag {
    pub fn add_edge(&mut self, from: String, edge: Edge) {
        self.last_seen.insert(from.clone(), edge.timestamp);
        self.last_seen.insert(edge.to.clone(), edge.timestamp);
        self.adj.entry(from).or_default().push(edge);
    }

    pub fn fanout_k(&self, node: &str, k: usize) -> usize {
        let mut visited = std::collections::HashSet::new();
        let mut q = VecDeque::new();
        q.push_back((node.to_string(), 0));

        while let Some((n, d)) = q.pop_front() {
            if d == k {
                continue;
            }
            if let Some(edges) = self.adj.get(&n) {
                for e in edges {
                    if visited.insert(e.to.clone()) {
                        q.push_back((e.to.clone(), d + 1));
                    }
                }
            }
        }
        visited.len()
    }

    pub fn suspicious_path(&self, start: &str, max_hops: usize) -> Option<Vec<String>> {
        let mut path = vec![start.to_string()];
        let mut current = start.to_string();

        for _ in 0..max_hops {
            if let Some(edges) = self.adj.get(&current) {
                if let Some(e) = edges
                    .iter()
                    .find(|e| matches!(e.edge_type, EdgeType::Bridge | EdgeType::Swap))
                {
                    current = e.to.clone();
                    path.push(current.clone());
                    continue;
                }
            }
            break;
        }

        if path.len() > 1 {
            Some(path)
        } else {
            None
        }
    }
}
