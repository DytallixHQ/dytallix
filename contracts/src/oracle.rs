use ink_env::{Environment, DefaultEnvironment};
use ink_prelude::vec::Vec;
use scale::{Decode, Encode};
use crate::{ContractError, AIAnalysisResult, Result};

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct OracleRequest {
    pub request_id: u64,
    pub request_type: OracleRequestType,
    pub data: Vec<u8>,
    pub callback_contract: ink_env::AccountId,
    pub callback_method: [u8; 4], // Method selector
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum OracleRequestType {
    FraudAnalysis,
    RiskScoring,
    ContractAudit,
    PriceData,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct OracleResponse {
    pub request_id: u64,
    pub result: Vec<u8>,
    pub timestamp: u64,
    pub confidence: u8,
}

pub trait AIOracle {
    /// Submit a request for AI analysis
    fn request_analysis(
        &mut self,
        request_type: OracleRequestType,
        data: Vec<u8>,
    ) -> Result<u64>;

    /// Get the result of a previous analysis request
    fn get_analysis_result(&self, request_id: u64) -> Result<Option<AIAnalysisResult>>;

    /// Check if oracle result is available
    fn is_result_ready(&self, request_id: u64) -> bool;
}

#[ink::contract]
pub mod ai_oracle_contract {
    use super::*;
    use ink_storage::{Mapping, traits::{PackedLayout, SpreadLayout}};
    use ink_prelude::collections::BTreeMap;

    #[ink(storage)]
    pub struct AIOracleContract {
        /// Oracle operator address
        operator: AccountId,
        /// Pending requests
        pending_requests: Mapping<u64, OracleRequest>,
        /// Completed responses
        responses: Mapping<u64, OracleResponse>,
        /// Request counter
        next_request_id: u64,
        /// Authorized AI service addresses
        authorized_services: Mapping<AccountId, bool>,
    }

    #[ink(event)]
    pub struct AnalysisRequested {
        #[ink(topic)]
        request_id: u64,
        #[ink(topic)]
        requester: AccountId,
        request_type: OracleRequestType,
    }

    #[ink(event)]
    pub struct AnalysisCompleted {
        #[ink(topic)]
        request_id: u64,
        confidence: u8,
    }

    impl AIOracleContract {
        #[ink(constructor)]
        pub fn new() -> Self {
            let mut authorized_services = Mapping::default();

            // In a real deployment, this would be configured properly
            // For now, allow the deployer as an authorized service
            let deployer = Self::env().caller();
            authorized_services.insert(&deployer, &true);

            Self {
                operator: deployer,
                pending_requests: Mapping::default(),
                responses: Mapping::default(),
                next_request_id: 1,
                authorized_services,
            }
        }

        #[ink(message)]
        pub fn request_fraud_analysis(&mut self, transaction_data: Vec<u8>) -> Result<u64> {
            self.request_analysis(OracleRequestType::FraudAnalysis, transaction_data)
        }

        #[ink(message)]
        pub fn request_risk_scoring(&mut self, address_data: Vec<u8>) -> Result<u64> {
            self.request_analysis(OracleRequestType::RiskScoring, address_data)
        }

        #[ink(message)]
        pub fn submit_analysis_result(
            &mut self,
            request_id: u64,
            result: Vec<u8>,
            confidence: u8,
        ) -> Result<()> {
            let caller = self.env().caller();

            // Verify caller is authorized AI service
            if !self.authorized_services.get(&caller).unwrap_or(false) {
                return Err(ContractError::NotAuthorized);
            }

            // Verify request exists
            if !self.pending_requests.contains(&request_id) {
                return Err(ContractError::InvalidState);
            }

            // Store response
            let response = OracleResponse {
                request_id,
                result,
                timestamp: self.env().block_timestamp(),
                confidence,
            };

            self.responses.insert(&request_id, &response);
            self.pending_requests.remove(&request_id);

            self.env().emit_event(AnalysisCompleted {
                request_id,
                confidence,
            });

            Ok(())
        }

        #[ink(message)]
        pub fn authorize_service(&mut self, service: AccountId) -> Result<()> {
            let caller = self.env().caller();
            if caller != self.operator {
                return Err(ContractError::NotAuthorized);
            }

            self.authorized_services.insert(&service, &true);
            Ok(())
        }

        #[ink(message)]
        pub fn revoke_service(&mut self, service: AccountId) -> Result<()> {
            let caller = self.env().caller();
            if caller != self.operator {
                return Err(ContractError::NotAuthorized);
            }

            self.authorized_services.remove(&service);
            Ok(())
        }

        #[ink(message)]
        pub fn get_response(&self, request_id: u64) -> Option<OracleResponse> {
            self.responses.get(&request_id)
        }

        #[ink(message)]
        pub fn is_service_authorized(&self, service: AccountId) -> bool {
            self.authorized_services.get(&service).unwrap_or(false)
        }
    }

    impl AIOracle for AIOracleContract {
        #[ink(message)]
        fn request_analysis(
            &mut self,
            request_type: OracleRequestType,
            data: Vec<u8>,
        ) -> Result<u64> {
            let caller = self.env().caller();
            let request_id = self.next_request_id;
            self.next_request_id += 1;

            let request = OracleRequest {
                request_id,
                request_type: request_type.clone(),
                data,
                callback_contract: caller,
                callback_method: [0, 0, 0, 0], // Would be actual method selector
            };

            self.pending_requests.insert(&request_id, &request);

            self.env().emit_event(AnalysisRequested {
                request_id,
                requester: caller,
                request_type,
            });

            Ok(request_id)
        }

        #[ink(message)]
        fn get_analysis_result(&self, request_id: u64) -> Result<Option<AIAnalysisResult>> {
            if let Some(response) = self.responses.get(&request_id) {
                // Decode the result bytes into AIAnalysisResult
                match AIAnalysisResult::decode(&mut &response.result[..]) {
                    Ok(result) => Ok(Some(result)),
                    Err(_) => Err(ContractError::OracleError),
                }
            } else {
                Ok(None)
            }
        }

        #[ink(message)]
        fn is_result_ready(&self, request_id: u64) -> bool {
            self.responses.contains(&request_id)
        }
    }
}
