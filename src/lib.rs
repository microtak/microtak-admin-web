//! microtak-admin-web core library -- see `src/main.rs` for the actual
//! binary entry point and `README.md` for configuration/usage. Split into
//! a lib + thin bin (same pattern `microtak-server` itself uses) purely so
//! `tests/e2e.rs` can drive the real router against a real, running
//! `microtak_server::app::App`.

pub mod auth;
pub mod client;
pub mod config;
pub mod pages;
pub mod qr;
