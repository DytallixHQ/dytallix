//! Prototype interfaces for a future Dytallix contract test harness.
//!
//! This directory is not wired into the main crate CI surface and does not
//! represent a supported public execution harness yet. The types here exist as
//! placeholders for future harness integration work.

pub struct ContractAddress(pub String);
pub struct ContractResult;
pub struct ContractState;
pub struct AuditReport;
pub struct CompileError;
pub struct DeployError;
pub struct ContractError;

pub trait WasmCompiler {
    fn compile_source(&self, source: &str) -> Result<Vec<u8>, CompileError>;
}

pub trait ContractDeployer {
    fn deploy(&self, wasm: &[u8]) -> Result<ContractAddress, DeployError>;
}

pub trait ContractTestRunner {
    fn deploy_contract(&self, wasm: &[u8]) -> ContractAddress;
    fn call_method(&self, address: &ContractAddress, method: &str, args: &[u8]) -> Result<ContractResult, ContractError>;
    fn get_state(&self, address: &ContractAddress) -> ContractState;
    fn audit_with_ai(&self, address: &ContractAddress) -> AuditReport;
}

pub struct DummyTestRunner;

impl WasmCompiler for DummyTestRunner {
    fn compile_source(&self, _source: &str) -> Result<Vec<u8>, CompileError> {
        // Placeholder implementation for interface sketching only.
        Ok(vec![])
    }
}

impl ContractDeployer for DummyTestRunner {
    fn deploy(&self, _wasm: &[u8]) -> Result<ContractAddress, DeployError> {
        // Placeholder implementation for interface sketching only.
        Ok(ContractAddress("dummy_address".to_string()))
    }
}

impl ContractTestRunner for DummyTestRunner {
    fn deploy_contract(&self, wasm: &[u8]) -> ContractAddress {
        let _ = wasm;
        ContractAddress("dummy_address".to_string())
    }
    fn call_method(&self, _address: &ContractAddress, _method: &str, _args: &[u8]) -> Result<ContractResult, ContractError> {
        // Placeholder implementation for interface sketching only.
        Ok(ContractResult)
    }
    fn get_state(&self, _address: &ContractAddress) -> ContractState {
        // Placeholder implementation for interface sketching only.
        ContractState
    }
    fn audit_with_ai(&self, _address: &ContractAddress) -> AuditReport {
        // Placeholder implementation for interface sketching only.
        AuditReport
    }
}
