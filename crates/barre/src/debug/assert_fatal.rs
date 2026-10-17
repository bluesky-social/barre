//! Fatal assertions (`yoga/debug/AssertFatal.cpp`).
//!
//! Yoga logs at `Fatal` and then throws `std::logic_error` (or terminates
//! without exceptions). barre logs the same way and returns a
//! [`FatalError`]; the C ABI turns that into the same `std::logic_error`.
//! Assertions inside the layout algorithm (internal invariants, not API
//! misuse) panic instead.

use crate::config::Config;
use crate::debug::log::{log, log_default, log_with_config};
use crate::enums::LogLevel;
use crate::node::Node;

/// A failed fatal assertion. `message` is the text Yoga's `logic_error`
/// carries.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FatalError {
    pub message: &'static str,
}

/// `assertFatal`.
pub fn assert_fatal(condition: bool, message: &'static str) -> Result<(), FatalError> {
    if condition {
        return Ok(());
    }
    log_default(LogLevel::Fatal, &format!("{message}\n"));
    Err(FatalError { message })
}

/// `assertFatalWithNode`.
pub fn assert_fatal_with_node(
    node: &Node,
    condition: bool,
    message: &'static str,
) -> Result<(), FatalError> {
    if condition {
        return Ok(());
    }
    log(node, LogLevel::Fatal, &format!("{message}\n"));
    Err(FatalError { message })
}

/// `assertFatalWithConfig`.
pub fn assert_fatal_with_config(
    config: &Config,
    condition: bool,
    message: &'static str,
) -> Result<(), FatalError> {
    if condition {
        return Ok(());
    }
    log_with_config(config, None, LogLevel::Fatal, &format!("{message}\n"));
    Err(FatalError { message })
}
