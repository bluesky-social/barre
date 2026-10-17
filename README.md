# barre benchmark history

Written by `.github/workflows/bench.yml` on every push to `main` of [bluesky-social/barre](https://github.com/bluesky-social/barre).
barre against Yoga through the same C API on a GitHub-hosted macOS arm64 runner (`bench/run`, docs/BENCH.md).
Shared runners are noisy: compare trends, not single runs.

| run | barre commit | geomean barre/Yoga (lower is faster) | workloads | |
|---|---|--:|--:|---|
| 2026-10-17 20:08 UTC | [`52d96baf8c`](https://github.com/bluesky-social/barre/commit/52d96baf8c) | 0.662× | 35 | [table](results/20261017-200850-52d96baf8c.md) · [json](results/20261017-200850-52d96baf8c.json) |
| 2026-10-17 18:01 UTC | [`1854783423`](https://github.com/bluesky-social/barre/commit/1854783423) | 0.898× | 35 | [table](results/20261017-180128-1854783423.md) · [json](results/20261017-180128-1854783423.json) |
| 2026-10-17 17:55 UTC | [`300ee03da5`](https://github.com/bluesky-social/barre/commit/300ee03da5) | 0.897× | 35 | [table](results/20261017-175534-300ee03da5.md) · [json](results/20261017-175534-300ee03da5.json) |
| 2026-10-17 17:49 UTC | [`f47ea26fa8`](https://github.com/bluesky-social/barre/commit/f47ea26fa8) | 0.892× | 35 | [table](results/20261017-174927-f47ea26fa8.md) · [json](results/20261017-174927-f47ea26fa8.json) |
| 2026-10-17 17:45 UTC | [`9bbc3b4aa9`](https://github.com/bluesky-social/barre/commit/9bbc3b4aa9) | 0.893× | 35 | [table](results/20261017-174509-9bbc3b4aa9.md) · [json](results/20261017-174509-9bbc3b4aa9.json) |

## Latest run

- barre: `52d96baf8c99` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 12.9/17.5/10.7
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.66×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.12 µs (4.06 µs–5.25 µs) | 2.57 µs (2.53 µs–4.18 µs) | **0.62×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 3.77 µs (3.72 µs–7.49 µs) | 2.52 µs (2.48 µs–4.57 µs) | **0.67×** | 10 | - | ok |
| `yoga/nested_flex` | 73.5 µs (72.3 µs–113 µs) | 42.1 µs (40.5 µs–56 µs) | **0.57×** | 100 | - | ok |
| `yoga/huge_nested` | 4.59 ms (4.02 ms–6.51 ms) | 4.92 ms (3.56 ms–8 ms) | **1.07×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 3.48 ms (2.97 ms–5.43 ms) | 2.53 ms (1.82 ms–3.39 ms) | **0.73×** | 0 | - | ok |
| `capture/feed-android` | 978 µs (930 µs–1.62 ms) | 452 µs (428 µs–654 µs) | **0.46×** | 57 | - | ok |
| `capture/feed-android.clean` | 32 ns (32 ns–46 ns) | 20 ns (19 ns–59 ns) | **0.61×** | 0 | - | ok |
| `capture/profile-ios` | 188 µs (175 µs–270 µs) | 83.1 µs (79.7 µs–122 µs) | **0.44×** | 13 | - | ok |
| `capture/profile-ios.clean` | 31 ns (30 ns–41 ns) | 20 ns (19 ns–24 ns) | **0.63×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 77.1 µs (72.8 µs–105 µs) | 36 µs (34.2 µs–57.2 µs) | **0.47×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 31 ns (30 ns–42 ns) | 20 ns (19 ns–59 ns) | **0.65×** | 0 | - | ok |
| `capture/chat-mac` | 5.37 ms (5.05 ms–9.11 ms) | 2.72 ms (2.49 ms–5.45 ms) | **0.51×** | 162 | - | ok |
| `capture/chat-mac.clean` | 32 ns (30 ns–104 ns) | 19 ns (19 ns–58 ns) | **0.61×** | 0 | - | ok |
| `rn/feed50/fresh` | 2.35 ms (2.26 ms–4.17 ms) | 1.07 ms (988 µs–1.5 ms) | **0.45×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 31 ns (30 ns–38 ns) | 20 ns (19 ns–29 ns) | **0.64×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 53.2 µs (50.5 µs–65.9 µs) | 29.3 µs (28.6 µs–41.2 µs) | **0.55×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 51.1 µs (49.6 µs–70.5 µs) | 15.7 µs (15.2 µs–21.8 µs) | **0.31×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 393 µs (368 µs–536 µs) | 349 µs (321 µs–522 µs) | **0.89×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.29 ms (2.07 ms–2.96 ms) | 994 µs (947 µs–1.39 ms) | **0.43×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 42.5 µs (40.2 µs–52.9 µs) | 23.5 µs (22.8 µs–29.8 µs) | **0.55×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 18.7 µs (18.4 µs–23.6 µs) | 8.33 µs (8.17 µs–10.5 µs) | **0.45×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 248 µs (232 µs–323 µs) | 101 µs (98 µs–125 µs) | **0.41×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 3.93 ms (3.69 ms–5.23 ms) | 1.54 ms (1.42 ms–2.07 ms) | **0.39×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 52.9 ms (46.9 ms–79.1 ms) | 18.9 ms (17.7 ms–28.1 ms) | **0.36×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 31 ns (30 ns–38 ns) | 20 ns (19 ns–25 ns) | **0.63×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.36 ms (1.29 ms–1.8 ms) | 786 µs (650 µs–1.02 ms) | **0.58×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 78.2 µs (74.8 µs–102 µs) | 36.2 µs (35.7 µs–45.1 µs) | **0.46×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 694 µs (648 µs–1.17 ms) | 397 µs (368 µs–743 µs) | **0.57×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 413 µs (366 µs–703 µs) | 197 µs (180 µs–314 µs) | **0.48×** | 200 | 703 | ok |
| `micro/node_new_free` | 62 ns (60 ns–146 ns) | 90 ns (87 ns–152 ns) | **1.46×** | 0 | - | n/a |
| `micro/style_set_x20` | 68 ns (65 ns–92 ns) | 305 ns (298 ns–425 ns) | **4.49×** | 0 | - | n/a |
| `micro/style_get_x20` | 12 ns (11 ns–33 ns) | 18 ns (17 ns–44 ns) | **1.51×** | 0 | - | n/a |
| `micro/insert_remove_child` | 23 ns (22 ns–38 ns) | 34 ns (34 ns–101 ns) | **1.52×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 183 µs (176 µs–379 µs) | 265 µs (249 µs–476 µs) | **1.45×** | 0 | - | ok |
| `micro/layout_get_feed50` | 6.91 µs (6.5 µs–14.5 µs) | 10.4 µs (9.92 µs–24.6 µs) | **1.51×** | 0 | - | n/a |
