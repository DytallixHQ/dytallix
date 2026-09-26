/*
Simple Oracle Module

Basic oracle functionality without ink! dependencies
*/

use crate::types::{Address, Amount};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRequest {
    pub id: u64,
    pub requester: Address,
    pub data_type: String,
    pub parameters: HashMap<String, String>,
    pub callback_address: Option<Address>,
    pub fee: Amount,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleResponse {
    pub request_id: u64,
    pub data: Vec<u8>,
    pub confidence: u8, // 0-100
    pub timestamp: u64,
    pub provider: Address,
}

#[derive(Debug)]
pub struct SimpleOracle {
    requests: HashMap<u64, OracleRequest>,
    responses: HashMap<u64, OracleResponse>,
    next_request_id: u64,
    authorized_providers: HashMap<Address, bool>,
}

impl SimpleOracle {
    pub fn new() -> Self {
        Self {
            requests: HashMap::new(),
            responses: HashMap::new(),
            next_request_id: 1,
            authorized_providers: HashMap::new(),
        }
    }
}

impl Default for SimpleOracle {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleOracle {
    pub fn submit_request(&mut self, mut request: OracleRequest) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;

        request.id = request_id;
        self.requests.insert(request_id, request);

        request_id
    }

    pub fn submit_response(&mut self, response: OracleResponse) -> Result<(), String> {
        if !self.requests.contains_key(&response.request_id) {
            return Err("Request not found".to_string());
        }

        self.responses.insert(response.request_id, response);
        Ok(())
    }

    pub fn get_response(&self, request_id: u64) -> Option<&OracleResponse> {
        self.responses.get(&request_id)
    }

    pub fn authorize_provider(&mut self, provider: Address) {
        self.authorized_providers.insert(provider, true);
    }

    pub fn is_provider_authorized(&self, provider: &Address) -> bool {
        self.authorized_providers
            .get(provider)
            .copied()
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_basic_functionality() {
        let mut oracle = SimpleOracle::new();

        let request = OracleRequest {
            id: 0, // Will be set by submit_request
            requester: "dyt1user123".to_string(),
            data_type: "price".to_string(),
            parameters: HashMap::new(),
            callback_address: None,
            fee: 100,
            timestamp: 1234567890,
        };

        let request_id = oracle.submit_request(request);
        assert_eq!(request_id, 1);

        let response = OracleResponse {
            request_id,
            data: vec![1, 2, 3, 4],
            confidence: 95,
            timestamp: 1234567900,
            provider: "dyt1oracle456".to_string(),
        };

        let result = oracle.submit_response(response);
        assert!(result.is_ok());

        let retrieved = oracle.get_response(request_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().confidence, 95);
    }
}
