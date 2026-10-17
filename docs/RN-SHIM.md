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
