// Feature unification must never add legacy routes or alternate crypto backends
// to the selected consensus application without a compile-time failure.

pub mod addr; // address derivation
pub mod crypto; // new crypto module
pub mod genesis;
pub mod emergency_freeze;
pub mod emergency_verifier;
pub mod upgrade;
pub mod runtime;
pub mod state;
pub mod storage;
pub mod util;

mod settlement;

mod block_lifecycle;

pub mod supply;
// Fixed classes for a stopped application (E04 gap 15).
pub mod failure_class;

pub mod consensus_settlement;

pub mod recovery_fees;
pub mod recovery_store;
pub(crate) mod state_tree;
pub mod snapshot;
pub mod app_metrics;

pub mod ordinary_authority;
pub mod ordinary_transport;
pub(crate) mod governance_actions;
pub(crate) mod governance_execution;
pub(crate) mod governance_v3_meter;
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
