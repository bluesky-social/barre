//! Logging (`yoga/debug/Log.cpp`).

use crate::config::Config;
use crate::enums::LogLevel;
use crate::node::Node;

/// The function behind a [`Logger`]. `data` is the logger's opaque payload
/// (the C ABI stores the embedder's `YGLogger` there).
pub type LogFn =
    fn(data: usize, config: &Config, node: Option<&Node>, level: LogLevel, message: &str);

/// A config's logger (`YGConfigSetLogger`).
#[derive(Copy, Clone, Debug)]
pub struct Logger {
    pub func: LogFn,
    pub data: usize,
}

/// `getDefaultLogger`: errors and fatals go to stderr, everything else to
/// stdout.
fn default_log(level: LogLevel, message: &str) {
    match level {
        LogLevel::Error | LogLevel::Fatal => eprint!("{message}"),
        _ => print!("{message}"),
    }
}

/// `yoga::log(const Config*, ...)`: logs through `config`'s logger, or the
/// default logger.
pub fn log_with_config(config: &Config, node: Option<&Node>, level: LogLevel, message: &str) {
    match config.logger() {
        Some(logger) => (logger.func)(logger.data, config, node, level, message),
        None => default_log(level, message),
    }
}

/// `yoga::log(const Node*, ...)`: logs through the node's config.
pub fn log(node: &Node, level: LogLevel, message: &str) {
    log_with_config(node.config(), Some(node), level, message);
}

/// `yoga::log(LogLevel, ...)`: no config; uses the default logger.
pub fn log_default(level: LogLevel, message: &str) {
    default_log(level, message);
}
