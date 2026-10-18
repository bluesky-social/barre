# barre benchmark history

Written by `.github/workflows/bench.yml` on every push to `main` of [bluesky-social/barre](https://github.com/bluesky-social/barre).
barre against Yoga through the same C API on a GitHub-hosted macOS arm64 runner (`bench/run`, docs/BENCH.md).
Shared runners are noisy: compare trends, not single runs.

| run | barre commit | geomean barre/Yoga (lower is faster) | workloads | |
|---|---|--:|--:|---|
| 2026-10-18 08:08 UTC | [`9122b937ed`](https://github.com/bluesky-social/barre/commit/9122b937ed) | 0.702× | 35 | [table](results/20261018-080803-9122b937ed.md) · [json](results/20261018-080803-9122b937ed.json) |
| 2026-10-17 20:08 UTC | [`52d96baf8c`](https://github.com/bluesky-social/barre/commit/52d96baf8c) | 0.662× | 35 | [table](results/20261017-200850-52d96baf8c.md) · [json](results/20261017-200850-52d96baf8c.json) |
| 2026-10-17 18:01 UTC | [`1854783423`](https://github.com/bluesky-social/barre/commit/1854783423) | 0.898× | 35 | [table](results/20261017-180128-1854783423.md) · [json](results/20261017-180128-1854783423.json) |
| 2026-10-17 17:55 UTC | [`300ee03da5`](https://github.com/bluesky-social/barre/commit/300ee03da5) | 0.897× | 35 | [table](results/20261017-175534-300ee03da5.md) · [json](results/20261017-175534-300ee03da5.json) |
| 2026-10-17 17:49 UTC | [`f47ea26fa8`](https://github.com/bluesky-social/barre/commit/f47ea26fa8) | 0.892× | 35 | [table](results/20261017-174927-f47ea26fa8.md) · [json](results/20261017-174927-f47ea26fa8.json) |
| 2026-10-17 17:45 UTC | [`9bbc3b4aa9`](https://github.com/bluesky-social/barre/commit/9bbc3b4aa9) | 0.893× | 35 | [table](results/20261017-174509-9bbc3b4aa9.md) · [json](results/20261017-174509-9bbc3b4aa9.json) |

## Latest run

- barre: `9122b937ed7e` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 14.3/29.6/18.3
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.70×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.32 µs (4.04 µs–10.9 µs) | 2.67 µs (2.56 µs–6.94 µs) | **0.62×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.85 µs (3.68 µs–9.84 µs) | 2.6 µs (2.51 µs–6.84 µs) | **0.68×** | 10 | - | ok |
| `yoga/nested_flex` | 79.9 µs (72.4 µs–194 µs) | 46.7 µs (42.8 µs–117 µs) | **0.58×** | 100 | - | ok |
| `yoga/huge_nested` | 5.53 ms (4.37 ms–9.72 ms) | 6.54 ms (3.54 ms–10.7 ms) | **1.18×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 3.92 ms (3.17 ms–6.34 ms) | 2.84 ms (2.47 ms–4.64 ms) | **0.72×** | 0 | - | ok |
| `capture/feed-android` | 1.1 ms (975 µs–2.08 ms) | 514 µs (460 µs–1.06 ms) | **0.47×** | 57 | - | ok |
| `capture/feed-android.clean` | 33 ns (32 ns–100 ns) | 24 ns (24 ns–77 ns) | **0.74×** | 0 | - | ok |
| `capture/profile-ios` | 201 µs (178 µs–471 µs) | 90.7 µs (82 µs–233 µs) | **0.45×** | 13 | - | ok |
| `capture/profile-ios.clean` | 33 ns (31 ns–100 ns) | 24 ns (23 ns–78 ns) | **0.74×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 84.6 µs (75.5 µs–178 µs) | 40.4 µs (36.2 µs–107 µs) | **0.48×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–66 ns) | 25 ns (24 ns–78 ns) | **0.79×** | 0 | - | ok |
| `capture/chat-mac` | 5.8 ms (5.18 ms–8.86 ms) | 3.27 ms (2.66 ms–6.02 ms) | **0.56×** | 162 | - | ok |
| `capture/chat-mac.clean` | 32 ns (30 ns–51 ns) | 24 ns (24 ns–63 ns) | **0.76×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.7 ms (2.26 ms–5.25 ms) | 1.17 ms (1.03 ms–2.42 ms) | **0.43×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 32 ns (31 ns–102 ns) | 26 ns (24 ns–79 ns) | **0.79×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 55.5 µs (52.6 µs–147 µs) | 32.2 µs (30.2 µs–57.5 µs) | **0.58×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 52.6 µs (49.8 µs–83.5 µs) | 19 µs (16.5 µs–49.6 µs) | **0.36×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 408 µs (375 µs–569 µs) | 407 µs (358 µs–680 µs) | **1.00×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.4 ms (2.12 ms–4.13 ms) | 1.16 ms (978 µs–1.91 ms) | **0.48×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 45.3 µs (40.8 µs–116 µs) | 26.2 µs (24.5 µs–75 µs) | **0.58×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 19.2 µs (18.5 µs–47.5 µs) | 8.67 µs (8.42 µs–11 µs) | **0.45×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 251 µs (233 µs–378 µs) | 106 µs (100 µs–131 µs) | **0.42×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 4.17 ms (3.87 ms–5.49 ms) | 1.56 ms (1.46 ms–2.07 ms) | **0.37×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 50.6 ms (47.8 ms–77.4 ms) | 19.4 ms (17.5 ms–27 ms) | **0.38×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 32 ns (30 ns–40 ns) | 24 ns (23 ns–78 ns) | **0.78×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.52 ms (1.3 ms–2.27 ms) | 794 µs (664 µs–1.27 ms) | **0.52×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 83.2 µs (75.5 µs–164 µs) | 38.4 µs (36 µs–83.6 µs) | **0.46×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 749 µs (670 µs–1.3 ms) | 413 µs (375 µs–697 µs) | **0.55×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 421 µs (373 µs–639 µs) | 216 µs (196 µs–408 µs) | **0.51×** | 200 | 703 | ok |
| `micro/node_new_free` | 62 ns (59 ns–147 ns) | 91 ns (88 ns–210 ns) | **1.47×** | 0 | - | n/a |
| `micro/style_set_x20` | 69 ns (66 ns–222 ns) | 328 ns (298 ns–878 ns) | **4.73×** | 0 | - | n/a |
| `micro/style_get_x20` | 12 ns (11 ns–33 ns) | 19 ns (17 ns–51 ns) | **1.55×** | 0 | - | n/a |
| `micro/insert_remove_child` | 24 ns (22 ns–63 ns) | 37 ns (34 ns–102 ns) | **1.54×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 197 µs (177 µs–468 µs) | 293 µs (256 µs–683 µs) | **1.49×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.8 µs (6.49 µs–14.6 µs) | 10.5 µs (10 µs–24 µs) | **1.54×** | 0 | - | n/a |
