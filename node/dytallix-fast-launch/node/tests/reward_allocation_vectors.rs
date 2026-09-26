use dytallix_fast_node::runtime::reward_allocation::allocate_reward_budget;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Vector {
    budget: String,
    weights: BTreeMap<String, String>,
    entitlements: BTreeMap<String, String>,
    reserve: String,
}

#[test]
fn agrees_with_independent_arbitrary_precision_vectors() {
    let vectors: Vec<Vector> =
        serde_json::from_str(include_str!("fixtures/reward_math_vectors.json")).unwrap();
    assert_eq!(vectors.len(), 80);
    for (index, vector) in vectors.into_iter().enumerate() {
        let snapshot = vector
            .weights
            .into_iter()
            .map(|(address, weight)| (address, weight.parse().unwrap()))
            .collect();
        let expected: BTreeMap<String, u128> = vector
            .entitlements
            .into_iter()
            .map(|(address, amount)| (address, amount.parse().unwrap()))
            .collect();
        let actual = allocate_reward_budget(vector.budget.parse().unwrap(), &snapshot).unwrap();
        assert_eq!(actual.entitlements, expected, "vector {index}");
        assert_eq!(
            actual.reserve,
            vector.reserve.parse::<u128>().unwrap(),
            "vector {index}"
        );
    }
}
