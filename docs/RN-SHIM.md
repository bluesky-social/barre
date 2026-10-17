# barre in React Native: shim design (milestone 5)

Target: social-app (`bluesky-social/social-app`), RN 0.86.3, Expo 57, Fabric + bridgeless. Paths are
relative to `social-app/node_modules/react-native/` (`RN/`) unless prefixed; line numbers are RN 0.86.3
as installed.

**TL;DR.** Keep Yoga's `yoga::Node`, `yoga::Style`, `yoga::Config` and the whole `YG*` C API as C++:
they are the persistence/ownership glue that RN *and four third-party libraries* compile against.
Replace only `yoga::calculateLayout` with a call into a separate `barre-rn` staticlib that walks the C++
nodes through a small host vtable and keeps its cache in a POD blob embedded in `yoga::Node`. iOS
already builds RN from source; Android must be forced to. **This requires barre's algorithm to be
generic over a node-access trait from M1** (§3.4), otherwise M5 is a rewrite.

---

> **Target is RN 0.88** (Samuel, 2026-10-17: social-app is upgrading shortly; 0.88.0 is on npm).
> The analysis below was done on 0.86.3 and still applies structurally. RN 0.88.0's
> `ReactCommon/yoga` is only a few commits behind barre's pin (`vendor/yoga` 52f6d0bd): it has
> errata bits 1|2|4|8 (`MinSizeUndefinedInsteadOfAuto` included) but not 16
> (`FlexFirstPassUsesRunningTotals`, yoga#2006), and it predates the pixel-grid rounding changes
> yoga#2037/#2040/#2041 (`PixelGrid.cpp` differs by ~130 diff lines). So: `barre-rn` masks errata
> to bits 1|2|4|8, and an A/B against RN's Yoga will show sub-pixel rounding differences
> that are expected (barre follows the newer rounding). Grid is present in both. Rebase the patch on
> `react-native@0.88.x` when doing the integration; line numbers below are 0.86.3.

## 0. Implementation status (milestone 5)

### Phase A: host-tested (done)

What exists, and where it deviates from §3 (c):

- **`crates/barre-rn`** (staticlib, exports only `barre_rn_init`,
  `barre_rn_state_size`, `barre_rn_layout_offsets`,
  `barre_rn_calculate_layout`): `node::RnNode` implements barre's
  `LayoutNode` over RN's `yoga::Node*`; `state::State` is the per-node blob
  (header, barre `Config` and `Style` snapshots, barre `LayoutResults`,
  ~1.1 KB of the 1280 reserved). The only change to `crates/barre` is
  `LayoutResults::OFFSETS` (byte offsets of the result fields, for C++).
- **`rn/patches/yoga-rn-0.88.patch`** (relative to the react-native package
  root, so the same hunks go into social-app's `pnpm patch`):
  - `yoga/barre/BarreRn.h`: the C interface (state header, flattened
    style/config, host table, layout offsets) mirrored by
    `crates/barre-rn/src/host.rs`; ABI version + `struct_size` checked at init.
  - `yoga/barre/BarreHost.{h,cpp}`: the host. Offsets into `yoga::Node` are
    measured at init on a probe node (pointer differences, so no
    `-Winvalid-offsetof`; bitfield flags found by toggling each setter on a
    node built in zeroed memory; the libc++ `std::vector` layout checked on
    a probe with two children). `barre::enabled()` reads `BARRE_LAYOUT`
    (Apple, host) or `debug.barre.layout` (Android) once; if barre is
    requested but its init fails, it **aborts** rather than silently laying
    out with Yoga (an A/B would compare Yoga with itself).
  - `node/Node.h`: `alignas(16) std::byte barreState_[BARRE_RN_STATE_SIZE]`
    (copied by the defaulted copy-ctor, zeroed in new/reset nodes), a friend
    for the host, and `barrePend(bit)`, called by **every** path that can
    change what barre snapshotted or reset what Yoga resets: non-const
    `style()` and `setStyle()` (style snapshot), `setConfig()`, `setLayout()`
    (YGNodeRemoveChild & co.), `markDirtyAndPropagate()` (computed flex
    basis). The move-ctor copies the blob; `Node.cpp`'s own reads of a
    child's display go through `std::as_const` so tree edits don't
    invalidate snapshots needlessly.
  - `algorithm/CalculateLayout.cpp`: `calculateLayout` dispatches to
    `barre::calculateLayout` or `calculateLayoutYoga` (the old body).
  - `YGNodeLayout.cpp`: every `YGNodeLayoutGet*` reads barre's results from
    the blob when barre is on (`barre::layoutState`), else Yoga's.
- **Deviation: no layout write-back.** §3 (c) (and the architect's note)
  had barre write results into Yoga's `LayoutResults` at the end. Instead
  barre's blob is the single source of truth and the C API getters read it
  through offsets barre exports (`barre_rn_layout_offsets`). Reasons: RN
  reads layout only through `YGNodeLayoutGet*` (no `getLayout()` user
  outside `ReactCommon/yoga`, nor in screens/safe-area/svg/expo); it costs
  one branch per getter instead of an end-of-pass walk with a write per
  node; and every node is always consistent, including nodes a pass reached
  but did not flag (rounding) and nodes it did not reach. Yoga's
  `LayoutResults` keeps its defaults, which the getters fall back to for
  nodes barre has not laid out (or that Yoga reset: `setLayout`).
- **Hot path without calls:** children (`std::vector` begin/end), owner,
  config pointer, flags (dirty, hasNewLayout, node type, reference baseline,
  containing block), function-pointer presence and min-content sizes are
  plain loads at the measured offsets; the snapshots and results are in the
  blob. Each node is checked once per layout pass (`header.pass`, a pass id
  in the handle): pending bits from C++, config pointer and
  `Config::version_` (configs change without touching their nodes). Calls
  into C++ remain only where Yoga calls out (measure, baseline,
  `cloneChildrenIfNeeded` when a child is not owned, the dirtied callback
  via `setDirty(true)`) and to re-snapshot a changed style/config.
- **Errata:** barre lays out with `(errata & 0xF) | 16`. RN 0.88's Yoga
  knows bits 1|2|4|8 and always has the pre-yoga#2006 first-pass behaviour,
  which in barre (= vendor/yoga) is bit 16 `FlexFirstPassUsesRunningTotals`.
  Masking alone (as originally planned) would turn the yoga#2006 fix *on*
  and change layouts against RN's Yoga.
- **Events:** barre publishes none of Yoga's `yoga::Event`s (RN doesn't
  subscribe).

**Conformance** (`./conf --engine rn-yoga | rn-yoga-patched | rn-barre`,
docs/CONFORMANCE.md):

| engine | pass | fail | build-fail | notes |
|---|---|---|---|---|
| `barre` (barre-ffi) | 847/847 | 0 | 0 | |
| `rn-yoga` (stock RN 0.88) | 792 + 34 xfail | 5 | 16 | version skew, below |
| `rn-yoga-patched` | 792 + 34 xfail | 5 | 16 | identical per test to `rn-yoga` |
| `rn-barre` | 829 | 2 | 16 | everything barre passes except the skew |
| `rn-yoga-patched --no-skip` | 892 + 34 xfail | 5 | 23 | |
| `rn-barre --no-skip` | 921 | 10 | 23 | + 8 `EventsTest` (barre emits no events) |

Version skew (RN 0.88's Yoga vs `vendor/yoga` 52f6d0bd), all expected:
- 16 build-fails: `YGFirstChildMinWidthTest`, `barre/YGConfigTest`,
  `barre/YGFirstPassRunningTotalsTest` use `YGErrataFlexFirstPassUsesRunningTotals`,
  which RN 0.88's headers lack (`--no-skip` adds 7 more files that need
  newer internals).
- `YGDirtyMarkingTest.dirty_propagation_through_child_set_to_display_contents`
  and `YGStyleTest.copy_style_box_sizing` fail on both arms: they test RN
  0.88's C++ (`YGNodeStyleSetDisplay`'s owner recount, `Style::operator==`),
  which the shim keeps.
- `YGRoundingLargeOffsetTest` and two `YGRoundingMeasureFuncTest` tests fail
  on RN's Yoga (older pixel rounding, before yoga#2037/#2040/#2041) and
  pass on `rn-barre` (barre follows the newer rounding).
- The 34 `yoga-divergences.txt` tests (Yoga bugs barre fixes) xfail on RN's
  Yoga and pass on `rn-barre`.

`--no-skip` matters here: with real C++ nodes, the files `skip.txt` excludes
for needing Yoga internals link, and `YGPersistentNodeCloningTest`,
`YGPersistenceTest`, `YGNodeCallbackTest`, `YGDirtiedTest`, `CacheTest` pass
on `rn-barre` exactly as on Yoga: the clone callback, copy-constructed nodes
and the dirtied callback behave through the host.

**Host-interface cost** (`bench/run --rn`, docs/BENCH.md; 5 rounds × 300
ms, Apple M4 Pro, barre-rn and barre-ffi with fat LTO, RN Android's C++
flags; barre at `7b2b248` + this work). Every `rn-barre` layout is
bit-identical to `barre`'s (checksums), measure-call and hasNewLayout counts
equal. Geometric means over 30 layout workloads:

| ratio | what it measures | geomean |
|---|---|---|
| `rn-barre/rn-yoga` | the in-app A/B (same binary, switch on/off) | **1.11×** |
| `rn-barre/barre` | host interface over RN's C++ nodes vs barre's own C ABI | 1.35× |
| `rn-yoga/yoga` | the patch (+1280-byte node) and version skew, on Yoga | 1.07× |

| workload | yoga | barre | rn-yoga | rn-barre | rn-barre/rn-yoga | rn-barre/barre |
|---|---|---|---|---|---|---|
| `capture/feed-android` | 638 µs | 399 µs | 661 µs | 559 µs | 0.85× | 1.40× |
| `capture/chat-mac` | 3.46 ms | 2.36 ms | 3.58 ms | 3.10 ms | 0.87× | 1.31× |
| `rn/feed50/fresh` | 1.57 ms | 1.07 ms | 1.62 ms | 1.47 ms | 0.91× | 1.38× |
| `rn/feed50/text_change` | 33.5 µs | 26.8 µs | 35.5 µs | 32.2 µs | 0.91× | 1.20× |
| `rn/feed50/text_same_size` | 31.2 µs | 17.5 µs | 33.5 µs | 21.2 µs | 0.63× | 1.21× |
| `rn/feed50/rotate` | 256 µs | 371 µs | 264 µs | 457 µs | 1.73× | 1.23× |
| `rn/feed50/resize` | 1.47 ms | 1.03 ms | 1.52 ms | 1.30 ms | 0.85× | 1.26× |
| `rn/feed50/insert_remove` | 25.2 µs | 20.3 µs | 27.0 µs | 24.2 µs | 0.90× | 1.19× |
| `rn/deep20/fresh` | 26.2 ms | 15.9 ms | 27.9 ms | 18.8 ms | 0.68× | 1.18× |
| `rn/wide_column1000/fresh` | 892 µs | 537 µs | 926 µs | 1.05 ms | 1.13× | 1.95× |
| `rn/wrap_grid300/fresh` | 443 µs | 334 µs | 523 µs | 648 µs | 1.24× | 1.94× |
| `rn/*/clean` (nothing dirty) | 21 ns | 33 ns | 22 ns | 39 ns | 1.78× | 1.18× |

Where the interface cost goes (`bench/profile ... --engine rn-barre -- --rn`):
- **First layout of a node** (the `fresh` workloads, wide trees worst): the
  style snapshot (C++ flattens `yoga::Style` through its value pool, ~36
  edge reads; Rust rebuilds barre's `Style`), the config snapshot, and
  initialising the 1.1 KB blob. The whole rn-barre − barre gap in
  `wide_column1000` (4000 nodes) is ~130 ns per node, most of it this. In
  RN it is paid once per mounted shadow node, then only when its style
  changes.
- **Per visit**: the state pointer and offset loads instead of barre's own
  fields; the once-per-pass check (config pointer + `version_` loads).
  Before the pass stamp (checking pending bits, style generation and
  config on every `child(i)`), that check was 10% of `feed50/fresh`'s
  samples; now each node is checked once per pass.
- No calls into C++ on the steady-state path: `rotate` (whole-tree
  relayout, no style change) is 1.23× barre, all of it loads.
- `rotate` is slower than Yoga in both barre arms: T-0006's flex-basis
  anchor (docs/BENCH.md), not the shim.

Getting closer to barre-ffi would mean caching more per node in the blob
(children pointer and count, flags) or a cheaper snapshot (copying
`yoga::Style`'s pool handles and decoding lazily); the remaining gap is
loads, not calls. Pixel differences between the arms are RN 0.88's older
rounding (e.g. `feed50`'s text nodes 1 pt taller under RN's Yoga at
y ≈ 17 000, yoga#2037), already visible as `rn-yoga` vs `yoga` mismatches.

### Phase B: social-app integration (in progress, handover 2026-10-18)

Worktree `~/Documents/Programming/bluesky-social/social-app-barre`, branch
`samuel/barre-layout-engine`, stacked on #12057 (RN 0.88 / Expo 58) and
#12024 (Android source builds). Nothing here is committed or pushed yet.

**What exists (uncommitted):**

- `plugins/withBarre.js`, registered in `app.config.js`. With `BARRE=1` at
  prebuild it runs `scripts/barre/build.sh`; without it, a no-op.
- `scripts/barre/build.sh` (`pnpm barre:build [ios|android|all]`): builds
  `barre-rn` with `--profile release-lto` and installs it into
  `react-native/ReactCommon/yoga/barre-rn/` (an xcframework on iOS, per-ABI
  `.a` files on Android), where the patched podspec and CMakeLists pick it up.
- `patches/react-native@0.88.0.patch`: the original patch plus the Yoga hunks
  appended. They are byte-identical to `rn/patches/yoga-rn-0.88.patch` on this
  branch (checked: 0 diff lines), so no refresh is needed.
- `pnpm-lock.yaml`: the only change is the `react-native` `patch_hash`,
  repeated wherever it is referenced. That is what pnpm writes when the patch
  changes, not an accidental regeneration.
- `node_modules` already has the patch applied, `ios/` is prebuilt, and a
  `BarreRN.xcframework` is already installed. It predates the fmt-only commit
  and came from a checkout I did not record, so rebuild it before measuring.

**Not verified:** no A/B build has been completed or launched. Treat every
claim above about the app side as "reads right", not "works".

**Gotchas:**

- `build.sh` defaults `BARRE_SRC` to `../barre`, the main checkout, which has
  no `crates/barre-rn` until the rn-shim layer (#12) merges. Until then set
  `BARRE_SRC` to a checkout of the `rn-shim` branch. After the merge the
  default works.
- The switch is off by default. The arm is chosen at launch: `BARRE_LAYOUT=1`
  (iOS) or the `debug.barre.layout` system property (Android). If barre is
  requested but fails to initialise, the app aborts rather than falling back
  to Yoga, so a running barre arm really is barre.
- iOS Release builds RN's C++ at `-Os` (social-app#12061 would make it `-O2`).
  Build both arms with the same optimisation flags or the A/B is meaningless.
- Expect sub-pixel differences against RN's Yoga (barre follows the newer
  rounding, yoga#2037/#2040/#2041). That is expected, not a shim bug.

**Next steps:**

1. Rebuild barre-rn (`BARRE=1`, iOS) and build the app once. The arm is a
   launch switch, so one binary serves both arms.
2. Launch it on the simulator `iPhone 17 Pro (barre)`
   (`564B0A5F-F8BA-480A-8AA6-240F060768CA`) with and without `BARRE_LAYOUT=1`.
   Check first that the barre arm starts and that a feed and a long thread
   render the same as Yoga's.
3. Give the argent comparison (scroll flow, layout timings) to a Sonnet
   subagent at medium effort. Reuse the instrumentation and flow from the o2
   experiment (raw data in `/tmp/bsky-o2-runs/`).
4. Write the results here, and commit the social-app work in stacked
   commits (`gh stack`, never `sync`).

Building was deliberately held back until the o2 experiment on `bsky-o2`
finished (3:00am on 2026-10-18): an Xcode build on the same machine would
perturb its timings. Do not use `bsky-o2`, the iPhone 14 or `social-app-o2`
for this work.

---

## 1. How RN 0.86 consumes Yoga

### 1.1 social-app's build mode

`ios/` and `android/` are CNG output (gitignored, `social-app/.gitignore:98-99`); config is
`social-app/app.config.js:310-333` (`expo-build-properties`):

- **iOS**: `buildReactNativeFromSource: true` (:315). The Expo Podfile template maps it to
  `RCT_USE_RN_DEP=0` / `RCT_USE_PREBUILT_RNCORE=0` (`expo/template.tgz: ios/Podfile:16-17`). Confirmed
  in `social-app/ios/Pods`: no `React-Core-prebuilt`/`ReactNativeDependencies`, `Local
  Podspecs/Yoga.podspec.json` present, Folly/glog/boost from source. C++23, iOS 16.4.
- **Android**: `buildReactNativeFromSource: IS_PRODUCTION` (:331). Dev builds use the **prebuilt
  `com.facebook.react:react-android` AAR**; only production adds the `includeBuild(...)
  { substitute(module("com.facebook.react:react-android")) ... }` block to `settings.gradle`
  (`expo-build-properties/build/android.js:280-293`). `newArchEnabled=true` (template
  `android/gradle.properties:38`).
  **Update (Samuel, 2026-10-17):** a social-app PR enables source builds for Android dev too.
  Assume ReactAndroid is always built from source; the prebuilt-AAR path below is moot.

### 1.2 iOS

- `scripts/react_native_pods.rb:199`: `pod 'Yoga', :path => "#{prefix}/ReactCommon/yoga"`.
- `ReactCommon/yoga/Yoga.podspec`: compiles `yoga/**/*.{cpp,h}` (:52-54) with `-Wall -Werror -std=c++20`
  (:38-45); all non-umbrella headers are *private* headers (:64) so Fabric can include
  `yoga/node/Node.h`; Fabric pods get `spec.dependency "Yoga"` + `Headers/Private/Yoga`
  (`scripts/cocoapods/new_architecture.rb:84,125`).
- Prebuilt mode (`react_native_pods.rb:135-137,212-217`) precompiles Fabric, including
  `YogaLayoutableShadowNode` with inlined `yoga::Node` accessors and `sizeof(yoga::Node)`, into
  `React.xcframework`. Changing `Node.h` is ABI-breaking there, so **barre needs source builds**, which
  social-app already does (Expo precompiled modules are off in this mode anyway:
  `expo-modules-autolinking/scripts/ios/precompiled_modules.rb:117-126`).

### 1.3 Android

- `ReactAndroid/src/main/jni/CMakeLists.txt:64` `add_react_common_subdir(yoga)` →
  `ReactCommon/yoga/yoga/CMakeLists.txt:21-25` builds `yogacore` STATIC from a glob of every `.cpp`, with
  IPO (:35), forced Release even in debug (`ReactCommon/yoga/CMakeLists.txt:12`).
- `first-party/yogajni` (`CMakeLists.txt:146`) is the Java `com.facebook.yoga` JNI, C API only
  (`yogajni/jni/YGJNI.h:8` includes `<yoga/Yoga.h>`). Everything is merged into **one
  `libreactnative.so`** (:259 `$<TARGET_OBJECTS:yoga>`, :272 links `yogacore`, `target_merge_so`).
  Yoga headers reach third-party C++ via prefab (`ReactAndroid/build.gradle.kts:271`).
- The prebuilt AAR's `libreactnative.so` *exports* Yoga's C++ symbols (`llvm-nm -DC` on
  `~/.gradle/.../react-android-0.86.3-release.aar`: `T facebook::yoga::Node::insertChild`,
  `replaceChild`, `setChildren`, ...; 185 `YGNode*` symbols) and third-party `.so`s link against them.
  Yoga is statically merged inside, so it **cannot be swapped without building ReactAndroid from
  source**: barre builds must set `android.buildReactNativeFromSource: true`.
- Java `YogaNode` is only instantiated by legacy `ReactShadowNodeImpl.java:108`; other Java uses
  (`MeasureUtil.kt`, `TextLayoutManager.kt`, ...) are `YogaMeasureMode`/constants. Linked, unused.

### 1.4 Where a replacement plugs in

From-source builds compile `RN/ReactCommon/yoga` straight out of `node_modules` on both platforms, so
one `pnpm patch` of `react-native@0.86.3` (social-app already carries
`social-app/patches/react-native@0.86.3.patch`, wired in `pnpm-workspace.yaml:34+`) reaches iOS (pod
`:path`) and Android (CMake glob + prefab headers). Each platform additionally needs one link input: a
vendored xcframework in `Yoga.podspec`, an `IMPORTED` static lib in `yoga/CMakeLists.txt`.

---

## 2. The surface RN actually uses

### 2.1 `yoga::Node`, by value, inside every shadow node

`YogaLayoutableShadowNode.h:13` includes `<yoga/node/Node.h>`; members `yoga::Config yogaConfig_` (:96,
**one Config per shadow node**) and `mutable yoga::Node yogaNode_` (:103). Calls in
`YogaLayoutableShadowNode.cpp`:

| call | lines | notes |
|---|---|---|
| `Node(const Config*)` | 73 | |
| `Node(const Node&)` (defaulted) | 100-102 | **clone = memberwise copy**: style, layout + measure cache, children vector, owner, funcs, dirty |
| `setOwner` | 131 (nullptr), 212, 304, 332 | 332 writes sentinel `0xBADC0FFEE0DDF00D` |
| `setDirty(bool)` | 164,169,247,375,387,552,576,587 | non-propagating (`Node.cpp:174-182`) |
| `isDirty` / `YGNodeIsDirty` | 112,173,344,366,386,693 | |
| `getChildren()` | 329,348,695,757,1032,1051 | `const std::vector<Node*>&` |
| `setChildren`, `insertChild`, `replaceChild(Node*,size_t)`, `removeChild(size_t)` | 350, 194, 305, 310 | |
| `style()` (mutable ref), `setStyle`, `style()==` | 367,386,390,546-551,584-586,**635-652**,920-967 | `layoutTree` writes root min/max **through the reference**, no `setDirty` |
| `getHasNewLayout` / `setHasNewLayout(false)` | 666,671,700-701 | RN copies metrics only for flagged nodes |

C API on the same object: `YGNodeSetContext` (:74,130), `YGNodeSetMeasureFunc` (:80,181),
`YGNodeSetBaselineFunc` (:84), `YGNodeSetConfig` (:132), `YGNodeGetOwner` (:210,303,319,330,709),
`YGNodeGetChildCount`, `YGNodeSetAlwaysFormsContainingBlock` (:397), `YGNodeStyleGetDisplay` (:400),
`YGNodeStyleGetOverflow` (:737), `YGNodeCalculateLayout` (:661), `YGNodeGetContext` (:888). Config:
`YGConfigSet/GetErrata`, `YGConfigSet/GetPointScaleFactor`, `YGConfigSetCloneNodeFunc` (:894),
`YGConfigSetExperimentalFeatureEnabled(FixFlexBasisFitContent)` (:903), logger via
`Config(FabricDefaultYogaLog)` (:71). Layout read-back is all C API: `layoutMetricsFromYogaNode`
(`components/view/conversions.h:142-171`, `YGNodeLayoutGet{Left,Top,Width,Height,Border,Padding,Direction}`),
`ParagraphShadowNode.cpp:82,212,215` (`RawWidth/RawHeight/Direction`), `BaseTextInputShadowNode.h:109`,
`AndroidTextInputShadowNode.cpp:92`. Never used: `YGNodeSetDirtiedFunc`, `YGNodeMarkDirty`, `yoga::Event`.

### 2.2 Value/style types (plain data)

Props store a `yoga::Style yogaStyle` (`YogaStylableProps.h`), built and read by `propsConversions.h`,
`YogaStylableProps.cpp`, `conversions.h`, `BaseViewProps.cpp`, `ViewShadowNode.cpp:42-57`,
`ConcreteViewShadowNode.h:100-107`, `animationbackend/AnimatedProps*.h`, `CullingContext.cpp:27`,
`AndroidTextInputComponentDescriptor.h:89-121` (const_casts and mutates props style). Types: `Style`
getters/setters, `StyleLength`, `StyleSizeLength` (`points/percent/ofAuto/ofStretch/ofMaxContent/
ofFitContent/undefined`), `FloatOptional`, `Edge`, `Dimension`, `Gutter`, `ordinals<>`, all enums.
`Style` wraps a `StyleValuePool` (`style/StyleValuePool.h:191`, `SmallValueBuffer<4>`): compact, not
FFI-friendly. RN 0.86's Yoga has `YGDisplayGrid` and grid tracks (`YGEnums.h:50,87-90`).

### 2.3 Third-party native code in social-app touching Yoga internals

- `react-native-screens/.../RNSSafeAreaViewShadowNode.cpp:79-91` (`yogaNode_.style()`, `setStyle`,
  `setDirty(true)`); 8 `*ShadowNode.h` call `yogaStyleHasSize(yogaNode_.style(), size)` (added by
  `social-app/patches/react-native-screens@4.30.0.patch`).
- `react-native-safe-area-context/.../RNCSafeAreaViewShadowNode.cpp:102-126` (same pattern).
- `react-native-svg/.../RNSVGLayoutableShadowNode.cpp:28-33` (`style()`/`setStyle`).
- `expo-modules-core/common/cpp/fabric/ExpoViewShadowNode.h:156-200` (C API + style),
  `ExpoViewComponentDescriptor.h:73-120` (props style). `react-native-nano-icons` (props style only).

So `yoga::Node`'s C++ interface is de facto public API: `style()`, `setStyle()`, `setDirty()` must stay
source-compatible. Legacy iOS `RCTShadowView.m` (behind `#ifndef RCT_REMOVE_LEGACY_ARCH`, :19) and
Android `yogajni` use only the C API.

### 2.4 The persistence model barre must honour

1. A shadow-node clone copy-constructs `yoga::Node`: the new node shares the old node's **children
   pointers** and inherits its **layout cache** and dirty flag (comment at :103-105). That is how
   unchanged subtrees skip layout across commits.
2. Children are shared across revisions; `owner` says who may mutate. RN nulls the clone's owner (:131),
   adopts unowned children (:210-212), and stamps the old parent's children with junk (:328-335).
3. During layout Yoga calls `cloneChildrenIfNeeded()` (`Node.cpp:388-405`; from `CalculateLayout.cpp:503,
   523,1409`, `AbsoluteLayout.cpp:560`): a child whose owner ≠ this is replaced via `config->cloneNode`
   → `yogaNodeCloneCallbackConnector` (:798-806), which clones the **ShadowNode** (`cloneChildInPlace`)
   and returns its embedded node. `display: contents` has `cloneContentsChildrenIfNeeded`.
4. Dirtiness propagates *by cloning*, not `markDirtyAndPropagate`: `updateYogaChildren` does
   `setDirty(!isClean)` (:337-376).
5. `hasNewLayout` is the contract with `layout()` (:695-735): RN copies metrics / fires `onLayout` only
   for flagged children. barre must flag exactly Yoga's set (incl. `zeroOutLayoutRecursively` and
   `cleanupContentsNodesRecursively`, `CalculateLayout.cpp:497-525`).
6. Measure/baseline funcs get `YGNodeConstRef` and read `context` (= ShadowNode*). Text measurement
   (`ParagraphShadowNode::measureContent`, :222) is the expensive callback.
7. Layout runs on whichever thread commits (`ShadowTree.cpp:409`, `SurfaceHandler.cpp:212`); surfaces
   can lay out concurrently. Yoga's only global is `gCurrentGenerationCount` (`CalculateLayout.cpp:36,2454`).

### 2.5 Version skew (matters for A/B)

`barre/vendor/yoga` (main, Oct 2026) differs from RN 0.86's copy (~970 diff lines). Vendor adds
`YGErrataMinSizeUndefinedInsteadOfAuto = 8`, `YGErrataFlexFirstPassUsesRunningTotals = 16`, and
`YGMinContentMeasureFunc`; RN 0.86 knows bits 1|2|4 only (`RN/ReactCommon/yoga/yoga/YGEnums.h:64-71`).
RN passes `YGErrataAll` (`YogaLayoutableShadowNode.cpp:604`), so barre with vendor semantics would
silently enable two extra errata. `barre-rn` must mask errata to RN's set.

---

## 3. Shim options

### (a) Replacement `yoga::Node` forwarding to a barre handle

- The copy-ctor runs on every ShadowNode clone (thousands per commit) and would have to FFI-clone Rust
  state: a call + allocation each.
- `getChildren()` returns `const std::vector<Node*>&`, `style()` returns a mutable `Style&` that RN
  writes through (:635-652), and owner gets a junk sentinel. All three force a C++ mirror anyway →
  double bookkeeping. Style ownership is lose-lose: copying into Rust per `setStyle` misses
  write-through; Rust reading C++ `Style` means bindgen over `StyleValuePool`'s encoding.
- PLAN.md's arena "owned by a tree" has no owner here: nodes belong to refcounted ShadowNodes shared
  across revisions and threads → a global locked arena with refcounts.

### (b) Patch RN to use only the C API

Rewrite `YogaLayoutableShadowNode.{h,cpp}` (~1300 LOC) and `conversions.h` around a `YGNodeRef`. The C
API lacks `setOwner`, non-propagating `setDirty` (`YGNodeMarkDirty` propagates and asserts a measure
func, `YGNode.cpp:108+`), whole-`Style` get/set/compare, and by-value copy (`YGNodeClone` heap-allocates).
We'd invent non-standard `YG*` extensions *and* patch screens, safe-area-context, svg,
expo-modules-core (§2.3). Invasive, rebases badly on every upgrade. No.

### (c) Keep Yoga's data model, replace only the algorithm — **recommended**

Keep `Node/Style/Config/LayoutResults`, `Node.cpp` tree ops and every `YG*.cpp` untouched. Change three
things in `RN/ReactCommon/yoga`:

1. `node/Node.h`: add `alignas(16) std::byte barreState_[BARRE_RN_STATE_SIZE]` + accessor, and a
   `uint32_t styleGen_` bumped by `setStyle()` **and by non-const `style()`** (catches write-through).
   The defaulted copy-ctor copies the blob, so clones inherit barre's cache exactly as they inherit
   Yoga's; add it to the hand-written move-ctor (`Node.cpp:31-50`) and `reset()`.
2. `algorithm/CalculateLayout.cpp`: rename `calculateLayout` → `calculateLayoutYoga`; the new
   `calculateLayout` (called by `YGNodeCalculateLayout`, `YGNode.cpp:87-94`) dispatches on
   `barreEnabled()` (§3.6) to `barre_rn_calculate_layout(node, ownerW, ownerH, dir, &kBarreHost)`.
3. New `algorithm/BarreHost.cpp` (must pass `-Wall -Werror`): the host vtable, thin C++ over
   `yoga::Node`: `children(n, &ptr, &len)`; `clone_children_if_needed(n)` /
   `clone_contents_children_if_needed(n)` (call the existing methods, so RN's clone callback runs
   unchanged); `style_snapshot(n, BarreStyle*)` (flatten `Style` into a `repr(C)` POD, only when
   `styleGen_` changed); `config(n, BarreConfig*)` (masked errata, point scale, experimental features);
   `measure`/`baseline` (via `Node::measure/baseline`, which handle context); `write_layout(n, const
   BarreLayoutOut*)` (fills `LayoutResults`, so every `YGNodeLayoutGet*` reader keeps working);
   `set_has_new_layout`, `set_dirty`; flags (`nodeType`, `isReferenceBaseline`,
   `alwaysFormsContainingBlock`, `hasContentsChildren`).

Rust side, `crates/barre-rn` (staticlib, **exports only `barre_rn_*`**, no `YG*`, so no clash with
Yoga's C API): a tree impl over `*mut yoga::Node` — children/clone/measure via the vtable, cache and
style snapshot read straight from the blob (no FFI on the cache-hit path). One process-wide
`AtomicU32` generation counter (mirrors Yoga). `panic = "abort"` (unwinding into C++ is UB). cbindgen
emits `barre_rn.h` with `BARRE_RN_STATE_SIZE` and the vtable, `static_assert`ed in `Node.h`.

Tradeoffs: barre's own arena/C-ABI layer is not exercised in RN, only the algorithm + cache (which is
where the time and Yoga's bugs #1552/#2019 live). `sizeof(yoga::Node)` changes, so all RN and
third-party C++ must rebuild (fine from source). Vtable calls don't inline without cross-language LTO
(`-Clinker-plugin-lto` + clang ThinLTO with matching LLVM) — measure before bothering. Deviates from
PLAN.md M5's "replaces the node": we replace the node's algorithmic state, not its C++ shell.

### 3.4 Requirements on barre core (decide in M1)

- Algorithm generic over a node-access trait (Taffy's `LayoutPartialTree` shape): children, style
  view, measure/baseline, cache get/set, layout write-back, clone-children hook. The arena + `barre-ffi`
  C ABI is one impl; `barre-rn` is another.
- Per-node cache/state is `Copy`, fixed-size, pointer-free (lives in a memcpy'd C++ blob). Budget ≈
  Yoga's (`node/LayoutResults.h:25-37`: 8 `CachedMeasurement`s, cached layout, flex basis + generation).
- Style via a flat snapshot struct; no global state beyond the generation counter; reentrant across
  threads on distinct trees.
- `hasNewLayout`, clone-on-traverse and `display: contents` behave exactly as Yoga. `display: grid`
  either implemented or delegated (barre returns "unsupported", C++ runs `calculateLayoutYoga`).

### 3.5 Step-by-step plan

1. **`crates/barre-rn`** + cbindgen header. Release profile: `lto = "fat"`, `codegen-units = 1`,
   `panic = "abort"`, `debug = "line-tables-only"` (symbols for Instruments / simpleperf).
2. **Host tests first**: build RN 0.86's `ReactCommon/yoga` with the patch on macOS and run Yoga's C API
   gtests through `./conf` in both modes, plus ports of RN's `components/view/tests/{LayoutTest,
   ViewTest}.cpp` (copy-ctor clones, owner juggling, clone callback).
3. **iOS**: `cargo build --release -p barre-rn --target aarch64-apple-ios` and
   `--target aarch64-apple-ios-sim` (`IPHONEOS_DEPLOYMENT_TARGET=16.4`; add `x86_64-apple-ios` only for
   Intel sims), then `xcodebuild -create-xcframework -library <ios>/libbarre_rn.a -headers include
   -library <ios-sim>/libbarre_rn.a -headers include -output BarreRN.xcframework`.
4. **Android**: `cargo ndk -t arm64-v8a -t x86_64 [-t armeabi-v7a -t x86] --platform 24 build --release
   -p barre-rn` (minSdk 24, `RN/gradle/libs.versions.toml:3`) → `target/<triple>/release/libbarre_rn.a`.
   Dev: arm64-v8a only, `-PreactNativeArchitectures=arm64-v8a`.
5. **Patch** (extend `react-native@0.86.3.patch` via `pnpm patch`, documented in its `.md`):
   - the three `ReactCommon/yoga` changes above;
   - `Yoga.podspec`: `if ENV['BARRE_RN_DIR']` → `vendored_frameworks = ".../BarreRN.xcframework"`,
     `GCC_PREPROCESSOR_DEFINITIONS += BARRE_RN=1`;
   - `yoga/yoga/CMakeLists.txt`: `if(DEFINED BARRE_RN_DIR)` → `add_library(barre_rn STATIC IMPORTED)`
     at `${BARRE_RN_DIR}/${ANDROID_ABI}/libbarre_rn.a`, link into `yogacore`, define `BARRE_RN=1`, and
     add `-Wl,--exclude-libs,libbarre_rn.a` to `reactnative` so Rust std symbols aren't exported;
   - `ReactAndroid/build.gradle.kts` cmake args: forward `-DBARRE_RN_DIR=${findProperty("barre.dir")}`
     (a gradle property, not an env var: the daemon caches env).
   Without `BARRE_RN_DIR` the patch builds stock Yoga.
6. **Expo config plugin** `plugins/withBarre.js` (active when `BARRE=1` at prebuild): force
   `android.buildReactNativeFromSource: true`, write `barre.dir` to `gradle.properties`, add
   `ENV['BARRE_RN_DIR'] ||= ...` to the Podfile header (same pattern as template lines 16-17).

### 3.6 A/B

One binary links both: Yoga's `algorithm/*.cpp` stays as the fallback arm. `barreEnabled()` is read
once per process: iOS `getenv("BARRE_LAYOUT")` (launch with `SIMCTL_CHILD_BARRE_LAYOUT=1`), Android
`__system_property_get("debug.barre.layout")` (`adb shell setprop debug.barre.layout 1`, restart app).
Never flip at runtime: Yoga's cache (`LayoutResults`) and barre's (blob) would go stale relative to each
other. Debug-only **shadow mode**: after a barre pass, copy the root's tree into a private `yoga::Node`
forest (no clone callback, same measure funcs), run `calculateLayoutYoga`, log frame/`hasNewLayout`
mismatches with the ShadowNode tag — differential testing on real social-app trees.

---

## 4. Profiling plan (argent)

**Hot path** (all `TraceSection`s, which on iOS emit os_signpost intervals under subsystem
`dev.reactnative.instruments`, `cxxreact/TraceSection.h:140`): `ShadowTree::commit` (`ShadowTree.cpp:332`)
→ `RootShadowNode::layoutIfNeeded` → `YogaLayoutableShadowNode::layoutTree` (:595) →
`configureYogaTree` (:601) → `YGNodeCalculateLayout` (:660; **the only part that changes**) →
`yogaNodeCloneCallbackConnector` (:802; clone-on-traverse) and `yogaNodeMeasureCallbackConnector`
(:814; text measurement — same cost per call in both arms, but the *call count* shows cache quality)
→ `YogaLayoutableShadowNode::layout` (metrics copy-back). Also `ParagraphShadowNode` (:430, nested
`layoutTree` for inline views).

**Fairness**: compare **Release** builds (`expo run:ios --configuration Release`, `expo run:android
--variant release`); in iOS Debug the Yoga pod is `-O0` while barre is always optimised (Android
forces Yoga to Release, `ReactCommon/yoga/CMakeLists.txt:12`). Same binary, arm chosen per §3.6.

**Scenario** (deterministic content): e2e mock server + e2e build (`pnpm e2e:mock-server`,
`e2e:build[-android]`, `social-app/package.json:85-88`). Record one argent flow (`argent-create-flow`)
mirroring `social-app/__e2e__/perf-test.yml`: launch → wait for Home feed → 8 momentum-free swipes up
(`PostFeed.tsx:1145-1169`: `FlatList`, `windowSize={9}`, so scrolling mounts/unmounts cells = commits
with fresh subtrees + text measurement) → swipe LEFT×3 / RIGHT×2 between feed tabs (pager mounts whole
feeds) → open a long `PostThread` → back. 5 cold-start runs per arm, interleaved.

**Measure** per arm:
1. `native-profiler-*` (xctrace / Perfetto): self+total time of `yoga::calculateLayout` vs
   `barre_rn_calculate_layout` and of `layoutTree`; UI hangs; `profiler-stack-query thread_breakdown`.
   For the signpost intervals, pass a `template_path` that includes the os_signpost instrument.
2. Counters behind `#ifdef BARRE_RN_STATS` in `BarreHost.cpp`: passes, nodes visited, measure and clone
   callbacks, cache hits per commit — barre must be ≤ Yoga on the same flow.
3. `react-profiler-*` for JS commit timing: should be unchanged (sanity check that both arms did the
   same React work).
4. Memory: `sizeof(yoga::Node)` before/after; RSS at end of flow.
5. Correctness: `screenshot-diff` at fixed scroll offsets between arms; shadow mode (§3.6) on the same
   flow in a debug build.

Targeted cases for the bugs barre fixes: iPad rotation with a `flexBasis: '100%'` child (yoga#1552),
and the Home feed after a font-scale change (stale `computedFlexBasis`, yoga#2019).
