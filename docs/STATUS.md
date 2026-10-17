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
   - **Phase B (social-app) is stopped before its builds:** worktree
     `/Users/samuel/Documents/Programming/bluesky-social/social-app-barre`,
     branch `samuel/barre-layout-engine`, stacked on Samuel's #12057 (RN 0.88
     / Expo 58) and #12024 (Android source builds). It has uncommitted work.
     The handover (what exists, what is unverified, gotchas, next steps) is
     "Phase B" in §0 of `docs/RN-SHIM.md`. The builds wait for 3:00am on
     2026-10-18, when another session's experiment on `bsky-o2` ends; then
     build once, run both arms on the `iPhone 17 Pro (barre)` simulator, and
     give the argent comparison to a Sonnet at medium effort.
   - **Priority:** correctness over speed. On-app measurements (below) show
     Yoga's own arithmetic is a small slice of a scrolling session.
2. **Perf toward 0.5× Yoga's time** (`docs/BENCH.md` has the ranked ideas and
   calls-per-node findings):
   - The rest of the rotate regression (T-0006's replays).
   - Fewer edge lookups.
   - A hot/cold split of node data, and a compact `Style`, which also
     shrinks the RN blob.
   - SIMD looks marginal.
   - Branches `perf-replay-skip` and `scratch-noreplay` are unlanded
     experiments from the stopped perf agent. Check them before redoing that
     work.
3. **The relayout == fresh fuzzer:** handoff in `/tmp/barre-fuzz-handoff/`.
   It finds relayout ≠ fresh in about 6% of random trees after T-0006 and
   T-0010. Turn it into a tool in the repo with a test-case minimiser. Known
   causes:
   - pixel-grid rounding on cached subtrees (seed 6, scale 1);
   - cache-slot occupancy (not yet isolated);
   - a run that hung at 0% CPU (probably a full stdout pipe; check).
4. **Small things:**
   - Write a verification convention for perf tickets into
     `docs/HARNESS.md`: a numeric gate check, since the verifier judges
     correctness, not targets.
   - `docs/PLAN.md` still says "Locally only for now".
   - Once no agent commits to `main`, switch the main checkout to `stack`
     (kahvi's setup).

## Open questions for Samuel

- **LICENSE:** there's no LICENSE file; `Cargo.toml` says MIT. Who should be
  the copyright holder?
- **kahvi:** port back two stacker bugs found here, missing `pipefail` in CI
  and `orch run` exiting before closing a red PR?

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
