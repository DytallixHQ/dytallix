#![allow(dead_code)]
#[cfg(test)]
#[path = "../tests/support/temp_directory.rs"]
mod test_support;

use log::{error, info, warn};
use std::path::Path;
use std::process;
use std::sync::Arc;

mod api;
mod crypto;
mod runtime;
mod storage;
mod types;
// Added modules required by runtime/api when compiled as bin (lib already exports these)
mod contracts;
mod genesis;
mod staking;
mod wasm; // PulseGuard WASM engine & host env // smart contract integration types
          // mod consensus;  // Temporarily disabled
          // mod networking;  // Temporarily disabled

// use crate::runtime::DytallixRuntime;  // Temporarily disabled
use crate::crypto::PQCManager;
use crate::types::{Block, NodeStatus, Transaction, TransactionPool};

pub struct DummyNode {
    // runtime: Arc<Result<DytallixRuntime, Box<dyn std::error::Error>>>,  // Temporarily disabled
    _pqc_manager: Arc<PQCManager>,
    transaction_pool: Arc<TransactionPool>,
}

impl DummyNode {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let pqc_manager = Arc::new(PQCManager::load_or_generate("pqc_keys.json")?);
        let transaction_pool = Arc::new(TransactionPool::new(10000));

        Ok(Self {
            // runtime,  // Temporarily disabled
            _pqc_manager: pqc_manager,
            transaction_pool,
        })
    }

    pub async fn start(&self) -> Result<(), String> {
        info!("Starting Dytallix Node...");
        info!("Dytallix Node started successfully");
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        info!("Stopping Dytallix Node...");
        info!("Dytallix Node stopped");
        Ok(())
    }

    pub async fn submit_transaction(&self, tx: Transaction) -> Result<(), String> {
        let tx_hash = self
            .transaction_pool
            .add_transaction(tx)
            .await
            .map_err(|e| format!("Failed to add transaction: {e}"))?;

        info!("Transaction {tx_hash} submitted successfully");
        Ok(())
    }

    pub fn get_block(&self, _hash: &str) -> Option<Block> {
        None
    }

    pub fn get_status(&self) -> NodeStatus {
        NodeStatus::Running
    }

    pub async fn get_balance(&self, _address: &str) -> Result<u64, String> {
        Ok(0)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging as early as possible
    env_logger::init();
    info!("Starting Dytallix Node - Debug Mode");

    // Check for required files
    info!("Checking for required configuration files...");
    if !Path::new("pqc_keys.json").exists() {
        warn!("pqc_keys.json not found - will generate new keys");
    } else {
        info!("Found pqc_keys.json");
    }

    // Initialize node
    info!("Initializing DummyNode...");
    let node = match DummyNode::new() {
        Ok(n) => {
            info!("DummyNode initialized successfully");
            Arc::new(n)
        }
        Err(e) => {
            error!("Failed to initialize DummyNode: {e}");
            process::exit(1);
        }
    };

    let node_clone = Arc::clone(&node);

    // Start node
    info!("Starting node services...");
    if let Err(e) = node_clone.start().await {
        error!("Failed to start node: {e}");
        process::exit(1);
    }
    info!("Node services started successfully");

    info!("Dytallix blockchain core is running!");

    // Start API server
    let port = std::env::var("PORT").unwrap_or_else(|_| "3003".to_string());
    info!("Starting API server on port {}...", port);
    tokio::spawn(async move {
        if let Err(e) = api::start_api_server().await {
            error!("API server failed to start: {e}");
            process::exit(1);
        }
    });

    info!("API server spawn initiated - waiting for startup...");

    // Give API server time to start
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    info!("Main loop starting - node should be fully operational");

    // Keep the main thread alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        info!("Node heartbeat - still running");
    }
}
