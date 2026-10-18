# Status and handoff

Where barre stands and what was in flight when the first architect session
paused (2026-10-18). A new architect session starts here, then reads
`CLAUDE.md`, `docs/PLAN.md` (including its decisions log) and the doc for
whatever it picks up.

## Where things are

- **Correctness:** 847/847 in scope. That's all 752 of Yoga's C-API tests
  plus 95 of barre's own: ports of the C-API parts of the Yoga test files
  that can't link, and regression tests for Yoga bugs. barre fixes yoga#1552,
  #2019, #2035, #2007, #1932, #1626, #1003 and #1784, and passes #2038
  (`conformance/divergences.md`). The bar is that relaying out a tree gives
  the same result as laying it out fresh.
- **Speed:** about 0.75× Yoga's time (geomean, `bench/`), and about 0.6× on
  RN-feed fresh layouts. `rn/feed50/rotate` is still about 1.1× because of
  T-0006's replays (`docs/BENCH.md`). The README's benchmark block comes from
  CI on GitHub's virtual M1, so it reads higher than local runs.
- **The swarm:** 10 tickets merged for about $17 in total. The pool is empty
  apart from T-0011 (the rotate regression), which is parked at
  `tier = "architect"` because workers can't run `bench/`.
- **The repo:** github.com/bluesky-social/barre, public. Work lands as stacked
  PRs with CI (`docs/STACKS.md`, `docs/HARNESS.md`). Layers #7, #8 and #10
  (perf) are merged into main.

## How to work

- **Landing:** never commit to or push `main`, and never commit on `stack`.
  Commit on your own branch (a worktree is fine, based on `stack`), then run
  `./orch land BRANCH` from the main checkout. If no `./orch run` is running,
  start one: `nohup ./orch run >> work/orch-run.log 2>&1 &`. Samuel merges
  green layers from the bottom.
- **Usage limits:** Opus and large Sonnet work stay within the plan's usage
  windows (`./orch usage`); stop above ~85% of five_hour. Haiku grinding
  never pauses, since it spills into API billing by design. The harness
  enforces this per tier.
- **Delegation:** use Opus subagents for code. A Sonnet subagent can babysit
  `./orch run`. Argent work (driving simulators and devices, profiling) goes
  to a **Sonnet subagent at medium effort**.
- **social-app:** branches start with `samuel/`, and you ask Samuel about a
  Linear ticket before creating one. Work in a worktree, never his main
  checkout. Don't push or open PRs unless he asks. In stacks, use `gh stack`
  but never `gh stack sync`.
- **The iPhone 14** (`00008110-0006206E0C7A401E`) is Samuel's test phone.
  When unattended device work finishes, lock it. Argent can't press power on
  a physical iPhone; Samuel approved setting Auto-Lock to 30 s for that.

## In flight

1. **RN shim (milestone 5).**
   - **Phase A (host-tested) is landed** as stack layer #12 (branch
     `rn-shim`, CI green, waiting for Samuel's merge after #11). It contains
     `crates/barre-rn`, the RN 0.88 Yoga patch, conf engines
     `rn-yoga`/`rn-yoga-patched`/`rn-barre`, `bench --rn`, and the
     podspec/CMake linking. Status and numbers are in §0 of `docs/RN-SHIM.md`.
   - **Phase B (social-app) builds and launches on iOS:** worktree
     `/Users/samuel/Documents/Programming/bluesky-social/social-app-barre`,
     branch `samuel/barre-layout-engine`, stacked on Samuel's #12057 (RN 0.88
     / Expo 58) and #12024 (Android source builds). It has uncommitted work.
     One Release binary runs both arms on the `iPhone 17 Pro (barre)`
     simulator; lldb shows the barre arm lays out only through barre. Only the
     logged-out splash has been rendered. Details, the lockfile problem (the
     diff is NOT just the `patch_hash`; it needs redoing before committing)
     and next steps are in "Phase B" in §0 of `docs/RN-SHIM.md`. The argent
     comparison (Sonnet, medium effort) is blocked on two things: which
     account to log in on that simulator, and porting the o2 timing
     instrumentation into the worktree.
   - **Priority:** correctness over speed. On-app measurements (below) show
     Yoga's own arithmetic is a small slice of a scrolling session.
2. **Perf toward 0.5× Yoga's time** (`docs/BENCH.md` has the ranked ideas and
   calls-per-node findings):
   - The rest of the rotate regression (T-0006's replays).
   - **Claw back the cost of the barre-only relayout fixes** (stack layer
     "fix-barre-only-relayout"). Measured 2026-10-18 with `bench/run --rev
     REV rn/` (REV = the stack top before vs after; two runs each, within
     about 1% of each other): rn/ geomean 0.52× → 0.55×, and on `rn/feed50`
     `clean` 12 → 16 ns, `text_same_size` 10.6 → 11.8 µs (+11%), `rotate`
     233 → 247 µs (0.91× → 0.97×), `text_change` +7%, `insert_remove` +6%;
     fresh layouts +1–3%. Per commit, no single culprit: 0a99eec costs
     nothing, then restamp on hit (074d3df) +2 ns on `clean` and about +5 µs
     on `rotate`, baseline invalidation (2135442) about +4 µs, the
     single-flex-child write (1fb7b09) about +7 µs, the fixed-size check
     (b5fab9e) +2 ns on `clean`. It was accepted because correctness comes
     first. Look for the same checks done on colder paths, or merged into
     the existing lookups.
   - Fewer edge lookups.
   - A hot/cold split of node data, and a compact `Style`, which also
     shrinks the RN blob.
   - SIMD looks marginal.
   - Branches `perf-replay-skip` and `scratch-noreplay` are unlanded
     experiments from the stopped perf agent. Check them before redoing that
     work.
3. **The relayout == fresh fuzzer** is now a repo tool: `./fuzz`, source in
   `fuzzer/`, how to use it and the failure classes in `docs/FUZZ.md`, 13
   minimised repros in `fuzzer/cases/` (`./fuzz check fuzzer/cases/X.case`;
   `./fuzz --yoga check ...` to compare with real Yoga). At seeds 1-10000 it
   fails 2358 of 30000 (seed, mode) pairs (7.9%), of which Yoga fails 19315
   (64%) of the same pairs. What is left to do, in order of importance:
   - **Barre-only failures:** 23 → 14 pairs at default knobs with branch
     `fix-barre-only-relayout` (35 → 22 `--wide`). Four mechanisms were
     traced and fixed (an earlier layout's entry hit twice in one layout,
     the `MaxContent`-anchor rule, the yoga#1932 baseline layout during a
     measure pass, the skipped `singleFlexChild` zero basis), each with a
     test in `conformance/tests/YGRelayoutFreshFuzzTest.cpp`. Of the 14 left
     (docs/FUZZ.md, barre-only), 9 are two residuals of T-0006's anchor that
     need a design decision, with failing tests in
     `YGRelayoutFreshResidualTest.cpp`: a superseded anchor that is not
     replayed (8 pairs; replaying it at the hit fixes them but breaks
     "replays only before a real run", keeping both pending costs 28 B per
     node) and a skipped non-anchor call (1 pair). The other 5 are
     Yoga-shared mechanisms (margin reuse, cached-subtree rounding) that a
     different cache history or the yoga#2035 divergence makes barre-only.
   - **Failures shared with Yoga:** margin in compatible-measurement reuse
     (`margin-reuse-*`) and the root cache hit skipping pixel rounding
     (`rounding-root-cache-hit`) were checked against the source;
     `rounding-cached-subtree` was not. Fixing them makes barre differ from
     Yoga, so record each in `conformance/divergences.md` (check whether it
     belongs with yoga#1784's entry).
   - **Non-finite fresh layouts** (`nonfinite-*`): NaN/inf in barre where
     Yoga gives finite values. Not a relayout difference, but a bug.
   - **The "hang" was not a hang** (a quadratic Python driver, not barre or
     a pipe; details in `docs/FUZZ.md`). Nothing to do.
4. **Small things:**
   - Once no agent commits to `main`, switch the main checkout to `stack`
     (kahvi's setup).
   - (Done: the perf-ticket verification convention is in `docs/HARNESS.md`,
     and `docs/PLAN.md` no longer says "Locally only for now".)

## Open questions for Samuel

- **LICENSE:** there's no LICENSE file; `Cargo.toml` says MIT. Who should be
  the copyright holder?
- **kahvi:** port back three stacker bugs found here? Missing `pipefail` in
  CI; `orch run` exiting before it has judged or closed a PR; and a ledger
  race where the orphan sweep marks a layer `closed` while its PR is still
  being opened (the push then records `pr` without resetting the status).

## Context from social-app measurements (2026-10-17/18)

- **iOS Release builds RN's C++ at Xcode's default `-Os`.** Android uses
  `-O2` plus thin LTO. Draft PR bluesky-social/social-app#12061 compiles it at
  `-O2` (default `all`).
- **In-app results** (instrumented `YGNodeCalculateLayout`, an argent flow on
  the simulator, raw data in `/tmp/bsky-o2-runs/`):
  - `-O2` cuts layout's own time by 5–10%, which is within the noise.
  - Text measurement is about 37% of layout.
  - All of layout adds up to about 350 ms over a 73 s scroll session.
  - Individual layouts in a long thread cost 5–6 ms on an iPhone 14.
- **What that means for barre in RN:** its value is mostly correctness, plus
  those heavier one-off layouts. Steady scrolling barely touches it.

`/tmp` paths (worktrees, handoffs, run data) don't survive a reboot. Commit
or land anything that matters.
