# barre

Yoga (the flexbox layout engine behind React Native) reimplemented in Rust,
built mostly by a swarm of cheap agents under a harness. Plan: docs/PLAN.md.
Code layout and conventions: docs/API.md. Operating the harness:
docs/HARNESS.md.

- **Yoga's test suite is the only measure of correctness.** `./conf [PREFIX]`
  compiles Yoga's unmodified gtest files against barre's C ABI and runs them,
  e.g. `./conf generated/YGFlexWrapTest` or
  `./conf YGMeasureTest/YogaTest.measure_once` (docs/CONFORMANCE.md).
- **Yoga's source is the spec.** `vendor/yoga/yoga/` is the reference;
  `./spec <name>` prints a Yoga function or type (camelCase or snake_case),
  `./spec --find <text>` searches names.
- **Port structure, not bugs.** Functions keep Yoga's names in snake_case
  (`compute_flex_basis_for_child`), in files mirroring Yoga's
  (`algorithm/CalculateLayout.cpp` → `crates/barre/src/algorithm/calculate_layout.rs`),
  each with a doc comment naming the Yoga function. Keep Yoga's comments,
  control flow and float arithmetic (same operations, same order, `f32`
  vs `f64` exactly as Yoga). Where barre deliberately differs, say so and why.
- **The algorithm only sees `LayoutNode`.** Everything in `algorithm/` is
  generic over the node-access trait (docs/API.md); never reach into
  `barre::Node` storage from there.
- **Never fake progress.** No special-casing tests or inputs, no hardcoded
  results, no swallowed errors, no guessed behaviour. Anything unimplemented
  calls `not_implemented!("YogaFunctionName")`, which aborts and is counted as
  a failure.
- **No Rust unit tests for layout behaviour.** The suite covers it. The only
  Rust tests are architect-owned checks of internals the suite cannot reach.
- **Protected, architect-only paths:** `vendor/`, `harness/`, `conformance/`,
  `tickets/`, `docs/`, Cargo manifests, `rust-toolchain.toml`, this file.
  Worker tickets list the only files they may change.
- `cargo clippy --workspace --all-targets -- -D warnings` must be clean.
