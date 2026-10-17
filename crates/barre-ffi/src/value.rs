//! `YGValue.h`, `YGPixelGrid.h`.

use barre::Unit;
use barre::style::StyleLength;

/// `YGValue`.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct YGValue {
    pub value: f32,
    /// `YGUnit`.
    pub unit: u32,
}

impl From<StyleLength> for YGValue {
    /// `StyleLength::operator YGValue`.
    fn from(length: StyleLength) -> YGValue {
        YGValue {
            value: length.value(),
            unit: length.unit() as u32,
        }
    }
}

#[unsafe(no_mangle)]
pub static YGValueZero: YGValue = YGValue {
    value: 0.0,
    unit: Unit::Point as u32,
};

#[unsafe(no_mangle)]
pub static YGValueUndefined: YGValue = YGValue {
    value: f32::NAN,
    unit: Unit::Undefined as u32,
};

#[unsafe(no_mangle)]
pub static YGValueAuto: YGValue = YGValue {
    value: f32::NAN,
    unit: Unit::Auto as u32,
};

#[unsafe(no_mangle)]
pub extern "C" fn YGFloatIsUndefined(value: f32) -> bool {
    value.is_nan()
}

#[unsafe(no_mangle)]
pub extern "C" fn YGRoundValueToPixelGrid(
    value: f64,
    point_scale_factor: f64,
    force_ceil: bool,
    force_floor: bool,
) -> f32 {
    barre::algorithm::pixel_grid::round_value_to_pixel_grid(
        value,
        point_scale_factor,
        force_ceil,
        force_floor,
    )
}
