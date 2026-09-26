//! Synthetic governance ownership records. No legacy reward mutation API is used.
//! These fixtures do not establish funded production stake or authorize migration.
use super::StakingModule;

pub trait StakeFixture {
    fn seed_total_stake(&mut self, amount: u128);
    fn seed_delegator_stake(&mut self, owner: &str, amount: u128);
}
impl StakeFixture for StakingModule {
    fn seed_total_stake(&mut self, amount: u128) {
        assert_eq!(
            self.reward_index, 0,
            "fixture must not rewrite legacy liabilities"
        );
        self.storage
            .db
            .put("staking:total_stake", bincode::serialize(&amount).unwrap())
            .unwrap();
        self.total_stake = amount;
    }
    fn seed_delegator_stake(&mut self, owner: &str, amount: u128) {
        let mut record = self.load_delegator_record(owner);
        assert_eq!(record.last_reward_index, 0);
        assert_eq!(record.accrued_rewards, 0);
        assert_eq!(self.reward_index, 0);
        record.stake_amount = amount;
        self.storage
            .db
            .put(
                format!("staking:delegator:{owner}"),
                bincode::serialize(&record).unwrap(),
            )
            .unwrap();
    }
}
