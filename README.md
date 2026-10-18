# barre benchmark history

Written by `.github/workflows/bench.yml` on every push to `main` of [bluesky-social/barre](https://github.com/bluesky-social/barre).
barre against Yoga through the same C API on a GitHub-hosted macOS arm64 runner (`bench/run`, docs/BENCH.md).
Shared runners are noisy: compare trends, not single runs.

| run | barre commit | geomean barre/Yoga (lower is faster) | workloads | faster than Yoga (excl. `micro/`) | |
|---|---|--:|--:|--:|---|
| 2026-10-18 09:09 UTC | [`59535867e5`](https://github.com/bluesky-social/barre/commit/59535867e5) | 0.689× | 35 | 29 of 29 | [table](results/20261018-090938-59535867e5.md) · [json](results/20261018-090938-59535867e5.json) |
| 2026-10-18 08:08 UTC | [`9122b937ed`](https://github.com/bluesky-social/barre/commit/9122b937ed) | 0.702× | 35 | 27 of 29 | [table](results/20261018-080803-9122b937ed.md) · [json](results/20261018-080803-9122b937ed.json) |
| 2026-10-17 20:08 UTC | [`52d96baf8c`](https://github.com/bluesky-social/barre/commit/52d96baf8c) | 0.662× | 35 | 28 of 29 | [table](results/20261017-200850-52d96baf8c.md) · [json](results/20261017-200850-52d96baf8c.json) |
| 2026-10-17 18:01 UTC | [`1854783423`](https://github.com/bluesky-social/barre/commit/1854783423) | 0.898× | 35 | 21 of 29 | [table](results/20261017-180128-1854783423.md) · [json](results/20261017-180128-1854783423.json) |
| 2026-10-17 17:55 UTC | [`300ee03da5`](https://github.com/bluesky-social/barre/commit/300ee03da5) | 0.897× | 35 | 21 of 29 | [table](results/20261017-175534-300ee03da5.md) · [json](results/20261017-175534-300ee03da5.json) |
| 2026-10-17 17:49 UTC | [`f47ea26fa8`](https://github.com/bluesky-social/barre/commit/f47ea26fa8) | 0.892× | 35 | 21 of 29 | [table](results/20261017-174927-f47ea26fa8.md) · [json](results/20261017-174927-f47ea26fa8.json) |
| 2026-10-17 17:45 UTC | [`9bbc3b4aa9`](https://github.com/bluesky-social/barre/commit/9bbc3b4aa9) | 0.893× | 35 | 21 of 29 | [table](results/20261017-174509-9bbc3b4aa9.md) · [json](results/20261017-174509-9bbc3b4aa9.json) |

## Latest run

- barre: `59535867e567` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 6.4/18.0/11.6
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.69×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.05 µs (3.74 µs–4.85 µs) | 2.57 µs (2.43 µs–3.27 µs) | **0.63×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.67 µs (3.46 µs–4.35 µs) | 2.51 µs (2.33 µs–2.96 µs) | **0.68×** | 10 | - | ok |
| `yoga/nested_flex` | 72.5 µs (67.5 µs–86.7 µs) | 42.6 µs (39.8 µs–48.5 µs) | **0.59×** | 100 | - | ok |
| `yoga/huge_nested` | 3.77 ms (3.35 ms–4.95 ms) | 3.22 ms (2.92 ms–6.26 ms) | **0.85×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 2.79 ms (2.47 ms–4.47 ms) | 1.64 ms (1.4 ms–2.48 ms) | **0.59×** | 0 | - | ok |
| `capture/feed-android` | 937 µs (887 µs–1.1 ms) | 433 µs (402 µs–526 µs) | **0.46×** | 57 | - | ok |
| `capture/feed-android.clean` | 32 ns (30 ns–36 ns) | 24 ns (22 ns–28 ns) | **0.74×** | 0 | - | ok |
| `capture/profile-ios` | 176 µs (169 µs–198 µs) | 81.7 µs (76.3 µs–94.7 µs) | **0.46×** | 13 | - | ok |
| `capture/profile-ios.clean` | 30 ns (29 ns–36 ns) | 24 ns (23 ns–31 ns) | **0.79×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 77.3 µs (72.4 µs–94.8 µs) | 36 µs (33.7 µs–41.1 µs) | **0.47×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–38 ns) | 24 ns (22 ns–29 ns) | **0.76×** | 0 | - | ok |
| `capture/chat-mac` | 5.19 ms (4.88 ms–5.67 ms) | 2.59 ms (2.32 ms–3.16 ms) | **0.50×** | 162 | - | ok |
| `capture/chat-mac.clean` | 30 ns (28 ns–35 ns) | 24 ns (23 ns–28 ns) | **0.80×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.32 ms (2.1 ms–2.65 ms) | 1.04 ms (959 µs–1.18 ms) | **0.45×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 30 ns (28 ns–34 ns) | 24 ns (23 ns–28 ns) | **0.77×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 52.5 µs (51 µs–60.2 µs) | 30.8 µs (29.5 µs–36.2 µs) | **0.59×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 49.8 µs (45.8 µs–58 µs) | 17 µs (16.3 µs–20.5 µs) | **0.34×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 363 µs (348 µs–406 µs) | 340 µs (319 µs–376 µs) | **0.94×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.15 ms (1.95 ms–2.36 ms) | 1.01 ms (932 µs–1.16 ms) | **0.47×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 41.5 µs (38.6 µs–49.5 µs) | 24.5 µs (23.2 µs–30 µs) | **0.59×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 18.5 µs (17.6 µs–21.8 µs) | 8.21 µs (7.88 µs–9.12 µs) | **0.44×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 237 µs (222 µs–268 µs) | 101 µs (96.5 µs–117 µs) | **0.43×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 3.74 ms (3.45 ms–4.14 ms) | 1.47 ms (1.35 ms–1.6 ms) | **0.39×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 44.5 ms (42.6 ms–49.1 ms) | 17.4 ms (16.6 ms–18.5 ms) | **0.39×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 30 ns (28 ns–34 ns) | 23 ns (22 ns–25 ns) | **0.77×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.33 ms (1.2 ms–1.61 ms) | 643 µs (599 µs–737 µs) | **0.48×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 73 µs (70.6 µs–80.1 µs) | 35.8 µs (34.2 µs–41.5 µs) | **0.49×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 669 µs (624 µs–740 µs) | 373 µs (360 µs–410 µs) | **0.56×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 354 µs (340 µs–393 µs) | 184 µs (174 µs–211 µs) | **0.52×** | 200 | 703 | ok |
| `micro/node_new_free` | 57 ns (55 ns–62 ns) | 85 ns (81 ns–96 ns) | **1.48×** | 0 | - | n/a |
| `micro/style_set_x20` | 64 ns (61 ns–76 ns) | 288 ns (283 ns–364 ns) | **4.53×** | 0 | - | n/a |
| `micro/style_get_x20` | 11 ns (10 ns–13 ns) | 16 ns (16 ns–19 ns) | **1.54×** | 0 | - | n/a |
| `micro/insert_remove_child` | 22 ns (21 ns–25 ns) | 34 ns (33 ns–40 ns) | **1.56×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 177 µs (163 µs–221 µs) | 250 µs (233 µs–292 µs) | **1.41×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.54 µs (6.07 µs–8.6 µs) | 9.88 µs (9.35 µs–13.1 µs) | **1.51×** | 0 | - | n/a |
