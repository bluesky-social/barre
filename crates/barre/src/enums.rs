//! Yoga's enums (`yoga/YGEnums.h`, `yoga/enums/*.h`).
//!
//! Discriminants match the C enum values exactly, so `as u32` gives the C
//! value. `from_raw` converts a C value (which may be out of range) and
//! `name` gives the string `YG*ToString` returns.

use std::ffi::CStr;

macro_rules! yoga_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($variant:ident = $value:literal => $str:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
        #[repr(u8)]
        pub enum $name {
            #[default]
            $($variant = $value),+
        }

        impl $name {
            /// Every variant, in declaration (ordinal) order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// Converts a C enum value; `None` if out of range.
            #[inline]
            pub const fn from_raw(raw: u32) -> Option<Self> {
                match raw {
                    $($value => Some($name::$variant),)+
                    _ => None,
                }
            }

            /// The string `YG<Enum>ToString` returns.
            pub const fn name(self) -> &'static CStr {
                match self {
                    $($name::$variant => $str,)+
                }
            }

            /// The string `YG<Enum>ToString` returns for a raw C value
            /// ("unknown" when out of range).
            pub const fn raw_name(raw: u32) -> &'static CStr {
                match Self::from_raw(raw) {
                    Some(v) => v.name(),
                    None => c"unknown",
                }
            }
        }
    };
}

// The first variant of each enum is its `Default`; that is only used for
// zero-initialisation, never as a style default (style defaults are explicit
// in `Style::new`).

yoga_enum!(Align {
    Auto = 0 => c"auto",
    FlexStart = 1 => c"flex-start",
    Center = 2 => c"center",
    FlexEnd = 3 => c"flex-end",
    Stretch = 4 => c"stretch",
    Baseline = 5 => c"baseline",
    SpaceBetween = 6 => c"space-between",
    SpaceAround = 7 => c"space-around",
    SpaceEvenly = 8 => c"space-evenly",
    Start = 9 => c"start",
    End = 10 => c"end",
});

yoga_enum!(BoxSizing {
    BorderBox = 0 => c"border-box",
    ContentBox = 1 => c"content-box",
});

yoga_enum!(Dimension {
    Width = 0 => c"width",
    Height = 1 => c"height",
});

yoga_enum!(Direction {
    Inherit = 0 => c"inherit",
    LTR = 1 => c"ltr",
    RTL = 2 => c"rtl",
});

yoga_enum!(Display {
    Flex = 0 => c"flex",
    None = 1 => c"none",
    Contents = 2 => c"contents",
    Grid = 3 => c"grid",
});

yoga_enum!(Edge {
    Left = 0 => c"left",
    Top = 1 => c"top",
    Right = 2 => c"right",
    Bottom = 3 => c"bottom",
    Start = 4 => c"start",
    End = 5 => c"end",
    Horizontal = 6 => c"horizontal",
    Vertical = 7 => c"vertical",
    All = 8 => c"all",
});

yoga_enum!(ExperimentalFeature {
    WebFlexBasis = 0 => c"web-flex-basis",
    FixFlexBasisFitContent = 1 => c"fix-flex-basis-fit-content",
});

yoga_enum!(FlexDirection {
    Column = 0 => c"column",
    ColumnReverse = 1 => c"column-reverse",
    Row = 2 => c"row",
    RowReverse = 3 => c"row-reverse",
});

yoga_enum!(GridTrackType {
    Auto = 0 => c"auto",
    Points = 1 => c"points",
    Percent = 2 => c"percent",
    Fr = 3 => c"fr",
    Minmax = 4 => c"minmax",
});

yoga_enum!(Gutter {
    Column = 0 => c"column",
    Row = 1 => c"row",
    All = 2 => c"all",
});

yoga_enum!(Justify {
    Auto = 0 => c"auto",
    FlexStart = 1 => c"flex-start",
    Center = 2 => c"center",
    FlexEnd = 3 => c"flex-end",
    SpaceBetween = 4 => c"space-between",
    SpaceAround = 5 => c"space-around",
    SpaceEvenly = 6 => c"space-evenly",
    Stretch = 7 => c"stretch",
    Start = 8 => c"start",
    End = 9 => c"end",
});

yoga_enum!(LogLevel {
    Error = 0 => c"error",
    Warn = 1 => c"warn",
    Info = 2 => c"info",
    Debug = 3 => c"debug",
    Verbose = 4 => c"verbose",
    Fatal = 5 => c"fatal",
});

yoga_enum!(MeasureMode {
    Undefined = 0 => c"undefined",
    Exactly = 1 => c"exactly",
    AtMost = 2 => c"at-most",
});

yoga_enum!(NodeType {
    Default = 0 => c"default",
    Text = 1 => c"text",
});

yoga_enum!(Overflow {
    Visible = 0 => c"visible",
    Hidden = 1 => c"hidden",
    Scroll = 2 => c"scroll",
});

yoga_enum!(PositionType {
    Static = 0 => c"static",
    Relative = 1 => c"relative",
    Absolute = 2 => c"absolute",
});

yoga_enum!(Unit {
    Undefined = 0 => c"undefined",
    Point = 1 => c"point",
    Percent = 2 => c"percent",
    Auto = 3 => c"auto",
    MaxContent = 4 => c"max-content",
    FitContent = 5 => c"fit-content",
    Stretch = 6 => c"stretch",
});

yoga_enum!(Wrap {
    NoWrap = 0 => c"no-wrap",
    Wrap = 1 => c"wrap",
    WrapReverse = 2 => c"wrap-reverse",
});

/// `yoga::PhysicalEdge` (`yoga/enums/PhysicalEdge.h`): the four physical
/// edges, with the same values as the matching [`Edge`]s.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PhysicalEdge {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
}

impl PhysicalEdge {
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// `YGErrata` / `yoga::Errata`: a bit set, not an enumeration.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Errata(pub u32);

impl Errata {
    pub const NONE: Errata = Errata(0);
    pub const STRETCH_FLEX_BASIS: Errata = Errata(1);
    pub const ABSOLUTE_POSITION_WITHOUT_INSETS_EXCLUDES_PADDING: Errata = Errata(2);
    pub const ABSOLUTE_PERCENT_AGAINST_INNER_SIZE: Errata = Errata(4);
    pub const MIN_SIZE_UNDEFINED_INSTEAD_OF_AUTO: Errata = Errata(8);
    pub const FLEX_FIRST_PASS_USES_RUNNING_TOTALS: Errata = Errata(16);
    pub const ALL: Errata = Errata(2147483647);
    pub const CLASSIC: Errata = Errata(2147483646);

    /// Whether any bit of `other` is set in `self`.
    #[inline]
    pub const fn intersects(self, other: Errata) -> bool {
        self.0 & other.0 != 0
    }

    /// The string `YGErrataToString` returns (exact matches only).
    pub const fn raw_name(raw: u32) -> &'static CStr {
        match raw {
            0 => c"none",
            1 => c"stretch-flex-basis",
            2 => c"absolute-position-without-insets-excludes-padding",
            4 => c"absolute-percent-against-inner-size",
            8 => c"min-size-undefined-instead-of-auto",
            16 => c"flex-first-pass-uses-running-totals",
            2147483647 => c"all",
            2147483646 => c"classic",
            _ => c"unknown",
        }
    }
}

impl std::ops::BitOr for Errata {
    type Output = Errata;
    fn bitor(self, rhs: Errata) -> Errata {
        Errata(self.0 | rhs.0)
    }
}
