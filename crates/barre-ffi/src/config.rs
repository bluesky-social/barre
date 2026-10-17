//! `YGConfig.h` (`yoga/YGConfig.cpp`).

use std::ffi::c_void;

use barre::config::CloneNodeFunc;
use barre::debug::assert_fatal::assert_fatal_with_config;
use barre::debug::log::Logger;
use barre::{Config, Errata, ExperimentalFeature};

use crate::{
    YGConfigConstRef, YGConfigRef, YGLogger, c_logger_trampoline, config, config_mut, from_raw,
    guard,
};

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigNew() -> YGConfigRef {
    Box::into_raw(Box::new(Config::new()))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigFree(config: YGConfigRef) {
    if !config.is_null() {
        // SAFETY: allocated by YGConfigNew (C API contract).
        drop(unsafe { Box::from_raw(config) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigGetDefault() -> YGConfigConstRef {
    Config::get_default()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetUseWebDefaults(config: YGConfigRef, enabled: bool) {
    config_mut(config).set_use_web_defaults(enabled);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigGetUseWebDefaults(c: YGConfigConstRef) -> bool {
    config(c).use_web_defaults()
}

#[unsafe(no_mangle)]
pub extern "C-unwind" fn YGConfigSetPointScaleFactor(c: YGConfigRef, pixels_in_point: f32) {
    guard(|| {
        assert_fatal_with_config(
            config(c),
            pixels_in_point >= 0.0,
            "Scale factor should not be less than zero",
        )?;
        config_mut(c).set_point_scale_factor(pixels_in_point);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigGetPointScaleFactor(c: YGConfigConstRef) -> f32 {
    config(c).point_scale_factor()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetErrata(c: YGConfigRef, errata: u32) {
    config_mut(c).set_errata(Errata(errata));
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigGetErrata(c: YGConfigConstRef) -> u32 {
    config(c).errata().0
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetLogger(c: YGConfigRef, logger: Option<YGLogger>) {
    config_mut(c).set_logger(logger.map(|logger| Logger {
        func: c_logger_trampoline,
        data: logger as usize,
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetContext(c: YGConfigRef, context: *mut c_void) {
    config_mut(c).set_context(context);
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigGetContext(c: YGConfigConstRef) -> *mut c_void {
    config(c).context()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetExperimentalFeatureEnabled(
    c: YGConfigRef,
    feature: u32,
    enabled: bool,
) {
    config_mut(c).set_experimental_feature_enabled(
        from_raw(feature, ExperimentalFeature::from_raw),
        enabled,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigIsExperimentalFeatureEnabled(c: YGConfigConstRef, feature: u32) -> bool {
    config(c).is_experimental_feature_enabled(from_raw(feature, ExperimentalFeature::from_raw))
}

#[unsafe(no_mangle)]
pub extern "C" fn YGConfigSetCloneNodeFunc(c: YGConfigRef, callback: Option<CloneNodeFunc>) {
    config_mut(c).set_clone_node_callback(callback);
}
