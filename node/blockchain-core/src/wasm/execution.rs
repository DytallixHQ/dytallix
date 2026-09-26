use super::{
    contract_registry::_ContractStore, engine::WasmEngine, gas_meter::_finalize_gas,
    types::ContractInstance,
};
use anyhow::{anyhow, Result};
use wasmtime::Func;

pub struct _WasmExecutor<'a, S: _ContractStore> {
    pub engine: &'a WasmEngine,
    pub store: &'a mut S,
}

impl<'a, S: _ContractStore> _WasmExecutor<'a, S> {
    pub fn _deploy(
        &mut self,
        creator: [u8; 32],
        code: &[u8],
        height: u64,
        gas_limit: u64,
    ) -> Result<(ContractInstance, u64)> {
        let code_hash = self.store._put_code(code)?;
        let inst = self.store._create_instance(code_hash, creator, height)?;
        // Optionally run an init() if export exists
        let (mut store_ctx, instance) = self.engine.instantiate_with_fuel(code, gas_limit)?;
        if let Some(init) = instance.get_func(&mut store_ctx, "init") {
            let _ = init.call(&mut store_ctx, &[], &mut []);
        }
        // Fuel metering disabled, approximate gas_used as gas_limit (placeholder)
        let gas_out = _finalize_gas(gas_limit, 0);
        let mut updated = inst.clone();
        updated.last_gas_used = gas_out.gas_used;
        self.store._update_instance(&updated)?;
        Ok((updated, gas_out.gas_used))
    }

    pub fn _execute(
        &mut self,
        address: [u8; 32],
        method: &str,
        args: &[u8],
        gas_limit: u64,
    ) -> Result<(Vec<u8>, u64)> {
        let inst = self.store._get_instance(&address)?;
        let code = self.store._get_code(&inst.code_hash)?;
        let (mut store_ctx, instance) = self.engine.instantiate_with_fuel(&code, gas_limit)?; // Fresh instantiation (no persistent state yet)

        // Set input in context
        let mut ctx = self.engine.env().context();
        ctx.input = args.to_vec();
        self.engine.set_context(ctx);

        let func: Func = instance
            .get_func(&mut store_ctx, method)
            .ok_or_else(|| anyhow!("method not found"))?;

        // Support zero-arg -> i32/i64 return only for demo
        // For real contracts, we expect them to use read_input/write_output and return void (or status code)
        // We allow the function to return nothing, i32, or i64.
        let ty = func.ty(&store_ctx);
        let mut results = vec![wasmtime::Val::I32(0); ty.results().len()];

        func.call(&mut store_ctx, &[], &mut results)?;

        // Fuel disabled; approximate gas
        let gas_out = _finalize_gas(gas_limit, 0);
        let mut updated = inst.clone();
        updated.last_gas_used = gas_out.gas_used;
        self.store._update_instance(&updated)?;

        // Retrieve output from env
        let output = self.engine.env().take_output();

        // If output is empty but function returned a value, maybe use that?
        // For now, we prefer explicit write_output. If output is empty and result is i64, we could convert it,
        // but let's stick to the plan: explicit output buffer.

        Ok((output, gas_out.gas_used))
    }
}
