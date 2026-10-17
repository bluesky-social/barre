# barre benchmark history

Written by `.github/workflows/bench.yml` on every push to `main` of [bluesky-social/barre](https://github.com/bluesky-social/barre).
barre against Yoga through the same C API on a GitHub-hosted macOS arm64 runner (`bench/run`, docs/BENCH.md).
Shared runners are noisy: compare trends, not single runs.

| run | barre commit | geomean barre/Yoga (lower is faster) | workloads | |
|---|---|--:|--:|---|
| 2026-10-17 17:45 UTC | [`9bbc3b4aa9`](https://github.com/bluesky-social/barre/commit/9bbc3b4aa9) | 0.893× | 35 | [table](results/20261017-174509-9bbc3b4aa9.md) · [json](results/20261017-174509-9bbc3b4aa9.json) |

## Latest run

- barre: `9bbc3b4aa98e` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.99.1 (58cf8f9c3 2026-10-13)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.1.1.101)
- Apple M1 (Virtual), 26.6.2, load avg at end 15.6/29.3/19.2
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.89×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 4.6 µs (4.09 µs–11.1 µs) | 3.52 µs (3.09 µs–8.25 µs) | **0.77×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 4.35 µs (3.73 µs–10.4 µs) | 3.42 µs (2.99 µs–7.91 µs) | **0.78×** | 10 | - | ok |
| `yoga/nested_flex` | 82.2 µs (73.2 µs–204 µs) | 65.2 µs (58.7 µs–153 µs) | **0.79×** | 100 | - | ok |
| `yoga/huge_nested` | 5.23 ms (4.21 ms–9.36 ms) | 6.39 ms (3.68 ms–11.9 ms) | **1.22×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 4.41 ms (3.5 ms–8.13 ms) | 3.1 ms (2.44 ms–6.21 ms) | **0.70×** | 0 | - | ok |
| `capture/feed-android` | 1.15 ms (1.01 ms–2.44 ms) | 643 µs (566 µs–1.5 ms) | **0.56×** | 57 | - | ok |
| `capture/feed-android.clean` | 35 ns (32 ns–102 ns) | 51 ns (48 ns–120 ns) | **1.48×** | 0 | - | ok |
| `capture/profile-ios` | 211 µs (186 µs–473 µs) | 137 µs (115 µs–321 µs) | **0.65×** | 13 | - | ok |
| `capture/profile-ios.clean` | 37 ns (31 ns–103 ns) | 58 ns (48 ns–123 ns) | **1.55×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 86.4 µs (76.5 µs–214 µs) | 53.3 µs (46.1 µs–136 µs) | **0.62×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 33 ns (31 ns–101 ns) | 51 ns (48 ns–122 ns) | **1.58×** | 0 | - | ok |
| `capture/chat-mac` | 7.35 ms (5.58 ms–11.5 ms) | 4.23 ms (3.53 ms–8.67 ms) | **0.58×** | 162 | - | ok |
| `capture/chat-mac.clean` | 33 ns (31 ns–102 ns) | 54 ns (48 ns–122 ns) | **1.64×** | 0 | - | ok |
| `rn/feed50/fresh` | 3.21 ms (2.49 ms–5.69 ms) | 2.01 ms (1.61 ms–4.35 ms) | **0.62×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 33 ns (31 ns–100 ns) | 54 ns (48 ns–121 ns) | **1.64×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 62.4 µs (53.5 µs–170 µs) | 43.1 µs (38.5 µs–117 µs) | **0.69×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 56.7 µs (50 µs–155 µs) | 27.8 µs (24 µs–66.3 µs) | **0.49×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 431 µs (376 µs–1.08 ms) | 618 µs (523 µs–1.63 ms) | **1.43×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 2.52 ms (2.18 ms–5.29 ms) | 1.95 ms (1.54 ms–3.86 ms) | **0.77×** | 135/136/139, 144/148 | 1077/1078/1080 | **MISMATCH**: 27 nodes differ, max Δ 0.333344 at /0/0/6/0/0/1/0/2; measure calls differ [148, 136, 139, 135] vs [148, 148, 148, 144] |
| `rn/feed50/insert_remove` | 47.5 µs (41.2 µs–126 µs) | 32.5 µs (30 µs–88.8 µs) | **0.68×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 20 µs (18.5 µs–49.8 µs) | 12.8 µs (12.1 µs–30.4 µs) | **0.64×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 272 µs (233 µs–611 µs) | 169 µs (146 µs–365 µs) | **0.62×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 5.71 ms (4.53 ms–9.55 ms) | 2.56 ms (2.08 ms–4.62 ms) | **0.45×** | 4356, 4382 | 32 | ok; measure calls differ [4356, 4356, 4356, 4356] vs [4382, 4382, 4382, 4382] |
| `rn/deep20/fresh` | 74.3 ms (61.4 ms–106 ms) | 36.2 ms (29 ms–44.7 ms) | **0.49×** | 66064, 66937 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0; measure calls differ [66064, 66064, 66064, 66064] vs [66937, 66937, 66937, 66937] |
| `rn/deep20/clean` | 36 ns (31 ns–100 ns) | 58 ns (48 ns–125 ns) | **1.58×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 1.64 ms (1.38 ms–3.9 ms) | 1.01 ms (828 µs–3.26 ms) | **0.62×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 86.5 µs (76.5 µs–215 µs) | 55.6 µs (46.8 µs–140 µs) | **0.64×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 829 µs (668 µs–2.01 ms) | 529 µs (460 µs–1.55 ms) | **0.64×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 437 µs (381 µs–1.07 ms) | 293 µs (258 µs–755 µs) | **0.67×** | 200 | 703 | ok |
| `micro/node_new_free` | 70 ns (61 ns–151 ns) | 98 ns (87 ns–213 ns) | **1.41×** | 0 | - | n/a |
| `micro/style_set_x20` | 73 ns (66 ns–225 ns) | 109 ns (90 ns–238 ns) | **1.50×** | 0 | - | n/a |
| `micro/style_get_x20` | 13 ns (11 ns–34 ns) | 19 ns (17 ns–50 ns) | **1.48×** | 0 | - | n/a |
| `micro/insert_remove_child` | 24 ns (22 ns–64 ns) | 36 ns (34 ns–97 ns) | **1.48×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 199 µs (178 µs–479 µs) | 274 µs (237 µs–683 µs) | **1.37×** | 0 | - | ok |
| `micro/layout_get_feed50` | 7.55 µs (6.75 µs–14.8 µs) | 11.8 µs (10.8 µs–25.7 µs) | **1.56×** | 0 | - | n/a |
