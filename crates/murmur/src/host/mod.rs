//! Agent Host module.
//!
//! Provides functionality for the `murmur-host` binary which wraps agent
//! subprocesses and communicates with the daemon via Unix socket.

pub mod manager;
pub mod server;

pub use manager::Manager;
pub use server::Server;
