//! `yoga::SizingMode` (`yoga/algorithm/SizingMode.h`).

use crate::enums::MeasureMode;

/// `yoga::SizingMode`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SizingMode {
    /// The size a box would take if its outer size filled the available
    /// space in the given axis; in other words, the stretch fit into the
    /// available space, if that is definite. Undefined if the available
    /// space is indefinite.
    StretchFit,
    /// A box's "ideal" size in a given axis when given infinite available
    /// space. Usually this is the smallest size the box could take in that
    /// axis while still fitting around its contents, i.e. minimizing unfilled
    /// space while avoiding overflow.
    #[default]
    MaxContent,
    /// If the available space in a given axis is definite, equal to
    /// clamp(min-content size, stretch-fit size, max-content size) (i.e.
    /// max(min-content size, min(max-content size, stretch-fit size))). When
    /// sizing under a min-content constraint, equal to the min-content size.
    /// Otherwise, equal to the max-content size in that axis.
    FitContent,
}

/// `measureMode`.
#[inline]
pub fn measure_mode(mode: SizingMode) -> MeasureMode {
    match mode {
        SizingMode::StretchFit => MeasureMode::Exactly,
        SizingMode::MaxContent => MeasureMode::Undefined,
        SizingMode::FitContent => MeasureMode::AtMost,
    }
}

/// `sizingMode`.
#[inline]
pub fn sizing_mode(mode: MeasureMode) -> SizingMode {
    match mode {
        MeasureMode::Exactly => SizingMode::StretchFit,
        MeasureMode::Undefined => SizingMode::MaxContent,
        MeasureMode::AtMost => SizingMode::FitContent,
    }
}
