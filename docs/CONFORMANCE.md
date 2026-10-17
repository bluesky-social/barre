# Conformance runner (`./conf`, `harness/conf`)

Yoga's own gtest suite, compiled unmodified, is the measure of barre's
correctness (docs/PLAN.md, "The oracle"). `./conf` builds it against barre's C
ABI staticlib (`crates/barre-ffi`) or against real Yoga (`--yoga`) and runs
every test in its own process.

```
./conf [FILTER...] [options]
```

## Test IDs

`<file>/<Suite>.<Name>`, where `<file>` is the source path relative to the test
directory without `.cpp`:

| source                                  | ID                                                   |
|-----------------------------------------|------------------------------------------------------|
| `vendor/yoga/tests/YGMeasureTest.cpp`   | `YGMeasureTest/YogaTest.dont_measure_...`            |
| `vendor/yoga/tests/generated/X.cpp`     | `generated/X/YogaTest.align_content_flex_start_...`  |
| `conformance/tests/X.cpp` (barre's own) | `barre/X/Suite.name`                                 |

Generated and hand-written files share some basenames (`YGAspectRatioTest`,
`YGFlexBasisFitContentTest`), hence the `generated/` prefix. Parameterized
tests keep gtest's names, e.g.
`YGFlexBasisFitContentTest/YogaTest/YGFlexBasisFitContentTest.scroll_avoids_remeasure/0`.

A FILTER is a prefix of test IDs: `generated/`, `generated/YGFlexTest`,
`YGMeasureTest/YogaTest.measure_once`, or a full ID. No filter = everything.

## Options

Engine:

- (default) build `cargo build -p barre-ffi` in `--engine-root` (default: this
  checkout) and link against its staticlib (found from cargo's JSON output, so
  `CARGO_TARGET_DIR` is respected).
- `--release`: build barre-ffi with `--release`.
- `--engine-root DIR`: cargo workspace to build barre-ffi from (e.g. a worker
  slot `work/slots/slot-N` or `work/merge`). Only the engine comes from there:
  test sources, `harness/conf`, `conformance/` and `vendor/` always come from
  the checkout `./conf` itself lives in (the trusted one when the gate runs it).
- `--target-dir DIR`: `CARGO_TARGET_DIR` for that build (separate warm caches
  per slot).
- `--lib PATH`: link this prebuilt staticlib, don't run cargo.
- `--yoga`: link against real Yoga built from `vendor/yoga/yoga/**/*.cpp`
  (same as `--engine yoga`).
- `--engine E`: `barre` (default), `yoga`, or one of the React Native engines
  (milestone 5, docs/RN-SHIM.md), which build the suite against React
  Native's copy of Yoga (`ReactCommon/yoga` of react-native 0.88.0, fetched
  by `rn/fetch-react-native.sh` into `<build-dir>/rn/`, or
  `$BARRE_RN_PRISTINE`) and compile the tests against its headers:
  - `rn-yoga`: RN's Yoga, unmodified (the version-skew baseline);
  - `rn-yoga-patched`: with `rn/patches/yoga-rn-0.88.patch` applied
    (`-DBARRE_RN=1`) and `crates/barre-rn` linked, layout switch off: Yoga
    lays out;
  - `rn-barre`: the same binaries with `BARRE_LAYOUT=1`: barre lays out
    through the host interface over RN's C++ nodes.
  The patched engines' canaries (whose output is kept) must show the switch
  in the requested position, or the run is invalid. `yoga-divergences.txt`
  applies to `rn-yoga*` as to `--yoga`. No ratchet: `--update-expected` and
  `--lib` are refused. RN 0.88's Yoga is older than `vendor/yoga`, so some
  tests are expected to differ (docs/RN-SHIM.md, "Phase A").

Running:

- `--jobs N` (default: CPU count), `--timeout S` per test (default 10),
  `--mem-limit-mb N` per test (default 2048, 0 = none).
- `--canaries N`: number of must-fail canaries (default 4).
- `--no-skip`: also build and run what `conformance/skip.txt` excludes
  (diagnostics only; can't be combined with `--update-expected`).
- `--build-dir DIR`: default `$BARRE_CONF_BUILD_DIR`, else
  `<vendor>/../build/conf`.
- `--list`: build, print the selected test IDs, exit.

Output:

- one summary line on stdout:
  `conf [barre-debug]: 859 tests: 12 pass, 700 crash, 40 fail, 107 skip; 1.6% of 752 in scope pass (...)`,
  followed by the most common `not implemented:` names; progress goes to
  stderr (`--quiet` silences it).
- `--show N`: the first N non-passing tests with their reason and output tail.
- `--by-file`: per-file table (pass/fail/crash/timeout/build-fail/skip).
- `--json F`: `{"<id>": {"status": S, "reason": R}}` for every selected test,
  with S one of `pass`, `fail`, `crash`, `timeout`, `build-fail`, `skip`
  (and, with `--yoga` only, `xfail`/`xpass`: see "Yoga divergences" below).
  R is empty for a pass, and otherwise one line:
  - `not implemented: <name>` whenever the output contains barre's
    `not_implemented!` panic message (`not implemented: <name>`, the rest of
    that line), whatever the status (usually `crash`, since the C ABI aborts);
  - `panic: <message>` for any other Rust panic;
  - `<file>:<line>: <assertion details>` for a gtest assertion failure;
  - `SIGABRT` / `SIGSEGV` / ..., `memory limit exceeded`,
    `timeout after Ns`, `exited with code N before the test finished`;
  - for `build-fail`, the first compiler error or
    `link: N undefined symbols, e.g. _YGFoo, ...`;
  - for `skip`, the reason from `skip.txt`.
- `--summary-json F`: `{variant, valid, invalid_reasons, canaries,
  canaries_ok, total, in_scope, pass, fail, crash, timeout, build_fail, skip,
  pass_rate, not_implemented: {name: count}, canary_results, regressions,
  vanished, new_passes,
  elapsed_s, run_s}`.
- `--baseline F`: compare with an expected-pass list (one ID per line, `#`
  comments). A regression is an ID in F that matches the filters, ran, and did
  not pass (fail/crash/timeout/build-fail); any regression makes the exit code
  1. IDs in F that are now skipped or no longer exist are reported as
  `vanished` (not regressions: skip.txt is a protected path, and upstream
  renames are not a change's fault); they drop out on the next rewrite. New
  passes are passes not in F. (harness/orch/conformance.py computes the same
  from `--json` itself.)
- `--update-expected`: rewrite the baseline (`--baseline F` if given, else
  `conformance/expected-pass.txt`, keeping its leading comment block) with
  this run's passes. With filters, only
  IDs matching them are replaced; others are kept. Refused with `--yoga`,
  `--no-skip`, and on an invalid run. It rewrites even when there are
  regressions (the regressed IDs drop out), so a caller that rejects the run
  must discard the file.

Exit codes: `0` ok; `1` regressions against `--baseline`, or (with `--yoga`)
an `xpass`; `2` error (cargo
build of barre-ffi failed, googletest or the runner's own objects failed to
build, bad arguments, missing vendor checkout); `3` invalid run (a canary
passed, or the symbol check failed). A test file that fails to build is not an
error: its tests are `build-fail`.

### Gate / merge-bot usage

The gate runs the trusted runner from the main checkout and the engine from a
worker slot:

```
./conf generated/YGFlexTest generated/YGFlexWrapTest \
  --engine-root work/slots/slot-3 --target-dir work/targets/slot-3 \
  --json out.json --summary-json summary.json \
  --baseline conformance/expected-pass.txt
```

The merge bot runs the full suite and ratchets:

```
./conf --engine-root work/merge --json out.json --summary-json summary.json \
  --baseline work/merge/conformance/expected-pass.txt --update-expected
```

Workers running `./conf` inside a slot worktree (no submodules checked out) set
`BARRE_VENDOR=<main checkout>/vendor`; the build dir then defaults to the main
checkout's `build/conf`, so googletest and the suite objects are shared.

## How it builds

`harness/conf/conf.py` writes `<build-dir>/build.ninja` and runs ninja
(`-k 0`, parallel, incremental via depfiles):

- **engine-independent, built once and shared by all variants and slots:**
  googletest (`gtest-all.cc`), the runner's gtest main
  (`harness/conf/main.cpp`), `vendor/yoga/tests/util/TestUtil.cpp`, the event
  shim, and one object per test source. All compile against `vendor/yoga`'s
  headers (`-std=c++20 -O0 -DDEBUG -fexceptions -frtti`, like Yoga's Debug
  CMake build). They always come from the vendor dir of the runner (or
  `BARRE_VENDOR`), never from the engine worktree.
- **per engine:** one binary per test source, linked under
  `<build-dir>/bin/<variant>/`: `yoga` (with `libyoga.a`, built from
  `vendor/yoga/yoga/**/*.cpp`), `barre-debug` / `barre-release` (this
  checkout's staticlib), or `lib-<hash of the staticlib path>` (other engine
  roots; the 8 most recently used are kept).

Barre binaries link `harness/conf/event_shim.cpp`, which defines Yoga's
internal `Event::subscribe/reset/publish`: `TestUtil.cpp` references them, and
barre has no C++ event API. Nothing is ever published to it; the tests that
depend on events are skipped.

A file whose compile or link fails gets `build-fail` for all its tests; ninja
keeps going. Test IDs come from `--gtest_list_tests` on the built binary
(cached by binary mtime); for a file that did not build, from the listing
cached by any earlier build of the same source (engine-independent), else
from the `TEST(...)` macros in the source.

A build dir is locked (`flock`) exclusively while building and shared while
running, so concurrent `./conf` runs on one build dir are safe.

Timing on a 14-core M-series Mac: first build from scratch ~6 s (googletest,
suite objects, libyoga); relinking all 54 binaries after an engine change
~2.5 s; a no-op rebuild plus the full ~750 in-scope tests ~2 s.

Requirements: python3, ninja, clang++ (or `$CXX`), nm, cargo.

## How it runs, and why passes can't be faked

- Each test runs as `<binary> --gtest_filter=Suite.Name` in its own process,
  in parallel, with a timeout (process group killed), a memory cap and a
  minimal environment. A signal, an `abort()` (barre's C ABI turns panics into
  aborts), exceeding the memory cap, or exiting before the test finished is
  `crash`; running past the timeout is `timeout`.
- The binaries use the runner's own gtest main, not `gtest_main`. The runner
  passes a random nonce in `CONF_NONCE`, which main removes from the
  environment before any test runs, and main prints
  `CONF-RESULT <nonce> ran=N passed=N failed=N skipped=N` after the tests. A
  test passes only if that line, with this process's nonce, says exactly one
  test ran and passed, and the process exited 0.
- The memory cap is enforced in main too: `RLIMIT_AS` on Linux; macOS has no
  working address-space limit, so a watchdog thread polls the process
  footprint and exits with code 97.
- **Canaries:** every run generates a test file in a fresh temp dir, with a
  random suite name and random test names. Each canary builds a random small
  tree through the C API, lays it out and asserts a deliberately wrong value
  (the root's size off by a random amount, or a child position far outside the
  tree). They are compiled, linked against the same engine and run through the
  same code path as the suite. If any canary passes, or they fail to build,
  the run is invalid (exit 3) and the baseline is not written.
- **Symbol check (barre only):** the staticlib must define
  `YGNodeCalculateLayout`/`YGNodeNew`/`YGConfigNew`, contain Rust code, and
  contain none of Yoga's layout code (`facebook::yoga::calculateLayout`,
  `yoga::Node` constructors); every linked test binary and the canary binary
  are checked the same way (`nm`, cached by mtime). Failure = invalid run.

## Scope: `conformance/skip.txt`

One entry per line: `<ID prefix>  <reason>`. A prefix ending in `/` excludes a
whole file (not built). Skipped tests are reported as `skip` under both barre
and `--yoga`, never as pass, and don't count in the pass rate.

Policy: a test is in scope iff it exercises the public C ABI
(`vendor/yoga/yoga/Yoga.h`). Out of scope:

- files that need non-inline Yoga internals to link (`CacheTest`,
  `EventPublisherTest`, `StyleTest`, `YGConfigTest`, `YGDirtiedTest`,
  `YGNodeCallbackTest`, `YGPersistenceTest`, `YGPersistentNodeCloningTest`),
  found with `nm -u <object> | c++filt | grep facebook::yoga`;
- files that link but would read barre's handles as `yoga::Node` or observe
  Yoga's event stream (`YGLayoutableChildrenTest`, `EventsTest`);
- header-only tests of Yoga's C++ internals that call no `YG*` function and
  would pass vacuously (`FloatOptionalTest`, `GridStyleTest`, `OrdinalsTest`,
  `SmallValueBufferTest`, `StyleValuePoolTest`, `YGValueTest`).

That is 16 files, 107 tests; 752 tests in 54 files are in scope, plus
barre's own tests in `conformance/tests/`.

What these files test through the C API is ported to `conformance/tests/`
under the same file and test names (`barre/YGConfigTest/...`,
`barre/YGPersistenceTest/...`, ...); each port names the Yoga file and commit
it came from, and says where it had to rewrite internals-only setup.

## The ratchet: `conformance/expected-pass.txt`

Sorted, one ID per line: every test that passes on main. A change may never
lose one (`--baseline`), and the merge bot rewrites it
(`--update-expected`) after a full run.

## Yoga divergences: `conformance/yoga-divergences.txt`

Some of barre's own tests assert behaviour where Yoga is wrong (Yoga bugs
barre fixes, e.g. yoga#1552, yoga#2019), so they fail on real Yoga by design.
They are documented in `conformance/divergences.md` and listed, one exact test
ID per line with a reason (`<test ID>  <reason>`, as in `skip.txt`), in
`conformance/yoga-divergences.txt`. Only `--yoga` reads it:

- a listed test that fails, crashes or times out is reported as `xfail` and
  does not count against the pass rate (`... 100.0% of 798 + 11 xfail in
  scope pass`);
- a listed test that passes is `xpass`: printed as `UNEXPECTED PASS on Yoga`
  and the exit code is 1 (the test no longer demonstrates the bug, or Yoga
  fixed it);
- a listed ID that matches the filters but did not run is warned about.

Against barre the file has no effect: these are ordinary tests (and in the
ratchet once barre passes them).

## Validating the runner

`./conf --yoga` must pass everything in scope except the documented Yoga
divergences, which must all be `xfail` (at the pin: 798 pass + 11 xfail, exit
0; with `--no-skip`, also the 107 skipped tests pass). Re-run it after
changing `harness/conf`.
