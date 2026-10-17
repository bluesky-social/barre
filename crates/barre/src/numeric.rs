//! Float helpers (`yoga/numeric/Comparison.h`, `yoga/numeric/FloatOptional.h`).
//!
//! Yoga's `FloatOptional` is an `f32` where NaN means "undefined"; barre uses
//! plain `f32` with the same convention. Its operators map directly: `+`
//! propagates NaN, `<`/`>` are false when either side is NaN. The one
//! exception is `==`, where two undefined values compare equal; use
//! [`optional_eq`] for that.

/// `YGUndefined`.
pub const UNDEFINED: f32 = f32::NAN;

/// `yoga::isUndefined`.
#[inline(always)]
pub fn is_undefined(value: f32) -> bool {
    value.is_nan()
}

/// `yoga::isDefined`.
#[inline(always)]
pub fn is_defined(value: f32) -> bool {
    !value.is_nan()
}

/// `yoga::maxOrDefined`: the max if both are defined, otherwise whichever is
/// defined (or undefined).
#[inline]
pub fn max_or_defined(a: f32, b: f32) -> f32 {
    if is_defined(a) && is_defined(b) {
        // std::max(a, b) == (a < b) ? b : a
        if a < b { b } else { a }
    } else if is_undefined(a) {
        b
    } else {
        a
    }
}

/// `yoga::minOrDefined`.
#[inline]
pub fn min_or_defined(a: f32, b: f32) -> f32 {
    if is_defined(a) && is_defined(b) {
        // std::min(a, b) == (b < a) ? b : a
        if b < a { b } else { a }
    } else if is_undefined(a) {
        b
    } else {
        a
    }
}

/// `yoga::inexactEquals(float, float)`: equal within 0.0001, or both
/// undefined.
#[inline]
pub fn inexact_equals(a: f32, b: f32) -> bool {
    if is_defined(a) && is_defined(b) {
        (a - b).abs() < 0.0001f32
    } else {
        is_undefined(a) && is_undefined(b)
    }
}

/// `yoga::inexactEquals(double, double)`.
#[inline]
pub fn inexact_equals_f64(a: f64, b: f64) -> bool {
    if !a.is_nan() && !b.is_nan() {
        (a - b).abs() < 0.0001f64
    } else {
        a.is_nan() && b.is_nan()
    }
}

/// `FloatOptional::operator==`: equal, or both undefined.
#[inline]
pub fn optional_eq(a: f32, b: f32) -> bool {
    a == b || (is_undefined(a) && is_undefined(b))
}

/// `FloatOptional::unwrapOrDefault`.
#[inline]
pub fn unwrap_or_default(value: f32, default: f32) -> f32 {
    if is_undefined(value) { default } else { value }
}
