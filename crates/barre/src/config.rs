//! Layout configuration (`yoga/config/Config.h`, `Config.cpp`).

use std::ffi::c_void;

use crate::debug::log::Logger;
use crate::enums::{Errata, ExperimentalFeature};
use crate::node::{Node, NodePtr};

/// `YGCloneNodeFunc`: called when layout needs a private copy of a shared
/// child. Returning null falls back to [`crate::node::tree::node_clone`].
pub type CloneNodeFunc =
    unsafe extern "C-unwind" fn(old_node: *const Node, owner: *const Node, child_index: usize) -> *mut Node;

/// `yoga::Config`.
///
/// Configs are owned by the embedder (the C API frees them with
/// `YGConfigFree`); nodes point at them and must not outlive them.
#[derive(Debug)]
pub struct Config {
    clone_node_callback: Option<CloneNodeFunc>,
    logger: Option<Logger>,
    use_web_defaults: bool,
    version: u32,
    experimental_features: u32,
    errata: Errata,
    point_scale_factor: f32,
    context: *mut c_void,
}

// SAFETY: only used for the immutable default config, whose context pointer
// is null and which is never mutated (the C API only hands out a const
// pointer to it).
struct SyncConfig(Config);
unsafe impl Sync for SyncConfig {}

static DEFAULT_CONFIG: SyncConfig = SyncConfig(Config::new());

/// `configUpdateInvalidatesLayout`: whether moving a node from `old` to `new`
/// should dirty previously calculated layout results.
pub fn config_update_invalidates_layout(old: &Config, new: &Config) -> bool {
    old.errata() != new.errata()
        || old.enabled_experiments() != new.enabled_experiments()
        || old.point_scale_factor() != new.point_scale_factor()
        || old.use_web_defaults() != new.use_web_defaults()
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

impl Config {
    /// A config with Yoga's defaults (`YGConfigNew`).
    pub const fn new() -> Config {
        Config {
            clone_node_callback: None,
            logger: None,
            use_web_defaults: false,
            version: 0,
            experimental_features: 0,
            errata: Errata(
                Errata::MIN_SIZE_UNDEFINED_INSTEAD_OF_AUTO.0
                    | Errata::FLEX_FIRST_PASS_USES_RUNNING_TOTALS.0,
            ),
            point_scale_factor: 1.0,
            context: std::ptr::null_mut(),
        }
    }

    /// `Config::getDefault`: the immutable config used by nodes created
    /// without one.
    pub fn get_default() -> &'static Config {
        &DEFAULT_CONFIG.0
    }

    pub fn set_use_web_defaults(&mut self, use_web_defaults: bool) {
        self.use_web_defaults = use_web_defaults;
    }
    #[inline]
    pub fn use_web_defaults(&self) -> bool {
        self.use_web_defaults
    }

    /// `Config::setExperimentalFeatureEnabled`.
    pub fn set_experimental_feature_enabled(&mut self, feature: ExperimentalFeature, enabled: bool) {
        if self.is_experimental_feature_enabled(feature) != enabled {
            let bit = 1u32 << (feature as u32);
            if enabled {
                self.experimental_features |= bit;
            } else {
                self.experimental_features &= !bit;
            }
            self.version = self.version.wrapping_add(1);
        }
    }
    #[inline]
    pub fn is_experimental_feature_enabled(&self, feature: ExperimentalFeature) -> bool {
        self.experimental_features & (1u32 << (feature as u32)) != 0
    }
    /// `Config::getEnabledExperiments`, as a bit set indexed by feature.
    #[inline]
    pub fn enabled_experiments(&self) -> u32 {
        self.experimental_features
    }

    /// `Config::setErrata`.
    pub fn set_errata(&mut self, errata: Errata) {
        if self.errata != errata {
            self.errata = errata;
            self.version = self.version.wrapping_add(1);
        }
    }
    /// `Config::addErrata`.
    pub fn add_errata(&mut self, errata: Errata) {
        if !self.has_errata(errata) {
            self.errata = self.errata | errata;
            self.version = self.version.wrapping_add(1);
        }
    }
    /// `Config::removeErrata`.
    pub fn remove_errata(&mut self, errata: Errata) {
        if self.has_errata(errata) {
            self.errata = Errata(self.errata.0 & !errata.0);
            self.version = self.version.wrapping_add(1);
        }
    }
    #[inline]
    pub fn errata(&self) -> Errata {
        self.errata
    }
    #[inline]
    pub fn has_errata(&self, errata: Errata) -> bool {
        self.errata.intersects(errata)
    }

    /// `Config::setPointScaleFactor`. The C API rejects negative values
    /// before calling this.
    pub fn set_point_scale_factor(&mut self, point_scale_factor: f32) {
        if self.point_scale_factor != point_scale_factor {
            self.point_scale_factor = point_scale_factor;
            self.version = self.version.wrapping_add(1);
        }
    }
    #[inline]
    pub fn point_scale_factor(&self) -> f32 {
        self.point_scale_factor
    }

    pub fn set_context(&mut self, context: *mut c_void) {
        self.context = context;
    }
    #[inline]
    pub fn context(&self) -> *mut c_void {
        self.context
    }

    /// `Config::getVersion`: bumped whenever a setting that affects layout
    /// changes, so cached layouts computed under an older version are
    /// redone.
    #[inline]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Sets the logger; `None` restores the default (stdout/stderr).
    pub fn set_logger(&mut self, logger: Option<Logger>) {
        self.logger = logger;
    }
    #[inline]
    pub fn logger(&self) -> Option<&Logger> {
        self.logger.as_ref()
    }

    pub fn set_clone_node_callback(&mut self, clone_node: Option<CloneNodeFunc>) {
        self.clone_node_callback = clone_node;
    }

    /// `Config::cloneNode`: clones `node` for `owner` via the callback, or
    /// with [`crate::node::tree::node_clone`] if there is none or it returns
    /// null.
    pub fn clone_node(&self, node: &Node, owner: &Node, child_index: usize) -> NodePtr {
        if let Some(callback) = self.clone_node_callback {
            // SAFETY: the callback is the embedder's `YGCloneNodeFunc`, which
            // receives the nodes as `YGNodeConstRef`s and returns a node it
            // owns (or null).
            let clone = unsafe { callback(node, owner, child_index) };
            if let Some(clone) = NodePtr::from_raw(clone) {
                return clone;
            }
        }
        crate::node::tree::node_clone(node)
    }
}
