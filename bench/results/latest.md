# barre vs Yoga — 2026-10-17 16:07

- barre: `ece98bb76264` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.98.1 (48a229cea 2026-09-01)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.3.34.2)
- Apple M4 Pro, 27.0.1, load avg at end 8.8/7.6/7.8
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.91×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 2.7 µs (2.55 µs–2.91 µs) | 2.59 µs (2.39 µs–2.81 µs) | **0.96×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 2.45 µs (2.24 µs–2.65 µs) | 2.49 µs (2.26 µs–2.71 µs) | **1.02×** | 10 | - | ok |
| `yoga/nested_flex` | 49 µs (46 µs–51.7 µs) | 43.9 µs (40.5 µs–48 µs) | **0.90×** | 100 | - | ok |
| `yoga/huge_nested` | 2.49 ms (2.39 ms–2.75 ms) | 2.53 ms (2.37 ms–2.81 ms) | **1.02×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 1.69 ms (1.63 ms–1.9 ms) | 1.52 ms (1.44 ms–1.71 ms) | **0.90×** | 0 | - | ok |
| `capture/feed-android` | 642 µs (620 µs–678 µs) | 589 µs (565 µs–619 µs) | **0.92×** | 57 | - | ok |
| `capture/feed-android.clean` | 22 ns (20 ns–33 ns) | 12 ns (12 ns–13 ns) | **0.58×** | 0 | - | ok |
| `capture/profile-ios` | 125 µs (116 µs–130 µs) | 111 µs (100 µs–116 µs) | **0.89×** | 13 | - | ok |
| `capture/profile-ios.clean` | 21 ns (20 ns–22 ns) | 12 ns (12 ns–14 ns) | **0.58×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 52.9 µs (51.2 µs–56.1 µs) | 49 µs (46 µs–51.5 µs) | **0.93×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 21 ns (21 ns–22 ns) | 12 ns (11 ns–14 ns) | **0.58×** | 0 | - | ok |
| `capture/chat-mac` | 3.46 ms (3.38 ms–3.65 ms) | 3.2 ms (3.09 ms–3.39 ms) | **0.92×** | 162 | - | ok |
| `capture/chat-mac.clean` | 21 ns (20 ns–22 ns) | 12 ns (12 ns–13 ns) | **0.59×** | 0 | - | ok |
| `rn/feed50/fresh` | 1.59 ms (1.54 ms–1.69 ms) | 1.36 ms (1.29 ms–1.44 ms) | **0.86×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 21 ns (21 ns–22 ns) | 12 ns (12 ns–13 ns) | **0.58×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 33.4 µs (32.8 µs–34.8 µs) | 30.8 µs (29.8 µs–32 µs) | **0.92×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 31.2 µs (30.6 µs–32.3 µs) | 29 µs (27.9 µs–30.1 µs) | **0.93×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 258 µs (249 µs–269 µs) | 217 µs (205 µs–227 µs) | **0.84×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 1.48 ms (1.42 ms–1.56 ms) | 1.3 ms (1.23 ms–1.36 ms) | **0.87×** | 135/136/139 | 1077/1078/1080 | ok |
| `rn/feed50/insert_remove` | 25.5 µs (24.1 µs–26.9 µs) | 24.1 µs (22.9 µs–25.4 µs) | **0.95×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 12.8 µs (12.3 µs–13.3 µs) | 11.7 µs (10.7 µs–12.3 µs) | **0.91×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 155 µs (148 µs–162 µs) | 143 µs (134 µs–153 µs) | **0.93×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 2.23 ms (2.16 ms–2.4 ms) | 2.22 ms (2.06 ms–2.46 ms) | **0.99×** | 4356 | 32 | ok |
| `rn/deep20/fresh` | 26.4 ms (26 ms–27.4 ms) | 30 ms (29 ms–32.4 ms) | **1.14×** | 66064 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/deep20/clean` | 21 ns (20 ns–22 ns) | 12 ns (12 ns–13 ns) | **0.58×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 877 µs (845 µs–926 µs) | 766 µs (719 µs–793 µs) | **0.87×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 50.5 µs (48.8 µs–53.6 µs) | 45 µs (42 µs–47.7 µs) | **0.89×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 436 µs (423 µs–451 µs) | 416 µs (396 µs–439 µs) | **0.95×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 250 µs (241 µs–260 µs) | 234 µs (222 µs–248 µs) | **0.94×** | 200 | 703 | ok |
| `micro/node_new_free` | 27 ns (26 ns–28 ns) | 47 ns (44 ns–50 ns) | **1.78×** | 0 | - | n/a |
| `micro/style_set_x20` | 53 ns (50 ns–55 ns) | 37 ns (36 ns–38 ns) | **0.69×** | 0 | - | n/a |
| `micro/style_get_x20` | 7 ns (7 ns–7 ns) | 12 ns (12 ns–13 ns) | **1.79×** | 0 | - | n/a |
| `micro/insert_remove_child` | 19 ns (18 ns–22 ns) | 20 ns (19 ns–22 ns) | **1.06×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 132 µs (127 µs–140 µs) | 154 µs (148 µs–162 µs) | **1.17×** | 0 | - | ok |
| `micro/layout_get_feed50` | 4.92 µs (4.43 µs–5.28 µs) | 8.76 µs (8.27 µs–9.39 µs) | **1.78×** | 0 | - | n/a |
