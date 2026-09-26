pub struct _GasOutcome {
    pub gas_limit: u64,
    pub gas_used: u64,
}

impl _GasOutcome {
    pub fn _remaining(&self) -> u64 {
        self.gas_limit - self.gas_used
    }
}

pub fn _finalize_gas(gas_limit: u64, remaining: u64) -> _GasOutcome {
    _GasOutcome {
        gas_limit,
        gas_used: gas_limit - remaining,
    }
}
