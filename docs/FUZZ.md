# Relayout == fresh fuzzer (`./fuzz`, `fuzzer/`)

barre's bar is that relaying out a tree gives the same result as laying it
out fresh (docs/PLAN.md). `./fuzz` checks that on random trees and minimises
what it finds. It is a tool, not a test: nothing gates on it. Minimised cases
are input for the architect, who decides what becomes a conformance test.

## What it checks

A case is a tree description, a sequence of root sizes, a resize mode, a
point scale factor and errata (`YGErrataAll` by default). The tree is laid
out at each size in turn; after every step a fresh tree is built from the
same description, laid out directly at that size, and compared. Any
left/top/width/height differing by more than 1e-3, anywhere in the tree, is a
failure. Modes:

- 0: the root pinned through min/max width/height, then
  `YGNodeCalculateLayout(w, h)` (what React Native does);
- 1: `YGNodeCalculateLayout(w, h)` only;
- 2: root style width, `YGNodeCalculateLayout(undefined, undefined)` (height
  unused).

Every case runs at point scale 0 and at a nonzero scale (`--scale`, default
1). The failure's *class* is which of the two fail: `scale1-only`,
`scale0-only` or `both`. Leaves get a text-like measure function (width
`min(available, len)`, 20 per line). A NaN or infinite value in a fresh
layout is reported separately (`nonfinite`): it is not a relayout difference.

Everything goes through the C ABI: `fuzzer/fuzz.cpp` links barre-ffi's
staticlib, or real Yoga with `--yoga`, like `./conf`.

## Running it

```
./fuzz run --seeds 10000 --jobs 7                 # seeds 1..10000, modes 0 1 2
./fuzz run --seeds 5000-6000 --minimise --save DIR  # also minimise and dedupe
./fuzz show --seed 6                               # the case a seed generates
./fuzz check --seed 6 --mode 0                     # or: ./fuzz check FILE...
./fuzz min --seed 6 --mode 0 --out x.case          # minimise one failure
./fuzz snippet x.case [--run]                      # standalone C++ repro
./fuzz compare a.json b.json                       # two `run --json` outputs
```

Worktrees need `BARRE_VENDOR=<checkout>/vendor`. Engine options go before
the command: `--yoga`, `--lib PATH`, `--engine-root DIR`, `--debug` (barre-ffi
without `--release`), and `--slots N`, a diagnostic build of barre with
`MAX_CACHED_MEASUREMENTS = N` (at most 32) from a patched copy under
`build/fuzz/` (the checkout is not touched). Builds and binaries live in
`build/fuzz/`.

`run` forks one child per seed, `--jobs` at a time, each with a timeout
(`--timeout`, 10 s, or 300 s with `--minimise`). A child that times out is
killed and reported with where it was (mode, size step, phase) and its CPU
time, so a spin (CPU-bound) and a block (sleeping) look different; a child
that dies is reported as a crash. Results go to stdout, one line per failing
(seed, mode), then a summary; progress goes to stderr. `--json FILE` writes
one record per failure. Default knobs: 10000 seeds take about 5 s at 7 jobs,
minimising included.

Generator knobs (the defaults reproduce the scratch fuzzer's trees, so its
seed numbers carry over): `--depth 4`, `--kids 3` (0..K children),
`--root-kids 3`, `--measure 50` (percent of leaves with a measure function),
`--siblings N` (half the containers get N extra measure-function leaves),
`--steps N` (N random sizes instead of the default 7), `--wide` (also
justify/alignSelf/alignContent/absolute/minWidth/maxHeight/gap).

## The minimiser

In-process delta debugging over the description, repeated until nothing
changes: drop subtrees (largest first), replace a node by its children, drop
earlier size steps, unset style fields, simplify values (percent to points,
smaller and rounder numbers), shrink sizes (whole hundreds, at least 100).
Every candidate is re-run and kept only if the *same failure* still occurs:
the same mode and class, the same node (tracked by identity through the
reductions) differs in the same field with the same sign, and it first
fails at the last size step. `--loose` drops the same-node condition: smaller
results, but they can drift to a simpler mechanism of the same class. The
result is printed after a round trip through the text format and re-checked.

`--pass-on EXE` (or the runner's `--yoga-passes` and `--slots-pass N`) adds a
second oracle: another build of the fuzzer must pass every candidate. That
keeps a barre-only failure from drifting into one Yoga shares, or a
slot-dependent one into one that more slots don't fix. `--primary 0|S` picks
the scale a `both` case is minimised at. `min --nonfinite --at-scale S`
minimises a NaN/infinite fresh layout instead.

## Reading a repro

```
# class scale1-only (over scales 0 and 1)
# minimised from seed 6 mode 0, knobs default: 37 -> 7 nodes, 7 -> 2 sizes, 466 checks
# at scale 1, last size: /1/0/0/0 height: relayout 1 fresh 0
mode 0
scale 1
errata all
sizes 200x200 100x100
root
  node shrink=1                                             # /0
    node ar=1                                               # /0/0
  node shrink=1 w=1                                         # /1
    node                                                    # /1/0
      node h=1%                                             # /1/0/0
        node text=1                                         # /1/0/0/0
```

Two-space indentation is nesting; paths count children from 0. Keys: `dir`,
`flex`, `grow`, `shrink`, `basis`, `w`, `h`, `minw`, `minh`, `maxw`, `maxh`,
`pad`/`mar` (all edges), `ovf`, `ai`, `as`, `jc`, `ac`, `wrap`, `ar`, `gap`
(all gutters), `pos`, and `text=N` (a measure function, text of length N).
Lengths take `%`; enums use CSS names. Lay the tree out at 200x200, then at
100x100, and compare with a fresh tree at 100x100. `./fuzz snippet FILE`
prints the same as a self-contained C++ program against `Yoga.h`; `--run`
builds and runs it (exit 1 means relayout != fresh).

## Known failure classes

At `stack` 8d12c13, default knobs, seeds 1..10000 (30000 (seed, mode) pairs):
barre fails 2358 (7.9%); Yoga fails 19315 (64%) of the same pairs. (After
branch `fix-barre-only-relayout`: 2290; 70 pairs fixed, 2 new, both shared
with Yoga.) By class, with the size of the first difference:

| class | pairs | difference | Yoga fails too |
|---|---|---|---|
| `scale1-only` | 551 | always exactly 1 pixel | 549 |
| `both` | 1689 | 1 px: 1345, more: 330, less: 14 | 1668 |
| `scale0-only` | 118 | always under 1 | 118 |

Mechanisms behind them, from the minimised cases in `fuzzer/cases/`
(`./fuzz check fuzzer/cases/*.case`; `--yoga` to compare):

- **Margin in compatible-measurement reuse** (`margin-reuse-1px`,
  `margin-reuse-subpixel`; Yoga too). `can_use_cached_measurement` passes
  `available_width - margin_row` but `last_available_width` with the margin
  still in, to `new_size_is_stricter_and_still_valid`. A leaf with margin 1
  measured at 400 (398 inner) is "stricter" at 401, so 398 is reused where a
  fresh layout gets 399. Most 1 px `both` failures and, through fractional
  sizes that rounding hides, the `scale0-only` class. The 3-node tree in
  `margin-reuse-1px` (one per mode) is what 392 of the 2358 failing pairs
  minimise to.
- **Rounding skipped on a root cache hit** (`rounding-root-cache-hit`; Yoga
  too). `calculate_layout` rounds only when `calculate_layout_internal`
  returns true, but a cache hit still resets the root's dimensions to the
  unrounded measured size: the same width twice gives a root height of
  33.33 instead of 33.
- **Rounding of cached subtrees** (`rounding-cached-subtree`, seed 6; Yoga
  too). The rest of `scale1-only`: 451 of its 542 minimised cases have an
  `aspectRatio`, 468 a percentage.
- **Runaway fresh layouts** (`runaway-fresh`; Yoga too): the fresh layout is
  the wrong one (20971524 tall); 13 failures have a value above 1e5.
- **barre-only** (Yoga passes at that scale): 23 pairs at `stack` 499417a,
  14 after branch `fix-barre-only-relayout` (35 → 22 with `--wide`, 21 → 14
  with `--steps 20`, 51 → 31 with `--siblings 10 --depth 6 --kids 3 --steps
  20 --wide`). Traced and fixed there, each with a test in
  `conformance/tests/YGRelayoutFreshFuzzTest.cpp`:
  - `barre-only-basis-mode2`: a container hit an earlier layout's cache
    entry, then hit it again in the same layout. The entry kept its old
    generation, so the second hit was taken for an earlier-layout hit again
    and replayed the flex-basis anchor a second time, after a run had
    rewritten the bases since. Fix: restamp the entry on the first hit.
  - `slots-barre-only`: T-0006 counted every call with a `MaxContent` main
    axis as an anchor, but a caller can pass a definite size with that mode,
    and then `compute_flex_basis_for_child`'s resolved branch keeps the
    bases. Fix: an anchor needs an undefined available main size. (This
    fails with 8 cache slots only by accident of which entries survive.)
  - `barre-only-baseline`: the yoga#1932 fix lays a baseline child out
    during its owner's *measure* pass; an ancestor's layout pass answered
    from an earlier layout's cache then left the subtree as that measure
    left it. Fix: such a baseline layout invalidates earlier layouts'
    `cached_layout` entries up the owner chain.
  - `barre-only-basis-mode0`: the `singleFlexChild` zero basis written by a
    `StretchFit`-main call answered from an earlier layout's cache (a
    residual T-0006 had listed). Fix: make that write at the hit, or after
    the pending anchor replay.
  - Found while fixing the second: a fixed-size measurement (the
    `measure_node_with_fixed_size` shortcut) computes no basis, so it must
    not be a node's anchor even as its first call (seed 5580; guarded by
    `fixed_size_measurement_is_not_an_anchor`).

  What is left (default knobs, 14 pairs), all traced:
  - **Superseded anchor** (8 pairs: seeds 259, 2422, 5766, 6496, 6723, 7046,
    7441, 8266, all mode 2; plus 9620 with the `--siblings` knobs, which the
    second fix above exposed). Two anchor calls of a container are answered
    from an earlier layout's cache before its next run; only the later one
    is replayed. The earlier one made the children's first calls of the
    layout in a fresh layout (their own anchors), so a grandchild keeps a
    resolved basis where the fresh layout measured its content. Failing
    test: `YGRelayoutFreshResidualTest.superseded_anchor_not_replayed`. It
    needs a design decision: replaying the superseded anchor at the hit
    (tried: fixes 38 more pairs at default knobs, barre-only 14 → 6, `./conf`
    green, no new failures) breaks T-0006's guarantee that replays run only
    right before a real run (it can clone children and do work Yoga skips);
    keeping both anchors pending costs another 28-byte `FlexBasisAnchor` per
    node.
  - **Skipped non-anchor call** (seed 254): a call of a container with a
    definite main size, answered from an earlier layout's cache, made anchor
    calls into its children in the fresh layout; nothing replays them. This
    is T-0006's "history more than one level deep". Failing test:
    `YGRelayoutFreshResidualTest.skipped_non_anchor_call_made_child_anchors`.
  - Margin in compatible-measurement reuse (seed 7636, 3 pairs): the
    Yoga-shared mechanism below; barre-only only because the leaf's cache
    history differs from Yoga's.
  - Rounding of a cached subtree (seed 4337 = `barre-only-scale1`, 2 pairs):
    the Yoga-shared mechanism below; barre-only because barre's *fresh*
    layout at the first size differs from Yoga's (with the `minh=90` removed
    they agree, so presumably the yoga#2035 divergence), which moves the
    cached subtree in barre and not in Yoga.

  Also new at default knobs: seed 2465 (modes 0, 1), Yoga fails it too; it
  is margin-reuse, reached through a different replay history.
- **Cache-slot occupancy.** The measurement cache is a ring of 8 entries and
  the lookup scans only `[0, next_index)`, so after a wrap a relayout sees a
  different set of entries than a fresh layout. Against `--slots 32`: 7 pairs
  fail with 8 slots only, 120 with 32 only (evicting stale entries hides
  reuse failures more often than it causes them). Hunted on purpose with 5000
  seeds per setting:

  | knobs | barre fails | 8-slot-only | barre-only |
  |---|---|---|---|
  | `--steps 20` | 2029 | 30 | 11 |
  | `--measure 100 --depth 5 --kids 4 --steps 12` | 5937 | 78 | 8 |
  | `--siblings 6 --measure 100 --steps 12` | 1649 | 16 | 2 |
  | `--siblings 10 --depth 6 --kids 3 --steps 20 --wide` | 4511 | 97 | 19 |

  Minimised with `--slots-pass 32`, they need many sizes (10 of 20) and are
  mostly the margin-reuse mechanism (`slots-margin-reuse`, 3 nodes, Yoga
  too); 2 of the first set's 30 and 13 of the last set's 30 are barre-only
  (`slots-barre-only`). Traced, `slots-barre-only` is not a slot mechanism:
  it is the `MaxContent`-anchor rule (fixed, see barre-only below); the slot
  count only decides which earlier-layout entries survive to be hit. The
  `[0, next_index)` scan after a wrap is Yoga's lookup, ported as is. With
  `--steps 20` and 10000 seeds, 8-slot-only pairs go 64 → 56 and
  32-slot-only 298 → 280 with that branch.
- **NaN/infinite fresh layouts** (barre-only, `--wide`): a column with only an
  absolute child and `justifyContent: space-around` gets height inf
  (`nonfinite-space-around`; Yoga: 1), and a row with `gap` gets an inf width
  (`nonfinite-gap`). 28 runs in 30000 pairs × 2 scales, 10 of them barre-only.

Dedupe: exact minimised text merges 2358 failures into 1120 repros (1073
shapes, structure plus set fields without values), so treat minimised cases as
examples of a mechanism, not a count of distinct bugs.

## The "hang" (STATUS.md item 3)

There was none. The reported process, `./fuzz-main 2777 1 1` at 0% CPU in
state `S` for over an hour, came from the scratch `minsearch.py`, which ran
`./fuzz-main N 1 1` once per mode (and `./fuzz-main N 0 1` for the modes that
failed) for every N from 1 to 6000, each call fuzzing all seeds 1..N: quadratic. The `ps -p` that showed
0% / `S` / 61 minutes was the Python driver, waiting in `subprocess.run`; the
`fuzz-main 2777` child beside it had started that minute and had used 0.66 s
of CPU. At about 0.3 ms per seed the loop needs roughly an hour to reach
N = 2777, which is when it was looked at. Today `./fuzz-main 2777 1 1`
finishes in 0.9 s writing 41 KB, at `stack` and at 0323406, 7784e1d and
371c971 (the revisions around the report), and `./fuzz run --seeds 10000` has
no timeouts or crashes.

A full stdout pipe does look like that: the scratch fuzzer with 6000 seeds
(91 KB) into a reader that never reads (`| sleep`) sits in state `S` at 0%
CPU, and `sample` shows it in `printf` → `__write_nocancel`. 41 KB fits in a
macOS pipe (64 KB), so that was not it either. `./fuzz run` reports a
deliberate spin and a deliberate block differently (`--inject-hang SEED:spin`
or `SEED:sleep` tests the runner).

## Limitations

- One measure function (text-like and monotone). A measure function that is
  not consistent across constraints would find a different class.
- The minimiser keeps the class, node, field and sign, not the mechanism; a
  minimised case can differ from its seed's case by more than size. Check a
  minimised case with `--yoga` and `--slots 32` before reading a mechanism
  into it.
- `--slots` patches `MAX_CACHED_MEASUREMENTS` by regex in a copy; it fails
  loudly if the constant moves. `Default` limits it to 32.
