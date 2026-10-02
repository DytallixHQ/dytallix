//! Native service ownership: the disposable development mode, or the
//! production mode of a production build. Running grants no launch
//! authorization; the root-signed genesis does.
pub mod config;
pub mod lease;
pub mod processes;
pub mod readiness;
pub mod service;
pub mod startup_diagnostic;
