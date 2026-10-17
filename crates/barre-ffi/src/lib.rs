//! Yoga's C API (`vendor/yoga/yoga/Yoga.h` and the headers it includes),
//! implemented over barre.
//!
//! Every function and global declared by those headers is exported with its
//! exact C name and signature (`./crates/barre-ffi/check-symbols.sh` checks
//! that none is missing). `YGNodeRef` points at a [`barre::Node`] and
//! `YGConfigRef` at a [`barre::Config`]. Enums cross the boundary as their C
//! integer values and are converted here.
//!
//! Errors: Yoga's fatal assertions throw `std::logic_error`; so do ours (via
//! `shim/shim.cpp`), from `extern "C-unwind"` functions. Any other panic
//! (including `not_implemented!`) aborts the process: plain `extern "C"`
//! functions abort when a panic reaches them, and [`guard`] aborts on panics
//! inside the `C-unwind` ones.

#![allow(non_snake_case, non_upper_case_globals)]
// Every export dereferences the raw pointers it is given; that is the C API's
// contract (YGNodeRef/YGConfigRef must be valid), so the functions are not
// marked `unsafe` individually.
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::missing_safety_doc)]

mod config;
mod enums;
mod layout;
mod node;
mod style;
mod value;

use std::ffi::{CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};

use barre::debug::assert_fatal::FatalError;
use barre::{Config, Node};

pub use value::YGValue;

/// `YGNodeRef`.
pub type YGNodeRef = *mut Node;
/// `YGNodeConstRef`.
pub type YGNodeConstRef = *const Node;
/// `YGConfigRef`.
pub type YGConfigRef = *mut Config;
/// `YGConfigConstRef`.
pub type YGConfigConstRef = *const Config;

/// `YGLogger`. The `va_list` is opaque to Rust; only the shim calls it.
pub type YGLogger = unsafe extern "C-unwind" fn(
    config: YGConfigConstRef,
    node: YGNodeConstRef,
    level: c_int,
    format: *const c_char,
    args: *mut c_void,
) -> c_int;

unsafe extern "C-unwind" {
    fn barre_call_logger(
        logger: YGLogger,
        config: *const c_void,
        node: *const c_void,
        level: c_int,
        message: *const c_char,
    ) -> c_int;
    fn barre_throw_logic_error(message: *const c_char, length: usize) -> !;
}

/// Throws `std::logic_error(message)`, as Yoga's `fatalWithMessage` does.
fn throw_logic_error(error: FatalError) -> ! {
    // SAFETY: the shim copies the message into the exception before
    // throwing. The exception unwinds through the calling `extern
    // "C-unwind"` export into C++.
    unsafe { barre_throw_logic_error(error.message.as_ptr().cast(), error.message.len()) }
}

/// Runs the body of an `extern "C-unwind"` export: a [`FatalError`] becomes
/// a thrown `std::logic_error`, and a panic aborts (it must never unwind into
/// C).
fn guard<R>(f: impl FnOnce() -> Result<R, FatalError>) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => throw_logic_error(error),
        Err(_) => std::process::abort(),
    }
}

/// A `fatalWithMessage` with no preceding log (Yoga's `fatalWithMessage`
/// called directly).
fn fatal_without_log(message: &'static str) -> FatalError {
    FatalError { message }
}

/// The [`barre::debug::log::LogFn`] behind a C `YGLogger`: the logger is
/// stored in the [`barre::debug::log::Logger`]'s `data`.
fn c_logger_trampoline(
    data: usize,
    config: &Config,
    node: Option<&Node>,
    level: barre::LogLevel,
    message: &str,
) {
    // SAFETY: `data` was stored from a `YGLogger` by `YGConfigSetLogger`.
    let logger: YGLogger = unsafe { std::mem::transmute::<usize, YGLogger>(data) };
    let message = CString::new(message.replace('\0', "")).expect("NULs were removed");
    let node_ptr = node.map_or(std::ptr::null(), |n| n as *const Node);
    // SAFETY: the shim calls the embedder's logger with these arguments.
    unsafe {
        barre_call_logger(
            logger,
            (config as *const Config).cast(),
            node_ptr.cast(),
            level as c_int,
            message.as_ptr(),
        );
    }
}

/// Borrows a node. Null aborts.
#[inline]
fn node<'a>(node: YGNodeConstRef) -> &'a Node {
    // SAFETY: the C API's contract: a YGNodeRef is a live node.
    unsafe { node.as_ref() }.expect("null YGNodeRef")
}

/// The [`barre::NodePtr`] of a node. Null aborts.
#[inline]
fn node_ptr(node: YGNodeConstRef) -> barre::NodePtr {
    barre::NodePtr::from_raw(node.cast_mut()).expect("null YGNodeRef")
}

/// Borrows a config. Null aborts.
#[inline]
fn config<'a>(config: YGConfigConstRef) -> &'a Config {
    // SAFETY: the C API's contract: a YGConfigRef is a live config.
    unsafe { config.as_ref() }.expect("null YGConfigRef")
}

/// Mutably borrows a config. Null aborts. The borrow must not be live while
/// the config's logger can run.
#[inline]
fn config_mut<'a>(config: YGConfigRef) -> &'a mut Config {
    // SAFETY: the C API's contract: a YGConfigRef is a live config, and no
    // other reference to it is used during the call.
    unsafe { config.as_mut() }.expect("null YGConfigRef")
}

/// Converts a C enum value, aborting on out-of-range values (Yoga's
/// behaviour for those is undefined).
#[inline]
fn from_raw<T>(raw: u32, convert: fn(u32) -> Option<T>) -> T {
    convert(raw).unwrap_or_else(|| panic!("invalid enum value {raw}"))
}
