//! Dytallix Blockchain Node Main
#![allow(async_fn_in_trait)]

use std::sync::Arc;
use log::{info, warn};

mod consensus;
mod crypto;
mod networking;
mod runtime;
mod storage;
mod types; // Add types module
mod api; // Add API module

use crate::consensus::ConsensusEngine;
use crate::crypto::PQCManager;
use crate::networking::NetworkManager;
use crate::runtime::DytallixRuntime;
use crate::storage::StorageManager;
use crate::types::{TransactionPool, Transaction, Block, NodeStatus}; // Import core types

pub trait DytallixNode {
    async fn start(&self) -> Result<(), String>;
    async fn stop(&self) -> Result<(), String>;
    async fn submit_transaction(&self, tx: Transaction) -> Result<(), String>;
    fn get_block(&self, hash: &str) -> Option<Block>;
    fn get_status(&self) -> NodeStatus;
}

pub struct DummyNode {
    runtime: Arc<DytallixRuntime>,
    consensus: Arc<ConsensusEngine>,
    network: Arc<NetworkManager>,
    storage: Arc<StorageManager>,
    pqc_manager: Arc<PQCManager>,
    transaction_pool: Arc<TransactionPool>, // Add transaction pool
}

impl DytallixNode for DummyNode {
    async fn start(&self) -> Result<(), String> {
        info!("Starting Dytallix Node...");

        // Start network layer
        // self.network.start().await?;

        // Start consensus engine
        // self.consensus.start().await?;

        info!("Dytallix Node started successfully");
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        info!("Stopping Dytallix Node...");

        // self.consensus.stop().await?;
        // self.network.stop().await?;

        info!("Dytallix Node stopped");
        Ok(())
    }

    async fn submit_transaction(&self, tx: Transaction) -> Result<(), String> {
        // Add transaction to pool
        let tx_hash = self.transaction_pool.add_transaction(tx).await
            .map_err(|e| format!("Failed to add transaction: {e}"))?;

        info!("Transaction {} submitted successfully", tx_hash);
        Ok(())
    }
            from,
            to,
            amount,
            nonce,
            fee: 1, // Default fee
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            signature: crate::types::PQCTransactionSignature {
                signature: dytallix_pqc::Signature {
                    data: Vec::new(),
                    algorithm: dytallix_pqc::SignatureAlgorithm::Dilithium5,


                },
                public_key: Vec::new(),
            },
            ai_risk_score: Some(0.2),
        };

    fn get_block(&self, hash: &str) -> Option<Block> {
        // Implement block retrieval from storage
        match self.storage.get_block_by_hash(hash) {
            Ok(Some(block)) => Some(block),
            Ok(None) => {
                log::warn!("Block not found: {}", hash);
                None
            }
            Err(e) => {
                log::error!("Error retrieving block {}: {}", hash, e);
                None
            }
        }
    }

    fn get_status(&self) -> NodeStatus {
        NodeStatus::Running
    }

    async fn get_balance(&self, address: &str) -> Result<u64, String> {
        // Implement balance retrieval from storage
        match self.storage.get_address_balance(address).await {
            Ok(balance) => {
                log::info!("Retrieved balance for address {}: {}", address, balance);
                Ok(balance)
            }
            Err(e) => {
                log::error!("Error retrieving balance for address {}: {}", address, e);
                Err(format!("Failed to retrieve balance: {e}"))
            }
        }
    }
}
        // TODO: Lookup block
        None
    }
    fn get_status(&self) -> NodeStatus {
        // TODO: Return node status
        NodeStatus::Running
    }
}

impl DummyNode {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        info!("Initializing Dytallix Node...");

        // Initialize PQC manager first (load or generate keys)
        let pqc_manager = Arc::new(PQCManager::load_or_generate("pqc_keys.json")?);
        info!("Post-quantum cryptography initialized");

        // Initialize storage
        let storage = Arc::new(StorageManager::new().await?);
        info!("Storage layer initialized");

        // Initialize runtime
        let runtime = Arc::new(DytallixRuntime::new(Arc::clone(&storage))?);
        info!("Runtime initialized");

        // Initialize consensus engine
        let consensus = Arc::new(ConsensusEngine::new(
            Arc::clone(&runtime),
            Arc::clone(&pqc_manager),
        )?);
        info!("Consensus engine initialized");

        // Initialize network manager
        let network = Arc::new(NetworkManager::new(
            Arc::clone(&consensus),
            Arc::clone(&pqc_manager),
        ).await?);
        info!("Network manager initialized");

        // Initialize transaction pool
        let transaction_pool = Arc::new(TransactionPool::new(10000));
        info!("Transaction pool initialized (max 10000 transactions)");

        Ok(Self {
            runtime,
            consensus,
            network,
            storage,
            pqc_manager,
            transaction_pool,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("Welcome to Dytallix - Post-Quantum AI-Enhanced Cryptocurrency");

    let node = DummyNode::new().await?;

    // Handle graceful shutdown
    let node_clone = Arc::new(node);
    let shutdown_node = Arc::clone(&node_clone);

    // Start API server in background
    let api_node = Arc::clone(&node_clone);
    tokio::spawn(async move {
        if let Err(e) = crate::api::start_api_server(api_node).await {
            log::error!("API server error: {}", e);
        }
    });

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        warn!("Received shutdown signal");
        if let Err(e) = shutdown_node.stop().await {
            log::error!("Error during shutdown: {}", e);
        }
        std::process::exit(0);
    });

    node_clone.start().await?;

    // Keep the main thread alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
