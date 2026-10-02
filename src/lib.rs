//! The implementation of the `lopi` command-line program.
//!
//! This library exists for the `lopi` binary and its tests. It has no stable API: anything
//! in it may change in any release, so do not depend on it from other crates.

pub mod app;
pub mod askpass;
pub mod atomic;
pub mod backup_bundle;
pub mod cli;
pub mod commands;
pub mod completion;
pub mod config;
pub mod error;
pub mod install;
pub mod lock;
pub mod output;
pub mod resolve;
pub mod secrets;
pub mod ssh;
pub mod state;
pub mod time;
pub mod ui;
