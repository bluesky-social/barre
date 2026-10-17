# barre vs Yoga — 2026-10-17 16:37

- barre: `c11d1b3ec7a7` (cargo --release, CARGO_PROFILE_RELEASE_LTO=fat, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1), rustc 1.98.1 (48a229cea 2026-09-01)
- yoga: `vendor/yoga` (pin 52f6d0bd), `-std=c++20 -gline-tables-only -O2 -DNDEBUG -flto=thin -fno-omit-frame-pointer -fexceptions -frtti -ffunction-sections -fdata-sections -fPIC`, Apple clang version 21.0.0 (clang-2100.3.34.2)
- Apple M4 Pro, 27.0.1, load avg at end 10.1/9.8/9.1
- 5 interleaved rounds × 300 ms per workload per engine (+100 ms warmup); times are ns per YGNodeCalculateLayout (or per op for `micro/`, per build+layout+free for `yoga/*` blocks)
- geometric mean barre/yoga over 35 workloads: **0.75×**

| workload | yoga median (p10–p90) | barre median (p10–p90) | barre/yoga | measure calls/layout (y, b) | new-layout nodes (y, b) | check |
|---|---|---|---|---|---|---|
| `yoga/stack_with_flex` | 2.69 µs (2.45 µs–2.93 µs) | 1.96 µs (1.77 µs–2.14 µs) | **0.73×** | 0 | - | ok |
| `yoga/align_stretch_undefined_axis` | 2.46 µs (2.25 µs–2.61 µs) | 1.9 µs (1.72 µs–2.03 µs) | **0.77×** | 10 | - | ok |
| `yoga/nested_flex` | 48.5 µs (44.7 µs–51.8 µs) | 34.6 µs (31.4 µs–37 µs) | **0.71×** | 100 | - | ok |
| `yoga/huge_nested` | 2.51 ms (2.41 ms–2.77 ms) | 2.15 ms (2.03 ms–3.05 ms) | **0.85×** | 0 | - | ok |
| `yoga/huge_nested.layout_only` | 1.73 ms (1.66 ms–1.92 ms) | 1.15 ms (1.08 ms–1.42 ms) | **0.67×** | 0 | - | ok |
| `capture/feed-android` | 648 µs (627 µs–691 µs) | 380 µs (362 µs–414 µs) | **0.59×** | 57 | - | ok |
| `capture/feed-android.clean` | 22 ns (20 ns–24 ns) | 14 ns (13 ns–15 ns) | **0.64×** | 0 | - | ok |
| `capture/profile-ios` | 127 µs (116 µs–140 µs) | 73.2 µs (66.9 µs–82 µs) | **0.58×** | 13 | - | ok |
| `capture/profile-ios.clean` | 21 ns (20 ns–23 ns) | 14 ns (13 ns–18 ns) | **0.68×** | 0 | - | ok |
| `capture/rendering-sample-mac` | 53.1 µs (48.2 µs–56.8 µs) | 32.5 µs (29.2 µs–34.9 µs) | **0.61×** | 24 | - | ok |
| `capture/rendering-sample-mac.clean` | 21 ns (19 ns–22 ns) | 14 ns (13 ns–15 ns) | **0.66×** | 0 | - | ok |
| `capture/chat-mac` | 3.51 ms (3.37 ms–3.69 ms) | 2.15 ms (2.06 ms–2.29 ms) | **0.61×** | 162 | - | ok |
| `capture/chat-mac.clean` | 21 ns (19 ns–22 ns) | 14 ns (13 ns–15 ns) | **0.65×** | 0 | - | ok |
| `rn/feed50/fresh` | 1.58 ms (1.5 ms–1.67 ms) | 946 µs (906 µs–1.02 ms) | **0.60×** | 548 | 1451 | ok |
| `rn/feed50/clean` | 21 ns (20 ns–22 ns) | 14 ns (13 ns–15 ns) | **0.65×** | 0 | 1 | ok |
| `rn/feed50/text_change` | 33.3 µs (30.6 µs–36 µs) | 21.8 µs (21.1 µs–23.1 µs) | **0.65×** | 1 | 63 | ok |
| `rn/feed50/text_same_size` | 31.2 µs (30.4 µs–32.7 µs) | 12.7 µs (12.3 µs–13.4 µs) | **0.41×** | 1 | 62 | ok |
| `rn/feed50/rotate` | 256 µs (246 µs–270 µs) | 292 µs (279 µs–307 µs) | **1.14×** | 0 | 1101 | ok |
| `rn/feed50/resize` | 1.47 ms (1.41 ms–1.53 ms) | 934 µs (883 µs–994 µs) | **0.64×** | 135/136/139 | 1077/1078/1080 | ok |
| `rn/feed50/insert_remove` | 25.5 µs (23.8 µs–27.2 µs) | 16.4 µs (15.7 µs–17.2 µs) | **0.64×** | 0 | 52/54 | ok |
| `rn/deep5/fresh` | 12.7 µs (11.5 µs–13.3 µs) | 7.71 µs (7 µs–8.04 µs) | **0.61×** | 17 | 12 | ok |
| `rn/deep10/fresh` | 152 µs (142 µs–162 µs) | 98.2 µs (89.9 µs–106 µs) | **0.64×** | 160 | 22 | **MISMATCH**: 1 nodes differ, max Δ 0.333328 at /0/1/1/1/1/0 |
| `rn/deep15/fresh` | 2.23 ms (2.16 ms–2.33 ms) | 1.62 ms (1.55 ms–1.85 ms) | **0.72×** | 4356 | 32 | ok |
| `rn/deep20/fresh` | 26.5 ms (26 ms–27.4 ms) | 22.5 ms (22 ms–23.2 ms) | **0.85×** | 66064 | 42 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/deep20/clean` | 21 ns (20 ns–22 ns) | 13 ns (13 ns–14 ns) | **0.64×** | 0 | 1 | **MISMATCH**: 6 nodes differ, max Δ 0.333374 at /0/1/1/1/1/1/1/1/1/1/1/1/0 |
| `rn/wide_column1000/fresh` | 878 µs (845 µs–926 µs) | 535 µs (514 µs–566 µs) | **0.61×** | 1000 | 4003 | ok |
| `rn/wide_row200/fresh` | 50.2 µs (47.5 µs–53.9 µs) | 32.6 µs (31.5 µs–35 µs) | **0.65×** | 400 | 201 | ok |
| `rn/wrap_grid300/fresh` | 436 µs (421 µs–462 µs) | 305 µs (292 µs–321 µs) | **0.70×** | 300 | 1203 | ok |
| `rn/absolute100/fresh` | 254 µs (243 µs–269 µs) | 159 µs (152 µs–170 µs) | **0.63×** | 200 | 703 | ok |
| `micro/node_new_free` | 26 ns (26 ns–28 ns) | 40 ns (40 ns–42 ns) | **1.51×** | 0 | - | n/a |
| `micro/style_set_x20` | 51 ns (49 ns–54 ns) | 52 ns (50 ns–56 ns) | **1.01×** | 0 | - | n/a |
| `micro/style_get_x20` | 7 ns (6 ns–8 ns) | 12 ns (12 ns–13 ns) | **1.80×** | 0 | - | n/a |
| `micro/insert_remove_child` | 19 ns (18 ns–22 ns) | 22 ns (20 ns–24 ns) | **1.15×** | 0 | - | n/a |
| `micro/build_feed50_tree` | 131 µs (123 µs–138 µs) | 151 µs (143 µs–161 µs) | **1.16×** | 0 | - | ok |
| `micro/layout_get_feed50` | 4.83 µs (4.34 µs–5.4 µs) | 8.08 µs (7.28 µs–8.98 µs) | **1.67×** | 0 | - | n/a |
