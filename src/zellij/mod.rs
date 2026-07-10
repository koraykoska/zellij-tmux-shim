//! The zellij bridge: JSON models and (later) the subprocess client that is the
//! only code permitted to spawn the real `zellij` binary.

pub mod client;
pub mod types;
