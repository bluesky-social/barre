//! The `YG*ToString` functions of `YGEnums.h`.

use std::ffi::c_char;

use barre::enums::*;

macro_rules! to_string {
    ($fn_name:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $fn_name(value: u32) -> *const c_char {
            <$ty>::raw_name(value).as_ptr()
        }
    };
}

to_string!(YGAlignToString, Align);
to_string!(YGBoxSizingToString, BoxSizing);
to_string!(YGDimensionToString, Dimension);
to_string!(YGDirectionToString, Direction);
to_string!(YGDisplayToString, Display);
to_string!(YGEdgeToString, Edge);
to_string!(YGErrataToString, Errata);
to_string!(YGExperimentalFeatureToString, ExperimentalFeature);
to_string!(YGFlexDirectionToString, FlexDirection);
to_string!(YGGridTrackTypeToString, GridTrackType);
to_string!(YGGutterToString, Gutter);
to_string!(YGJustifyToString, Justify);
to_string!(YGLogLevelToString, LogLevel);
to_string!(YGMeasureModeToString, MeasureMode);
to_string!(YGNodeTypeToString, NodeType);
to_string!(YGOverflowToString, Overflow);
to_string!(YGPositionTypeToString, PositionType);
to_string!(YGUnitToString, Unit);
to_string!(YGWrapToString, Wrap);
