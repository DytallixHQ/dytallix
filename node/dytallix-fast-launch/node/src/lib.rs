// Feature unification must never add legacy routes or alternate crypto backends
// to the selected consensus application without a compile-time failure.

pub mod addr; // address derivation
pub mod alerts; // alerting subsystem
pub mod crypto; // new crypto module
pub mod execution; // deterministic execution engine
pub mod gas; // gas accounting system
pub mod genesis;
pub mod emergency_freeze;
pub mod emergency_verifier;
pub mod upgrade;
pub mod mempool;
pub mod metrics; // observability module (internally feature-gated)
pub mod runtime;
// Expose governance module unconditionally; runtime flags gate behavior
pub use runtime::governance;
// Expose staking module unconditionally; runtime flags gate behavior
pub use runtime::staking;
pub mod state;
pub mod storage;
pub mod types; // canonical transaction types
pub mod util;
 // added util module // p2p networking and gossip
            // re-export emission types
pub use runtime::emission::*;
 // vault + sealed keystore providers

mod settlement;

pub mod production_control;

mod block_lifecycle;
pub mod block_settlement;

mod signed_transaction;
pub mod supply;
pub mod transaction_cost;

pub mod consensus_settlement;

pub mod recovery_fees;

pub mod ordinary_authority;
pub mod ordinary_transport;
pub mod governance_signed_admission;
pub(crate) mod governance_v3_meter;
pub(crate) mod governance_v3_fee_settlement;
pub(crate) mod governance_v3_reservations;

pub mod ordinary_fee_settlement;
pub mod ordinary_meter;
pub mod ordinary_reservations;

pub mod ordinary_state;

pub mod ordinary_logical;

pub mod ordinary_validator;

pub mod ordinary_execution;

pub(crate) mod ordinary_admission;

pub mod root_genesis;

pub mod runtime_candidate;
pub mod runtime_candidate_v2;

pub mod release_handover;
