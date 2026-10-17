# Yoga issue tracker audit

Audit of `github.com/react/yoga` (formerly `facebook/yoga`) issues for layout
correctness bugs, performance problems, and semantics barre must preserve.
Methodology: pulled all 685 issues (`gh issue list --state all`), filtered by
label (`Algorithm`, `bug`) and title keywords, read ~190 full issue bodies +
comment threads. "Still present?" calls are based on reading the pinned
source (`vendor/yoga` @ `52f6d0bd`, 2026-10-16) via `./spec`, not on building
or running anything; "confirmed" means the exact code path described in the
issue was read at the pin, "likely" is a judgment call from code shape plus
issue/comment evidence, "unknown" means neither.

Two bugs are already known project priorities and are not re-researched here
beyond a cross-reference: **yoga#1552** (`flexBasis: '100%'` not recomputed
on rotation) and **yoga#2019** (stale `computedFlexBasis` across relayouts).
Both are confirmed still present at the pin (see their entries below).

## Summary table

| # | Title (short) | Category | Priority | Repro? | Still present? |
|---|---|---|---|---|---|
| 1552 | flexBasis % not recomputed on rotation | Caching/Relayout | P1 | C++ | **confirmed present** |
| 2019 | stale computedFlexBasis across relayouts | Caching/Relayout | P1 | C++ | **confirmed present** |
| 1003 | cached measurement ignores min/max | Caching/Relayout | P1 | JS | **confirmed present** |
| 2007 | computeMinContentMainSize recurses 2x/child (O(2^depth)) | Performance | P1 | C++, timed | **confirmed present** |
| 2038 | flexShrink runaway from FMA residual past #1974's guard | Flex distribution | P1 | C++ | **confirmed present** |
| 2035 | free space computed from min-clamped flex basis | Flex distribution | P1 | JS/HTML | **confirmed present** |
| 1974 | shrink-factor div-by-near-zero guard (fixes #1665) | Flex distribution | P1 (fix) | C++ | confirmed merged, insufficient (see #2038) |
| 1665 | flexBasis+flexShrink+borderWidth wrong value | Flex distribution | P1 | RN/HTML | **fixed** (by #1974) |
| 2027 | space-around misplaces first child with measure func | Measure func / rounding | P1 | JS | **confirmed present** (ack'd, blocked on other PRs) |
| 1784 | stale available-width info for measure funcs on resize | Caching/Relayout | P1 | described, no isolated repro | likely present |
| 1626 | % padding not updated on parent resize w/o dirtying child | Caching/Relayout | P1 | JS | likely present |
| 1659 | `YGNodeFree` doesn't mark parent dirty | Dirty marking / API | P1 | C++ | **fixed** (confirmed) |
| 2006 | minWidth diff on first child breaks even distribution | Flex distribution | P1 | JS/HTML | **fixed** (errata-gated, confirmed) |
| 872 | min-width % resolves against wrong ancestor | Percentage | P1 | JS | likely present (fix contested) |
| 1142 | alignContent+minHeight/minWidth wrap positions wrong | Align/Wrap | P1 | playground | **fixed** in 3.0/3.2 |
| 1739 | align-content+align-items regression (1.19→3.1) | Align | P1 | JS | **fixed** in 3.2.0 |
| 1695 | alignItems broken with alignContent:stretch | Align | P1 | playground | **fixed** in 3.1.0 |
| 967 | max-height+flex-grow leaves unallocated space | Min/Max + flex | P1 | link | likely present |
| 1770 / 1769 | min-height+flex produces wrong height (dup pair) | Min/Max | P1 | JSX | likely present |
| 1792 | alignContent:stretch doesn't stretch flexWrap rows | Align/Wrap | P1 | snack | likely present |
| 1788 | margin + align-items:stretch double-subtracts margin | Align | P1 | RN | likely present |
| 1358 | baseline-align width calc wrong (in-tree test itself) | Baseline | P1 | C++ test | likely present |
| 1932 | `calculateBaseline` returns NaN if child unmeasured | Baseline | P1 | JSX | **confirmed present** |
| 877 | skipped layouts (cache hit) don't get rounded | Rounding | P1 | described | likely present (structurally unchanged) |
| 1528 | top position wrong w/ 0.5pt height row at 3x scale | Rounding | P1 | Swift | likely present |
| 1651 | flex-shrink float overflow w/ specific `.01` decimals | Flex distribution | P1 | JS | likely present (same bug family as #2038) |
| 1783 | floating precision oversizes main dim (shrink factors) | Flex distribution | P1 | described+code | partially fixed by #1974, see #2038 |
| 674 | min/max on opposite edges leaves gaps between stretched children | Min/Max | P1 | Java | likely present |
| 677 | aspect ratio ignores min/max dimensions | Aspect ratio | P1 | JS | likely present |
| 1418 | aspect ratio on flex items differs from CSS spec | Aspect ratio | P1 | RN | likely present (design predates CSS `aspect-ratio`) |
| 1067 | flexWrap+alignItems+margin: cross-axis offset wrong | Align/Wrap | P2 | snack | confirmed on RN 0.79 (comment) |
| 1661 | % width child in content-sized row parent differs from web | Percentage | P2 | playground | likely present |
| 727 | measured node + max-width% + absolute position: parent too small | Measure func + Absolute | P2 | C++ test | likely present |
| 730 | web-compat fixture fails (shrink+padding+min-width) | Min/Max + flex | P2 | HTML | likely present |
| 485 | min/max width + % flex-basis + flex-grow fails web compat | Min/Max + flex | P2 | HTML | likely present (same root cause family as #2035) |
| 470 | text unexpectedly collapsed (nested flexGrow/flexShrink) | Flex distribution | P2 | RN | confirmed as of 2020 (comment) |
| 570 | parent ignores own min/max when children clamp | Min/Max | P2 | JS | disputed/unconfirmed |
| 931 | flex-shrink "snaps" to wrong size at specific widths | Flex distribution | P2 | link+RN | unknown/stale |
| 1794 / 1827 | 100%/50% width children overflow or get 0 width | Percentage | P2 | playground | likely present |
| 1796 | maxWidth ignored when same-axis padding exceeds it | Min/Max | P2 | playground | likely present |
| 1797 | maxHeight+UseWebDefaults: parent ignores child's clamp | Min/Max | P2 | described | likely present |
| 1856 | flexWrap + position:relative: no effect on main axis | Wrap/Absolute | P2 | JS | likely present |
| 1849 | align-items breaks under flex-wrap (webDefaults false) | Align/Wrap | P2 | playground | disputed (web also fails per comment) |
| 1819 | zero width unexpectedly stretches height | Cross-axis sizing | P2 | playground | unknown (closed, no fix comment) |
| 1685 | zero size nesting column/row 5 levels deep | Sizing | P2 | RN repo link | unknown, no comments |
| 1660 | absolute-node measure semantics unclear/inconsistent | Measure func + Absolute | P2 | JS | unknown (may be working as intended) |
| 1126 / 1298 | no min-content/max-content sizing mode | Foundational/Measure | P2 | N/A (feature) | **confirmed absent** (enables fixing several of the above) |
| 1295 | aspectRatio CSS-compliance fixture set | Aspect ratio | P2 | HTML fixtures | test fixtures only, not yet landed |
| 901 / 1574 | pixel-grid rounding accumulates error | Rounding | P2 | C/discussion | likely improved, not fully verified |
| 734 | perf regression since OOP refactor (2018) | Performance | P2 | benchmark numbers | historical, unknown at pin |
| 1858 | JS/WASM bindings: object identity not preserved | Bindings | P3 | JS | N/A to barre's C ABI |
| 1904 | `YogaProps` (Java) missing gap getters/setters | Bindings | P3 | described | binding-only |
| 1778 | `YGNodeStyleGetGap` doesn't return unit | API surface | P3 | described | fixed same day |
| 1818 | JS bindings: use-after-free on node freed then reused | Bindings / memory safety | P3 | JS | N/A to barre (no GC) |
| 1630 | playground vs C++ "discrepancy" | — | P3 (not a bug) | C++ | reporter's own misunderstanding |
| 1787 | flex squeezes static sibling to 0 | — | P3 (not a bug) | snack | documented `flex:1` semantics, see gotchas |
| 1680 | measured child stretched by default align-items | — | P3 (not a bug) | screenshot | default `align-items:stretch`, working as intended |
| 1691 | % width + gap leaves wrong remaining space | — | P3 (user error) | Swift | resolved by using flexGrow instead of % |
| 1676 | UILabel text change stops shrinking width (3.1.0) | Flex distribution | P3 | demo zip, uncondensed | insufficient info |
| 1822 | negative gap values rejected | — | P3 (correct per spec) | N/A | not a bug |

Closed-and-fixed items worth knowing about (brief, not tracked as open bugs):
`#1658` absolute % position wrong when parent/child flex directions differ
(fixed); `#1652`/`#1653` `gap` incorrectly gutters around absolutely
positioned children (fixed, RN 0.74+); `#1657` RTL `paddingStart`/`paddingEnd`
vs. % width bug (fixed); `#850` absolute % resolved against content box
instead of CSS containing-block padding edge (fix proposed, contested by a
counter-example in the same thread — unclear if the final merge handled it);
`#490` rounding off-by-one, the 2017 fix (commit `aa5b296`) that produced the
rounding algorithm `#901`/`#1574` are now complaining about; `#661` 3x-scale
rounding errors (fixed); `#824` text node shrinking on repeated layout passes
via `YGRoundToPixelGrid` (fixed same week); `#428` measure-func + dimension +
max% wrong size (fixed); `#466` `maxHeight` incorrectly included margin
(fixed).

---

## P1 — affects common React Native layouts, or caching/relayout correctness

### yoga#1552 — `flexBasis: '100%'` not recomputed on rotation (known, cross-ref)
Open since 2024-01-14. Already tracked as a barre priority (docs/PLAN.md).
**Confirmed present** by reading `computeFlexBasisForChild`
(`yoga/algorithm/CalculateLayout.cpp:234`, `./spec computeFlexBasisForChild`):
the resolved-flex-basis branch only recomputes when
`computedFlexBasis.isUndefined()`, or under the `WebFlexBasis` experimental
flag, when `computedFlexBasisGeneration != generationCount` — and the
generation-count check is a per-node equality test, not "was this subtree
re-visited for *this* set of constraints." A comment on the issue (from a
maintainer) gives a minimal repro confirmed on main `52f6d0b` (exactly
barre's pin):
```cpp
YGNodeRef root = YGNodeNew();
YGNodeStyleSetWidth(root, 400); YGNodeStyleSetHeight(root, 1000);
YGNodeRef child = YGNodeNew();
YGNodeStyleSetFlexBasisPercent(child, 25);
YGNodeInsertChild(root, child, 0);
YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR); // child height: 250
YGNodeStyleSetHeight(root, 1200);
YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR); // child height: still 250, should be 300
```

### yoga#2019 — stale `computedFlexBasis` reused across layouts (known, cross-ref)
Open since 2026-09-02. Already tracked (docs/PLAN.md). **Confirmed present**
— same code path as #1552. The issue body is an exceptionally detailed
repro/mechanism writeup (eight-node tree, three layouts at 400×800 →
800×400 → 400×800, a `flex:1` measured text node that should return to its
original height and comes back at the intermediate layout's value instead).
Mechanism, in the issue's own words: (1) `computedFlexBasis` is cleared only
by `markDirtyAndPropagate` walking *up* from a dirtied node, never pushed
down to descendants; (2) the retention guard in `computeFlexBasisForChild`
only checks *whether* a value exists, not *which layout* produced it; (3) an
ancestor's measurement cache can skip the subtree during the max-content
pass, so the stale value is never refreshed before a definite-size pass
consumes it. Full C++ repro included in the issue body (condensed, same
shape as #1552's). A C++ regression test (`tests/YGRelayoutTest.cpp`-style:
layout, relayout at a new size, relayout back at the original size, assert
the measured node's height is unchanged) is suggested directly in the issue.
Note: `conformance/tests/YGStaleFlexBasisTest.cpp` already exists untracked
in the barre working tree — another agent may already be working this.

### yoga#1003 — `YGNodeCanUseCachedMeasurement()` ignores min/max dimensions
Open since 2020-05-04, `Algorithm` label. A child with `height: 10,
minHeight: 40%, maxHeight: 60%` is measured once against an undefined parent
height (giving 10, correct), then the parent is laid out at height 30 (the
clamped expectation is `max(12, min(10,18)) = 12`), but the cache returns
the stale 10. **Confirmed present**: `canUseCachedMeasurement`
(`yoga/algorithm/Cache.cpp:45`, `./spec canUseCachedMeasurement`) takes only
`(widthMode, availableWidth, heightMode, availableHeight, lastWidthMode,
lastAvailableWidth, lastHeightMode, lastAvailableHeight, lastComputedWidth,
lastComputedHeight, marginRow, marginColumn, config)` — no min/max
parameters at all, so it structurally cannot detect that a min/max
constraint on the node itself changed the applicable clamp between calls. A
maintainer PR was pushed ("thanks for pointing that out, I just pushed a PR
for a fix") but a follow-up comment says adopting it "leads to increasing
the binary size for each node," so it is unclear whether a fix ever landed.
Directly in scope for barre's stated goal of keying cached values by the
constraints that produced them.

### yoga#2007 — `computeMinContentMainSize` recurses into every child twice (O(2^depth))
Open since 2026-08-13. No comments. **Confirmed present** by reading the
function (`yoga/algorithm/CalculateLayout.cpp:856`,
`./spec computeMinContentMainSize`): the container branch does
```cpp
float childMain = computeMinContentMainSize(child, nodeMainAxis, direction, ownerWidth, ownerHeight);
...
float childCross = computeMinContentMainSize(child, nodeCrossAxis, direction, ownerWidth, ownerHeight);
```
— two full recursive calls per child, each walking that child's entire
subtree again, with no memoization and no cache involvement (it's a pure
read-only min-content walk, invisible to cache counters). That's
`T(d) = 2*T(d-1)`. This is reachable from ordinary markup: it's called by
`computeAutoMinMainSize` for every in-flow flex item whenever
`Errata::MinSizeUndefinedInsteadOfAuto` is *not* set, i.e. under the modern
default (CSS Flexbox §4.5 automatic minimum size). The issue includes a
timed repro (plain nested flex rows, no measure funcs) showing ~4x time per
two levels of nesting (0.004s at depth 16, 4.233s at depth 26); it
extrapolates depth 30 to "around a minute, which reads as a hang rather than
slowness." Proposed fix: compute both axis components in a single walk per
subtree. Directly relevant to barre's "faster" goal and worth a benchmark in
`bench/`.

### yoga#2038 — flexShrink runaway (~8e9) via FMA residual slipping past #1974's guard
Open since 2026-10-14, sibling to #1665/#1783. **Confirmed present**: read
`distributeFreeSpaceFirstPass` and `distributeFreeSpaceSecondPass`
(`yoga/algorithm/CalculateLayout.cpp:1269`, `:1063`) — the guard added by
#1974 is `shrinkFactorMagnitude < 1e-6f` (absolute epsilon). The issue shows
a case where the true shrink-factor total is driven to a residual of
`1.9e-6` — just above the guard — by FMA (`fmadd`) contraction in
`distributeFreeSpaceFirstPass`'s subtraction, specifically because the
compiler uses the *unrounded* product in a fused multiply-add rather than
the separately-rounded float value `calculateFlexLine` computed. Reproduces
only with `-ffp-contract=on` (the default for `-O2` on Apple clang/arm64),
which is also why it can't be seen on the wasm playground. Full C API repro
(4 nodes: a row, an `aspectRatio` box with `flexShrink: 3`, and an
overflowing non-shrinkable sibling) in the issue body, with a table of which
`flexShrink` values fail (3, 6, 9, 11, 12, 19, 22, 999 — no safe non-zero
value exists). Fix suggested in a referenced PR #2039: round the product to
`float` before subtracting, and replace the epsilon with an exact count of
remaining shrinkable items (tracked in lockstep) rather than any epsilon at
all, since the residual scales with `flexShrink * flexBasis`. **Not yet
applied at the pin** — the guard is still the bare `1e-6f` check.
```cpp
YGNodeRef root = YGNodeNew();
YGNodeStyleSetWidth(root, 350); YGNodeStyleSetHeight(root, 350);
YGNodeRef row = YGNodeNew();
YGNodeStyleSetFlexDirection(row, YGFlexDirectionRow);
YGNodeInsertChild(root, row, 0);
YGNodeRef box = YGNodeNew();
YGNodeStyleSetAspectRatio(box, 22.0f/24.0f); // basis = 23 * 22/24 = 21.083334
YGNodeStyleSetFlexShrink(box, 3);
YGNodeInsertChild(row, box, 0);
YGNodeRef sibling = YGNodeNew(); // overflows the row; flexShrink stays 0
YGNodeStyleSetWidth(sibling, 600); YGNodeStyleSetHeight(sibling, 23);
YGNodeInsertChild(row, sibling, 1);
YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
// expected box 0x0, actual ~8.29e9 x 9.04e9
```

### yoga#2035 — free space computed from min-clamped flex bases
Open since 2026-10-02. **Confirmed present**: `calculateFlexLine`
(`yoga/algorithm/FlexLine.cpp:16`) accumulates
`sizeConsumed += flexBasisWithMinAndMaxConstraints + ...` where
`flexBasisWithMinAndMaxConstraints` comes from
`boundAxisWithinMinAndMax(...)` — the *clamped* basis — and both
`distributeFreeSpaceFirstPass`/`SecondPass` start each item's tentative size
from that same clamped `childFlexBasis`. Per CSS Flexbox §9.7, the initial
free space should subtract the *unclamped hypothetical main sizes* (flex
base sizes) of unfrozen items; an item is frozen at its minimum only if it
would otherwise violate it. Concretely:
```html
<div style="display: flex; width: 120px; height: 10px">
  <div style="flex: 1; min-width: 30px"></div>
  <div style="flex: 2"></div>
</div>
<!-- Chrome/Firefox/Safari: 40 / 80. Yoga: 60 / 60 -->

<div style="display: flex; width: 300px; height: 10px">
  <div style="flex: 1; min-width: 200px"></div>
  <div style="flex: 1"></div>
  <div style="flex: 1"></div>
</div>
<!-- browsers: 200 / 50 / 50. Yoga: 233 / 34 / 33 -->
```
Equivalent `yoga-layout` 3.2.1 JS repro included in the issue; same result
with `setUseWebDefaults(true)`. The issue notes #485 may share the root
cause but adds a percentage flex-basis and max-width on top — this is the
minimal case. No comments yet; not fixed.

### yoga#1974 — shrink-factor division-by-near-zero guard (fixes #1665)
Merged PR (labels `CLA Signed`, `Shared with Meta`), 2026-06-09.
**Confirmed merged** at the pin (see #2038's analysis above) but
**insufficient**: the `1e-6f` absolute-epsilon guard it introduced in
`distributeFreeSpaceSecondPass` does not catch the FMA-contraction residual
in #2038. Fixes the original bug in #1665 (all flex children frozen to
min-width in the first pass drove `totalFlexShrinkScaledFactors` to a
near-zero-but-not-exactly-zero value via ordinary float cancellation,
tripping an exact `== 0` check and dividing `remainingFreeSpace` by ~1e-7).
Regression test referenced: `YGFlexShrinkBorderBug.flex_basis_0_border_minwidth_row`.

### yoga#1665 — `flexBasis`+`flexShrink`+`borderWidth` produces a wrong value
Closed 2026-08-11 (`bug`, `Algorithm`), **fixed by #1974**. RN repro: four
row children with `flexBasis: 0, flexGrow: 1, flexShrink: 1`, one with
`borderWidth: 0.594443` (vs. `0.594441`/`0.594442` working) — a borderWidth
difference on the order of 1e-6 flips correctness, same float-cancellation
root cause as #1974/#2038.

### yoga#2027 — `justifyContent: space-around` misplaces the first child when children use a measure function
Open since 2026-09-20. **Confirmed present** (per maintainer comment, not
independently spec-checked). Repro: two 1×1 children in a 5-wide row with
`space-around`; fixed-size children give `a.left=1, b.left=3` (correct —
half the 1.5-unit gap, rounded). Measured children instead give
`a.left=0, b.left=3`. Minimal JS repro in the issue body. Root cause per the
maintainer's own comment: `roundLayoutResultsToPixelGrid`
(`PixelGrid.cpp`) floors a node's *position* whenever it's tagged
`NodeType::Text`, and any node with a measure function gets that tag
automatically — so the first child's correct fractional position is floored
to 0. The maintainer explicitly flags this as known tech debt
(internal TODO `t18095186`) and says two open PRs (#1860, #2011) are
already reworking the rounding logic, so a standalone fix here would
collide with those — i.e., **won't be fixed independently, wait for the
rounding rewrite**. Ink (terminal React renderer, where every leaf is a
measured text node) has two `space-around` tests marked as expected
failures because of this. Relevant to barre's rounding/measure-func work
since the underlying assumption ("every leaf with a measure func is text,
so ceil/floor its rounding") is itself suspect (see also #1528, #2027's
cousin bugs).

### yoga#1784 — stale available-width info for measure-function nodes on slow resize
Open since 2025-02-06, no labels. No isolated minimal repro (reporter
explicitly apologizes: percentage dimensions aren't supported in the
playground, which they say is crucial to the repro). Described: a wrapping
text node inside a 35%-width container gets its measure function invoked
with a stale available width while the container is resized incrementally
(140→180px: at 161px wide it's measured with 121px instead of the correct
141px). Reporter's team patched it three ways, settling on propagating a
`sizeChange` boolean through `calculateLayoutInternal`'s recursion that
forces a revisit once any ancestor's style dimensions changed mid-pass. A
maintainer comment agrees this is a known gap: "Measurement cache uses
available size and measure mode to know whether measurement can be reused,
but ... does not include containing block dimensions, which % use as
reference." Same caching-key-is-incomplete family as #1003 and the
flex-basis staleness bugs (#1552/#2019) — a recurring theme is that Yoga's
caches are keyed by *some* of the inputs that determine the output, not all
of them.

### yoga#1626 — percentage padding not updated when the parent's size changes
Open since 2024-03-14. JS repro: set a child's `paddingPercent(Left, 50)`
against a 10-wide parent (correctly resolves to 5), shrink the parent to
width 5 and recalculate — padding stays at 5 instead of 2.5 — *unless* the
child is separately dirtied by an unrelated style change. Confirmed to also
happen when the parent is not the root (nested repro in comments). A
maintainer comment agrees the cache key is incomplete: "I believe the cache
key should contain (separately) both: the sizing constraint in each axis...
and... the containing block size used for \% resolution." Not fixed; same
root-cause family as #1784/#1003.

### yoga#1659 — `YGNodeFree` does not mark the parent dirty — **fixed**
Closed 2026-06-09 (`bug` label). C++ repro showed `YGNodeFree(child0)`
leaves `YGNodeIsDirty(root) == false`, and `YGNodeFreeRecursive` only
dirties the parent incidentally (via `YGNodeRemoveChild`, only when the
freed node itself had children). **Confirmed fixed** at the pin:
`YGNodeFree` (`yoga/YGNode.cpp:41`) now does
`owner->removeChild(node); node->setOwner(nullptr); owner->markDirtyAndPropagate();`
unconditionally when an owner exists. Relevant to barre: any node-removal
API must dirty the ex-parent, not just explicit `removeChild`/style calls.

### yoga#2006 — a minWidth difference on the first child breaks even distribution — **fixed (errata-gated)**
Closed 2026-09-09. Repro: three flex items with equal `flexGrow`/`flexShrink`
and different `minWidth` on each — breaks unless all items share the same
`minWidth`/`flexBasis`. **Confirmed fixed** at the pin, and confirmed
**intentionally errata-gated for back-compat**: a code comment directly
above `distributeFreeSpaceFirstPass`'s "use original totals, not running
totals" logic cites `https://github.com/react/yoga/issues/2006` by URL and
explains: dividing by the *running* (already-reduced) totals as items freeze
is "the pre-fix behavior, preserved for existing layouts behind an errata
bit that is set on new configs by default" — i.e. this is exactly the kind
of API/semantics contract (see gotchas section) barre must replicate: new
configs get the corrected behavior, but an errata bit exists to reproduce
the old (buggy) behavior for existing layouts that may depend on it.

### yoga#872 — `min-width: 50%` resolves against the wrong ancestor
Open since 2019-03-18, `Algorithm`. JS repro (codesandbox) where
`childNode`'s `min-width: 50%` resolves against the root's width instead of
its immediate parent's width. A maintainer pushed a fix PR ("thanks for
reporting, I just pushed a PR for the fix") but the very next comment says
"that fix reveals a previous bug by failing other unit tests" and that
"it seems that max-width with percent is also bugged" — so likely **still
present in some form**, with `max-width` percent sharing the bug.

### yoga#1142 — `alignContent`+`minHeight`/`minWidth` positions children outside the parent — **fixed**
Closed 2024-12-03 (`Algorithm`). Repro: `flexDirection: row, minWidth: 500,
minHeight: 200, flexWrap: wrap, alignContent: flexEnd` with six 100×100
children renders two rows with **negative** `y` — children positioned
outside the parent bounds. A comment says "This was fixed in either Yoga 3.0
or Yoga 3.2" with a playground link showing correct output. Not
independently re-verified against the pin via `./spec`, but the fix is
recent (2.0→3.x era) and the pin (post-3.2) should include it.

### yoga#1739 — combining `align-content`+`align-items` regressed from 1.19.0→3.1.0 — **fixed in 3.2.0**
Closed 2024-12-03 (`bug`, `Algorithm`). Repro: `alignContent: stretch,
alignItems: flex-end` on a wrapped row diverges from Chrome, matching only
on Yoga 1.19.0. Comment: "Fixed in Yoga 3.2.0 with above PR" (screenshot
shown matching expected).

### yoga#1695 — `alignItems` broken when combined with `alignContent: stretch` — **fixed in 3.1.0**
Closed 2024-12-03 (`Algorithm`). Repro via playground link; a comment
diagnoses it as "a bug with `alignContent: stretch` impl," then "Fixed as of
3.1.0" with a before/after screenshot.

### yoga#967 — `max-height`+`flex-grow` leave unallocated space at the bottom of the container
Open since 2020-02-03, `Algorithm`. Link-based repro (playground,
expired tinyurl) showing space left unallocated below the largest
sub-component even though it has a 1000px `max-height` headroom; matches
Chrome/Safari without the gap. A maintainer comment: "the current algorithm
doesn't comply correctly to the spec here," pointing at PR #1015 which
addresses "the underlying problem... in more depth" — described as a
partial fix, not a full resolution. **Likely still present**, no closing
comment confirms resolution.

### yoga#1770 (and duplicate #1769) — `min-height` combined with `flex` produces incorrect height
Both open (#1770 2024-12-17, `Algorithm`; #1769 2024-12-14, unlabeled, same
bug). Repro: two children both `flex: 200` in a 400-tall column, one with
`minHeight: 100` — CSS flexbox clamps that child to `max(flexResult,
minHeight)` without inflating the total; Yoga instead *adds* the minHeight
on top, as if it were padding. Playground links in both; no comments, no
fix.

### yoga#1792 — `alignContent: stretch` doesn't stretch rows in a `flexWrap: wrap` layout
Open since 2025-02-28, no labels. Snack repro (`flexDirection: column,
flexWrap: wrap`, cross-axis `alignSelf` ineffective because the implicit row
containers don't stretch to fill the cross axis despite `alignContent:
stretch`). Matches web; iOS/Android diverge. No comments. **Likely present.**

### yoga#1788 — margin is subtracted multiple times under `alignItems: 'stretch'`
Open since 2025-02-19, no labels. RN repro: parent width 400, child
`margin: 24, alignSelf: stretch, aspectRatio: 16/9` — expected child width
360 (400 − 2×24), actual significantly narrower, as if the margin were
applied more than twice. Two "+1" bump comments, no fix.

### yoga#1358 — baseline-alignment width calculation is wrong *in Yoga's own test file*
Open since 2023-09-03, `Algorithm`. Points directly at
`tests/YGAlignBaselineTest.cpp:418-454`: the test asserts
`root_child1_child1.left == 500`, but given the parent (`root_child1`) is
only 500px wide with two 500px-wide children in a `row`, both children can
only take 250px each to stay in bounds — matching Firefox/Chrome's actual
behavior (HTML fixture included). I.e. the checked-in expected value itself
encodes the bug. Maintainer reply: "feel free to debug the logic in
CalculateLayout.cpp for laying out a box with baseline alignment" — no fix
has landed. Good candidate for a barre conformance-test deep dive since it
points at a specific wrong assertion in Yoga's own suite (worth checking
whether barre's ported copy of `YGAlignBaselineTest.cpp` carries the same
wrong expectation — if so, barre currently matches Yoga's bug rather than
CSS, which `conformance/divergences.md` should probably note either way).

### yoga#1932 — `calculateBaseline` returns NaN if the baseline child hasn't been measured
Open since 2026-04-14, no labels. **Confirmed present**: read
`calculateBaseline` (`yoga/algorithm/Baseline.cpp:17`, `./spec
calculateBaseline`) — when no in-flow child has `positionType != Absolute`
and `getLayoutChildren()` finds a `baselineChild` whose own
`measuredDimension(Dimension::Height)` was never written (no measure/baseline
func of its own and it fell through to the generic fallback
`return node->getLayout().measuredDimension(Dimension::Height);`), the
value returned can be NaN with no guard. Playground repro (nested row inside
a `alignItems: baseline` row) shows the aligned node getting `lineHeight: 0`
in Yoga vs. correct behavior in Chrome/Safari. No comments; not fixed.

### yoga#877 — layouts skipped via cache hit don't get rounded
Closed 2024-03-14 (`Algorithm`) but resolution is murky. Repro: calculate a
layout producing an unrounded internal value (e.g. `200.015`), which gets
externally rounded to `200`; recalculate with identical constraints without
dirtying — `YGLayoutNodeInternal` returns `false` (cache hit), so
`YGRoundToPixelGrid` is skipped and the stale *unrounded* `200.015` is now
externally visible. **Structurally still present** at the pin: `calculateLayout`
(`yoga/algorithm/CalculateLayout.cpp:2947`) only calls
`roundLayoutResultsToPixelGrid` `if (calculateLayoutInternal(...))` returns
true for the *root* call — if the top-level call is itself a cache hit, no
rounding pass runs for the whole tree at all. A closing comment from a
maintainer is itself uncertain: "Current code rounds whole tree... This is
separately kinda wasteful and not what we want for unowned nodes, but this
specific bug should likely no longer exist[s]" — hedged, not a confirmed
fix. Worth a direct conformance test (layout, then layout again at
identical constraints without dirtying, assert rounding is still applied).

### yoga#1528 — `top` position wrong when the previous sibling's height is 0.5 at 3x scale
Open since 2023-12-19, `Algorithm`. Swift/YogaKit repro: a 0.5pt-height
sibling above a fixed-height view throws off the following view's `top` at
pointScaleFactor 3. A maintainer comment diagnoses the root cause directly:
"Yoga assumes every leaf node is `YGNodeTypeText`, which triggers ceil
instead of round. This assumption is not correct when measure function is
used to delegate to different layout system." Same underlying
"measure-func ⇒ assumed text ⇒ biased rounding" assumption flagged in #2027.
No fix landed.

### yoga#1651 — flex-shrink + specific floating-point decimals overflow computed height/top
Open since 2024-04-26, `Algorithm`/`JavaScript`. JS repro (yoga-layout
3.0.4): three children with `flexShrink: 1`, each measure-func returning a
`.01`-suffixed height (e.g. `10.01`, `12.01`), margins summing larger than
available height — output explodes to values like `5248133` instead of `0`.
Changing any `.01` to `.02` "fixes" it. No comments. This is very likely the
same float-cancellation-in-shrink-distribution family as #1974/#2038/#1783
— worth checking against the #2039 fix (exact shrinkable-item count instead
of any epsilon) once/if that lands, since an epsilon-based guard (current
state at the pin) is unlikely to cover every decimal pattern.

### yoga#1783 — floating precision causes oversized main dimension (`totalFlexShrinkScaledFactors`)
Open since 2025-02-06, no labels, predates #1974/#2038. Points at the same
code region (`yoga/algorithm/CalculateLayout.cpp` around what is now line
~659/1063): "subtraction from `totalFlexShrinkScaledFactors` causes the
value to approach zero but not quite reach it... should instead be `<=
0.001`." A maintainer reply names the right tool (`yoga::inexactEquals`)
but calls the fix "a bit of a sledgehammer." **Status**: #1974's merge did
add an epsilon guard (`1e-6f`, narrower than the `0.001` suggested here),
but #2038 demonstrates it's not sufficient for all cases — so this issue is
best read as "partially addressed, not fully resolved" at the pin.

### yoga#674 — min/max sizes on opposite edges (top/bottom or left/right) leave gaps between stretched children
Open since 2017-11-25, `Algorithm`. Java repro: three stretched rows/three
stretched columns, each pair with asymmetric min/max on opposite edges —
expected no gaps between stretched nodes, actual visible empty space
(animated GIF in the issue). Root cause per a maintainer comment: "doing
only a two-way pass, instead of a loop for resolving the min/max
constraints" (`yoga/Yoga.cpp:2178` at the time). Still open seven years
later with a "can you apply the fix" ping unanswered. **Likely still
present** — the fundamental two-pass-not-iterative structure for combined
min/max resolution would need a real algorithmic change to fix, not a small
patch.

### yoga#677 — aspect ratio does not respect min/max dimensions
Open since 2017-12-01, `Algorithm`. JS repro: a node with `width: 400,
maxWidth: 100, aspectRatio: 2` should clamp to `100×50` but Yoga sizes it
`height = 200` (derived from the *unclamped* width), contradicting Yoga's
own documentation ("AspectRatio respects the Min and Max dimensions of an
item"). Confirmed by a second reporter in 2020 ("Just ran into this issue
today"). No fix landed as of the latest comment.

### yoga#1418 — aspect ratio on flexible items diverges from the CSS `aspect-ratio` spec
Open since 2023-10-11, `Algorithm`. RN repro inside a `ScrollView` with
nested `gap` + `aspectRatio` + `flex` producing an overflow on
Android/iOS not seen on web. A maintainer comment gives useful context for
barre: "aspectRatio in Yoga predates the aspectRatio property in CSS,"
cross-referencing #1295 (a set of gentest-style HTML fixtures diffing Yoga
vs. browsers specifically for aspect-ratio-under-flex cases). This is less
"a bug" than "Yoga's `aspectRatio` is an older, non-spec-compliant
implementation that CSS later standardized differently" — relevant to
barre's accuracy goal, and #1295's fixtures are a ready-made source of
regression tests once that gap is addressed.

---

## P2 — real but niche

**yoga#1067** — `flexWrap`+`alignItems`+`margin`: a margined item inside a
wrapping row is cross-axis-aligned as if its margin were part of its own
box (web/react-native-web excludes margin from the alignment box; native
Yoga includes it). Confirmed still reproducing on RN 0.79 per a 2024 comment
using `experimental_LayoutConformance`.

**yoga#1661** — when a row-direction parent's width is determined by
content and a child has a percentage width, Yoga sizes the parent as if the
`%`-width child contributed its full un-resolved size, instead of the
web behavior (child resolves to 0 against an as-yet-undetermined parent
width). Playground repro, no comments.

**yoga#727** — a `position: absolute, max-width: 90%` wrapping-text node
computes a parent smaller than its own measured content height (C++ test
case included, diffed against expected web fixture HTML).

**yoga#730** — a generated web-compatibility fixture
(`child_min_constraints_when_shrinking`: asymmetric `flex-shrink` +
`flex-basis` + `padding` + `min-width`) fails against Chrome's output.

**yoga#485** — combining `min/max-width` with percentage `flex-basis` and
`flex-grow` fails a web-compat fixture; the reporter of #2035 explicitly
flags this as sharing #2035's root cause (clamped-vs-unclamped flex basis
used for free-space accounting) plus a percentage/max-width interaction on
top.

**yoga#470** — text wrapped in nested `flexShrink`/`flexGrow` containers
goes fully invisible (not just clipped) in one branch of a conditional
layout; reproduced as of a 2020 comment against a contemporary RN build.

**yoga#570** — when a child's `min-height` exceeds its `height`, siblings
correctly position as if the child were at its clamped minimum, but the
*parent's own* height is computed from the child's unclamped `height` —
i.e. parent and children disagree about the child's effective size. A
maintainer couldn't reproduce with one specific HTML snippet, so this needs
re-verification against the pin before use as a regression test.

**yoga#931** — flex-shrink "jumps" to the wrong size at specific container
widths (400 misbehaves, 425/450 behave); cross-referenced by the reporter
against #674. Old (2019), no maintainer response, status unknown.

**yoga#1794 / #1827** — two related reports: a `height: '100%'` sibling
followed by an `auto`-sized wrapping sibling overflows the container instead
of being clamped (#1794, two HTML-equivalent repros, workaround: explicit
`flexShrink: 0`); and a `width: '50%'` child inside an `auto`-width,
`alignItems: center` parent gets 0 width instead of 50% of its immediate
parent (#1827, no comments). Possibly related percentage-resolution-against-
auto-sized-ancestor bugs.

**yoga#1796** — `maxWidth: 10` with `paddingLeft: 100` on the same node
produces a total width of 100 (the unclamped padding-expanded size),
ignoring `maxWidth` entirely. A follow-up comment reports a related 0-width
flicker on resize that may be the same root cause.

**yoga#1797** — with `setUseWebDefaults(false)` (the Yoga default), a
parent with `height: auto` and a child with `height: 700, maxHeight: 500`
sizes the *parent* to 700 (the child's unclamped height) rather than 500
(the child's clamped height); reporter says `setUseWebDefaults(true)` on the
parent works around it.

**yoga#1856** — `position: relative` + `left`/`top` offsets have no effect
along the **main** axis when the parent also has `flexWrap: wrap` (cross-axis
offset still works); diverges from Chrome. Playground-equivalent JSX repro
included.

**yoga#1849** — `align-items` appears to stop working once `flexWrap` is
set; however a follow-up comment says "web also no work," which suggests
this may be a test-case construction issue (missing `alignContent`) rather
than a Yoga bug — needs re-verification before treating as a real
divergence.

**yoga#1819** — closed without a visible fix comment: a width-1 node
inside an `alignItems: flex-start` wrapper unexpectedly takes the full
cross-axis height when width is set to exactly 0 vs. non-zero. Status
unclear.

**yoga#1685** — zero-size output when nesting `flexDirection: column` and
`flexDirection: row` five levels deep in a baseline-related RN component
tree (`WithBaseline`/`NoBaseline`/`FieldWrapper` wrapper pattern); full repo
link given, no comments, not triaged.

**yoga#1660** — questions/possible bugs around what `width`/`height`/mode
an absolutely-positioned node's measure function is invoked with (goes from
sane relative-position values to `NaN`/`MeasureModeUndefined` once
`positionType: absolute` is set); reporter explicitly says "if everything is
working as expected, feel free to close" — needs a maintainer read, not
confirmed as a bug.

**yoga#1126 / #1298** — Yoga has no `min-content`/`max-content` sizing mode
(`YGMeasureModeUndefined` is documented in Yoga's own source as covering
only "max content," explicitly *not* "min content," because Yoga doesn't
support default automatic minimum main sizes the way the comment describes —
though #2007's `computeAutoMinMainSize`/`computeMinContentMainSize` pair
shows that has since been partially walked back). This is foundational: a
real `min-content`/`max-content` mode would likely let several of the above
"differs from web" bugs (#1661, #727, #1794 family) be fixed at the root
instead of patched individually. Not implemented at the pin.

**yoga#1295** — a maintained set of HTML/CSS fixtures
(`gentest/fixtures/YGAspectRatioTest.html`-shaped) specifically diffing
Yoga's `aspectRatio` against spec-compliant CSS `aspect-ratio`, blocked on
text-measurement support in the gentest generator (#1296). Not a single bug
but a ready list of aspect-ratio edge cases (`fill_height`, `fill_max_height`,
`fill_max_width`, `fill_min_height`, `fill_min_width`, `fill_width`) worth
porting into `conformance/fixtures/` directly.

**yoga#901 / #1574** — `YGRoundToPixelGrid`/`YGRoundValueToPixelGrid`
accumulate rounding error (originally reported 2019, re-raised as a design
discussion in 2024). **Likely improved but not fully verified**: reading
`roundLayoutResultsToPixelGrid` (`yoga/algorithm/PixelGrid.cpp:87`) at the
pin shows it now computes in `double` throughout (not `float`) and has a
dedicated fast path — `textRounding && !hasFractionalWidth/Height` — that
rounds the node's own scaled dimension directly instead of going through
the "difference of two independently-rounded absolute edges" computation
that #901 specifically complained about. That fast path only engages when
the node is text-rounded *and* its own dimension already lands on a whole
pixel; the general case still computes width/height as a difference of two
rounded edges, which is the structure #901's proposed PR (#902) wanted to
avoid entirely. A #1574 comment from a maintainer explains *why* Yoga rounds
this way at all (commit `aa5b296`, the original #490 fix): it deliberately
avoids 1px gaps between adjacent rounded children at the cost of this
accumulation risk, trading one visual artifact for another. Treat as a
design tradeoff barre should understand (see gotchas) before "fixing" it.

**yoga#734** — a 2018 report of a "massive performance regression" from a
move to more OOP-style C++ (median benchmark times roughly doubling,
`Huge nested layout` going from 12.7ms (1.7.0) to 50ms (`fbd332d`)). Open,
stale, last comment (2019) claims "Yoga 1.6.0 is two times faster than
1.14.0." Historical signal that large refactors have previously cost Yoga
real performance; no current data at the pin. Useful context for barre's
`bench/` baselines but not an actionable bug on its own.

---

## P3 — cosmetic, spec-lawyering, bindings-only, or not-a-bug

- **yoga#1858** — JS/WASM bindings create a new wrapper object per
  `getParent()`/`getChild()` call instead of preserving object identity.
  Binding-layer only; doesn't apply to barre's C ABI.
- **yoga#1904** — Java `YogaProps` interface is missing gap getters/setters
  that exist on `YogaNode` (and the same gap will apply to the in-flight
  grid-layout PR #1898's properties). Binding surface completeness, not a
  layout bug.
- **yoga#1778** — `YGNodeStyleGetGap` returns a bare float with no unit
  (can't distinguish a percentage gap from a point gap). Closed same day;
  naming note from the same thread: `YGGutter` naming was deliberately
  chosen over "gap" internally to avoid ambiguity with the `all`/`row`/
  `column` gap-kind parameter — worth preserving if barre's API mirrors
  internal naming anywhere.
- **yoga#1818** — JS bindings: calling into a node after `freeRecursive()`
  (e.g. `setMeasureFunc` on an already-freed node) causes memory errors;
  maintainer response points at adopting `FinalizationRegistry`-based GC
  bindings as the real fix. N/A to barre, which has no GC-managed handles,
  but a reminder that **the C ABI gives no protection against use-after-free
  of a freed `YGNodeRef`**, and barre's arena-based handle design should
  decide deliberately what happens on that misuse (abort vs. UB vs. a
  checked panic) rather than inherit silent UB.
- **yoga#1630** — reporter's own "discrepancy between playground and C++"
  turned out to be a misunderstanding of `top`/`left` as parent-relative,
  not absolute screen coordinates. Not a bug.
- **yoga#1787** — a `flex`-only wrapper squeezes a fixed-size sibling to
  zero where web shows it at full size; explained directly by a maintainer
  as RN/Yoga's documented `flex: N` ⇒ `flexGrow: N, flexShrink: N` (unlike
  web's `flexShrink: 0` default for `flex: N` with no explicit shrink) — see
  the gotchas section below. Not a bug, a known compatibility shim.
- **yoga#1680** — a measured child is stretched to the cross-axis by its
  parent's default `align-items: stretch`; explained in-thread as expected
  behavior, reporter's follow-up question (shouldn't the measure func
  override that?) is unanswered but the premise is wrong — `alignItems`
  determines the constraint passed *into* the measure function, not a
  post-hoc override.
- **yoga#1691** — percentage widths plus `columnGap` don't leave the
  expected remaining space; resolved by the reporter switching from
  percentage widths to `flexGrow`, i.e. user-side API choice rather than a
  bug.
- **yoga#1676** — UILabel width stops shrinking after an upgrade to 3.1.0;
  no condensed/minimal repro (only a zip demo), insufficient to assess.
- **yoga#1822** — request to allow negative `gap` values; correctly
  rejected — CSS Box Alignment §3 explicitly disallows negative
  row/column-gap. Not a bug, keep as-is.

---

## Performance

- **yoga#2007** (detailed in P1 above) — `computeMinContentMainSize`'s
  double recursion per child is `O(2^depth)` and the single clearest,
  most concretely reproducible performance bug found in this audit. Confirmed
  present at the pin with exact matching code. High-value target for
  barre's "faster" milestone: fixing this is a correctness-preserving,
  purely mechanical change (single walk computing both axis components),
  not an algorithm redesign.
- **yoga#734** (detailed in P2 above) — historical (2018) report of Yoga
  roughly doubling its own benchmark times during a C++ OOP-style refactor;
  no current data, but useful as a cautionary precedent: a well-intentioned
  structural rewrite previously cost real performance, worth keeping
  `bench/` running continuously against barre's own refactors for the same
  reason.
- **yoga#334** (closed/fixed, 2017) — a measure function was being called
  more times than necessary for a given layout; fixed quickly once
  diagnosed. No longer actionable but a reminder that "excess measure
  calls" has historically been a real, fixable performance bug category
  Yoga has cared about — worth a `bench/` workload that counts measure-func
  invocations per layout, not just wall-clock time.
- **yoga#1904 / #1898** (bindings, noted above) — a grid-layout feature is
  apparently in-flight behind PR #1898. Not performance per se, but a
  heads-up that Yoga's surface area may be growing; barre's C ABI parity
  goal should watch for whether new `YG*` symbols land upstream between now
  and barre's own stabilization.
- The flex-distribution float-precision family (**#1974, #2038, #2035,
  #1783, #1651**) is not primarily a performance bug category, but the
  runaway values they produce (up to `~8e9`) would, in a real app, likely
  manifest as enormous layout trees / huge invalidation regions on the next
  pass — worth keeping in mind that a correctness bug in flex distribution
  can masquerade as a performance cliff downstream.

---

## API / semantics gotchas barre must preserve

- **Measure mode semantics.** Per `#1298`'s own quoted source comment, the
  three `YGMeasureMode` values map to CSS sizing concepts non-obviously:
  `YGMeasureModeUndefined` = *max-content* (not "no constraint"),
  `YGMeasureModeExactly` = *fill available*, `YGMeasureModeAtMost` = *fit
  content*. There is currently no *min-content* mode (`#1126`/`#1298`).
  Measure functions are also always called with constraints **already
  clamped by the node's own min/max dimensions** when a dimension is fixed
  (an old complaint in `#201`/`#160` that was fixed long ago) — but note
  `#1003` shows the *measurement cache* does not re-derive this clamp when
  deciding whether a cached measurement is reusable.
- **`flex: N` means `flexGrow: N, flexShrink: N, flexBasis: 0%` under
  `UseWebDefaults(false)`**, not CSS's `flex-grow: N, flex-shrink: 1,
  flex-basis: 0` (web always shrinks by 1 regardless of the flex shorthand's
  value) (`#1323`, `#1787`, `#701`). `UseWebDefaults(true)` changes this but
  "doesn't set everything to web defaults, because the flag was added before
  Yoga supported many of [the relevant CSS defaults]" — per a maintainer
  comment on `#1787`, it is explicitly an incomplete, historically-grown
  flag, not a clean "web mode" switch. Barre must replicate the *exact*
  partial set of properties `UseWebDefaults` currently touches, not what a
  "complete" web-defaults flag would logically touch.
- **Default `flexDirection` is `column`, default `flexShrink` is `0`**
  without `UseWebDefaults` (`#835`, `#801`) — both contradict Yoga's own
  published docs at the time those issues were filed (since corrected to
  document the divergence rather than fix it; see intentional deviations
  below).
- **Errata bits are the sanctioned way to preserve old, technically-wrong
  behavior.** Confirmed two concrete examples at the pin:
  `AbsolutePositioningIncorrect` (laying out absolute children from the
  *padding* edge instead of the *content* edge of the parent — `#1436`) and
  an unnamed-in-the-issue errata bit gating "divide by running (not
  original) free-space totals" during flex distribution, added specifically
  to fix `#2006` without breaking existing layouts that depended on the old
  numbers. Barre's errata support (`YGConfigSetErrata`) must reproduce not
  just the *documented* errata flags but ones added as narrow, surgical
  fixes like this.
- **`generationCount` + `ExperimentalFeature::WebFlexBasis` interact with
  the core `#1552`/`#2019` caching bugs.** `WebFlexBasis` is the flag that
  changes `computeFlexBasisForChild`'s retention guard from "has any value"
  to "was this value computed in the current generation" — but per the
  `#1552` thread this generation-based check is *itself* suspect ("we can
  compute the flex basis of the same node, under different constraints, in
  the same layout generation" — a maintainer comment). Don't assume turning
  on `WebFlexBasis` is a safe fix for the flex-basis staleness bugs; one
  comment on `#2019` records it as making things *worse* (child becomes 0
  instead of stale-but-plausible).
- **Leaf nodes with a measure function are unconditionally treated as
  `NodeType::Text`** for rounding purposes (ceil-biased width/height
  rounding, and floored position rounding) — this is a real, acknowledged
  assumption-violation source (`#1528`, `#2027`) when a measure function
  delegates to a non-text layout system (e.g. an image, a 3D view, a nested
  independent layout engine). Barre should decide deliberately whether to
  keep this text-biased default or expose a way to opt a measure-func node
  out of it — but whichever is chosen, it must match Yoga's current
  default behavior unless explicitly flagged as a fix.
- **`YGNodeFree`/`YGNodeFreeRecursive` dirty-marking contract**: freeing a
  node must dirty its (former) parent (`#1659`, fixed at the pin) —
  `YGNodeFreeRecursive`'s old behavior of only dirtying via
  `YGNodeRemoveChild`'s side effect (and thus missing leaf-only frees) is
  the kind of narrow lifecycle contract easy to miss in a reimplementation.
- **Use-after-free of a `YGNodeRef` is unchecked UB in Yoga's C ABI**
  (`#1818`, bindings-level, but the underlying C API gives embedders no
  guard). Barre's arena-based handles should decide this deliberately.
- **Point scale factor / pixel-grid rounding is a deliberate "no 1px gaps"
  tradeoff, not a bug to eliminate.** Per the `#1574` thread (citing the
  original `#490`/`aa5b296` fix), Yoga computes a node's rounded dimension
  as the difference between two independently-rounded absolute edges
  specifically so that adjacent children's rounded edges always line up
  with no 1px gap — at the cost of the accumulation artifacts complained
  about in `#901`/`#749`/`#824`. Apple's own layout rounding reportedly
  takes the opposite tradeoff (allowing occasional 1px overlaps). Barre
  should pick one tradeoff deliberately and document it, not "fix" the
  gap-avoidance behavior as if it were accidental.

---

## Known intentional deviations from CSS (barre must keep)

- **`UseWebDefaults(false)` (Yoga's own default) sets `flexDirection:
  column`** instead of CSS's `row`, and **`flexShrink: 0`** instead of CSS's
  `1` (`#835`, `#801`, long-documented once these issues were filed).
- **`flex: N` shorthand sets `flexShrink: N`** (tracking `flexGrow`)
  rather than CSS's fixed `flexShrink: 1` regardless of the shorthand value,
  and under `!UseWebDefaults`, **`flexBasis` from `flex: N` resolves to
  `0%`**, not `auto`/`0` per CSS (`#1323`, `#1787`). A still-open issue
  (`#1523`, not detailed above — title: "Flex shorthand in UseWebDefaults
  should set basis to 0 instead of auto") suggests even the `UseWebDefaults`
  path doesn't fully match CSS here; worth a closer look if/when barre
  implements `UseWebDefaults`.
- **`AbsolutePositioningIncorrect` errata**: existing code may rely on
  absolutely-positioned children being laid out from the parent's *padding*
  edge (old, technically-wrong behavior) rather than its *content* edge
  (correct per CSS). Both behaviors must be selectable via errata (`#1436`).
- **The "running vs. original free-space totals" errata from `#2006`**:
  by default, new configs use the corrected (original-totals) computation;
  an errata bit reproduces the old (running-totals) computation for
  existing layouts. Confirmed present in the pinned source via a direct
  code comment citing the issue.
- **Negative `gap` values are rejected**, matching the CSS Box Alignment
  spec's explicit prohibition, even though some embedders have asked for
  it as an escape hatch (`#1822`). Keep rejecting.
- **Explicit `width`/`height` always lose to `aspectRatio` when both are
  set** — a deliberate, documented design choice ("when implementing aspect
  ratio we made the decision that it should always be respected," per a
  maintainer comment on `#549`), not a bug, even though it surprises users
  coming from CSS's `aspect-ratio` (which is more of a fallback when one
  dimension is auto). Aspect ratio's divergence from the *newer* CSS
  `aspect-ratio` property generally is tracked as a real gap (`#1295`,
  `#1418`) but this specific precedence rule is intentional.
- **Yoga predates and does not match the standardized CSS `aspect-ratio`
  property's interaction with `flex` items** (`#1418`) — not clearly a "bug
  to fix" so much as "a different, older feature that happens to share a
  name"; barre should decide whether to match legacy Yoga behavior (for
  drop-in compatibility) or the newer CSS semantics (for "more accurate"),
  and `#1295`'s fixtures are the yardstick either way.
