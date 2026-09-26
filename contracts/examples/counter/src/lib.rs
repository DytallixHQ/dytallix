use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterContract {
    pub owner: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecuteMsg {
    Increment { by: u64 },
    Decrement { by: u64 },
    Reset { value: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryMsg {
    GetCount,
    GetOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryResponse {
    Count(i64),
    Owner(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CounterError {
    #[error("only the owner can reset the counter")]
    Unauthorized,
    #[error("counter underflow")]
    Underflow,
}

impl CounterContract {
    pub fn instantiate(owner: impl Into<String>, start_at: i64) -> Self {
        Self {
            owner: owner.into(),
            count: start_at,
        }
    }

    pub fn execute(&mut self, sender: &str, msg: ExecuteMsg) -> Result<(), CounterError> {
        match msg {
            ExecuteMsg::Increment { by } => {
                self.count += by as i64;
                Ok(())
            }
            ExecuteMsg::Decrement { by } => {
                let by = by as i64;
                if self.count < by {
                    return Err(CounterError::Underflow);
                }
                self.count -= by;
                Ok(())
            }
            ExecuteMsg::Reset { value } => {
                if sender != self.owner {
                    return Err(CounterError::Unauthorized);
                }
                self.count = value;
                Ok(())
            }
        }
    }

    pub fn query(&self, msg: QueryMsg) -> QueryResponse {
        match msg {
            QueryMsg::GetCount => QueryResponse::Count(self.count),
            QueryMsg::GetOwner => QueryResponse::Owner(self.owner.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_can_reset_counter() {
        let mut contract = CounterContract::instantiate("owner", 5);
        contract
            .execute("alice", ExecuteMsg::Increment { by: 7 })
            .unwrap();
        assert_eq!(contract.query(QueryMsg::GetCount), QueryResponse::Count(12));

        contract
            .execute("owner", ExecuteMsg::Reset { value: 3 })
            .unwrap();
        assert_eq!(contract.query(QueryMsg::GetCount), QueryResponse::Count(3));
    }

    #[test]
    fn non_owner_cannot_reset_counter() {
        let mut contract = CounterContract::instantiate("owner", 1);
        let err = contract
            .execute("alice", ExecuteMsg::Reset { value: 0 })
            .unwrap_err();
        assert_eq!(err, CounterError::Unauthorized);
    }
}
