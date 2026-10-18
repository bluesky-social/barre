// barre benchmark: one C++ source, linked once against real Yoga and once
// against barre-ffi. Uses only Yoga's public C API (<yoga/Yoga.h>), so both
// engines run byte-identical workload code. See docs/BENCH.md.
//
//   bench --list
//   bench --run NAME [--time-ms T] [--warmup-ms W] [--captures DIR] [--dump FILE]
//
// --run prints one JSON object on stdout (samples in ns per layout, checksum,
// measure-func call counts). bench/run drives it.

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include <pthread.h>
#include <sys/qos.h>

#include <yoga/Yoga.h>

#include "capture.h"

using Clock = std::chrono::steady_clock;

// ---------------------------------------------------------------------------
// Counters, measure funcs

uint64_t gMeasureCalls = 0;
static uint64_t gCaptureMisses = 0;

// A text-like measure func: `chars` glyphs of width `charW`, wrapped greedily
// to the available width, `lineH` per line. Deterministic and cheap so the
// numbers are about the engine, not the callback; the call count is reported
// separately because in RN every call is a real text layout (~10-30 us).
struct Text {
  float chars;
  float charW;
  float lineH;
};

static YGSize measureText(
    YGNodeConstRef node,
    float width,
    YGMeasureMode widthMode,
    float height,
    YGMeasureMode heightMode) {
  gMeasureCalls++;
  const Text* t = static_cast<const Text*>(YGNodeGetContext(node));
  float natural = t->chars * t->charW;
  float w = natural;
  float lines = 1;
  if (widthMode != YGMeasureModeUndefined && natural > width) {
    float avail = std::max(width, t->charW);
    lines = std::ceil(natural / avail);
    w = avail;
  }
  if (widthMode == YGMeasureModeExactly) {
    w = width;
  }
  float h = lines * t->lineH;
  if (heightMode == YGMeasureModeExactly) {
    h = height;
  } else if (heightMode == YGMeasureModeAtMost) {
    h = std::min(h, height);
  }
  return {w, h};
}

// Yoga's YGBenchmark.c measure func.
static YGSize measureYGBench(
    YGNodeConstRef node,
    float width,
    YGMeasureMode widthMode,
    float height,
    YGMeasureMode heightMode) {
  (void)node;
  gMeasureCalls++;
  return YGSize{
      widthMode == YGMeasureModeUndefined ? 10 : width,
      heightMode == YGMeasureModeUndefined ? 10 : height,
  };
}

// ---------------------------------------------------------------------------
// Checksums and dumps

struct Hasher {
  uint64_t h = 1469598103934665603ull;
  void add(uint32_t v) {
    for (int i = 0; i < 4; i++) {
      h ^= (v >> (8 * i)) & 0xff;
      h *= 1099511628211ull;
    }
  }
  void addf(float f) {
    uint32_t u;
    if (std::isnan(f)) {
      u = 0x7fc00000u;
    } else {
      std::memcpy(&u, &f, 4);
    }
    add(u);
  }
};

static void hashTree(YGNodeConstRef n, Hasher& h, size_t& nodes) {
  nodes++;
  h.addf(YGNodeLayoutGetLeft(n));
  h.addf(YGNodeLayoutGetTop(n));
  h.addf(YGNodeLayoutGetWidth(n));
  h.addf(YGNodeLayoutGetHeight(n));
  size_t c = YGNodeGetChildCount(n);
  h.add(static_cast<uint32_t>(c));
  for (size_t i = 0; i < c; i++) {
    hashTree(YGNodeGetChild(const_cast<YGNodeRef>(n), i), h, nodes);
  }
}

static void dumpTree(YGNodeConstRef n, const std::string& path, FILE* f) {
  fprintf(
      f,
      "%s %.9g %.9g %.9g %.9g\n",
      path.c_str(),
      YGNodeLayoutGetLeft(n),
      YGNodeLayoutGetTop(n),
      YGNodeLayoutGetWidth(n),
      YGNodeLayoutGetHeight(n));
  size_t c = YGNodeGetChildCount(n);
  for (size_t i = 0; i < c; i++) {
    dumpTree(
        YGNodeGetChild(const_cast<YGNodeRef>(n), i),
        path + "/" + std::to_string(i),
        f);
  }
}

// RN's YogaLayoutableShadowNode::layout(): visit children flagged with
// hasNewLayout, clear the flag, read their metrics, recurse only into them.
// Returns the number of nodes whose metrics RN would copy (and fire onLayout
// for) — barre must flag exactly Yoga's set.
static volatile float gSink;
static size_t rnReadMetrics(YGNodeRef n) {
  float acc = YGNodeLayoutGetLeft(n) + YGNodeLayoutGetTop(n) +
      YGNodeLayoutGetWidth(n) + YGNodeLayoutGetHeight(n) +
      static_cast<float>(YGNodeLayoutGetDirection(n));
  for (YGEdge e : {YGEdgeLeft, YGEdgeTop, YGEdgeRight, YGEdgeBottom}) {
    acc += YGNodeLayoutGetBorder(n, e) + YGNodeLayoutGetPadding(n, e);
  }
  gSink = acc;
  return 1;
}

static size_t rnLayoutChildren(YGNodeRef n) {
  size_t count = 0;
  size_t c = YGNodeGetChildCount(n);
  for (size_t i = 0; i < c; i++) {
    YGNodeRef child = YGNodeGetChild(n, i);
    if (YGNodeGetHasNewLayout(child)) {
      YGNodeSetHasNewLayout(child, false);
      count += rnReadMetrics(child);
      if (YGNodeStyleGetDisplay(child) != YGDisplayNone) {
        count += rnLayoutChildren(child);
      }
    }
  }
  return count;
}

static size_t rnLayout(YGNodeRef root) {
  size_t count = 0;
  if (YGNodeGetHasNewLayout(root)) {
    YGNodeSetHasNewLayout(root, false);
    count += rnReadMetrics(root);
  }
  return count + rnLayoutChildren(root);
}

// ---------------------------------------------------------------------------
// Tree building helpers (RN-shaped trees)

struct Rng {
  uint64_t s;
  explicit Rng(uint64_t seed) : s(seed * 2654435761ull + 1) {}
  uint32_t next() {
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    return static_cast<uint32_t>(s >> 11);
  }
  int range(int lo, int hi) { // inclusive
    return lo + static_cast<int>(next() % static_cast<uint32_t>(hi - lo + 1));
  }
};

// RN gives every shadow node its own yoga::Config
// (YogaLayoutableShadowNode::yogaConfig_), point scale 3 on a @3x phone,
// errata masked to what RN 0.88's Yoga knows (docs/RN-SHIM.md).
struct Tree {
  YGNodeRef root = nullptr;
  std::vector<YGConfigRef> configs;
  std::deque<Text> texts;
  std::vector<YGNodeRef> detached; // nodes temporarily removed from the tree
  bool perNodeConfig = true;
  YGConfigRef shared = nullptr;

  YGConfigRef config() {
    if (!perNodeConfig) {
      if (shared == nullptr) {
        shared = newConfig();
      }
      return shared;
    }
    YGConfigRef c = newConfig();
    return c;
  }
  YGConfigRef newConfig() {
    YGConfigRef c = YGConfigNew();
    YGConfigSetPointScaleFactor(c, 3.0f);
    YGConfigSetErrata(c, static_cast<YGErrata>(1 | 2 | 4 | 8));
    configs.push_back(c);
    return c;
  }
  YGNodeRef node() { return YGNodeNewWithConfig(config()); }
  YGNodeRef node(YGNodeRef parent) {
    YGNodeRef n = node();
    YGNodeInsertChild(parent, n, YGNodeGetChildCount(parent));
    return n;
  }
  YGNodeRef text(YGNodeRef parent, float chars, float fontSize = 15) {
    YGNodeRef n = node(parent);
    texts.push_back(Text{chars, fontSize * 0.5f, std::round(fontSize * 1.3f)});
    YGNodeSetContext(n, &texts.back());
    YGNodeSetMeasureFunc(n, measureText);
    return n;
  }
  YGNodeRef box(YGNodeRef parent, float w, float h) {
    YGNodeRef n = node(parent);
    YGNodeStyleSetWidth(n, w);
    YGNodeStyleSetHeight(n, h);
    return n;
  }
  ~Tree() {
    if (root != nullptr) {
      YGNodeFreeRecursive(root);
    }
    for (YGNodeRef n : detached) {
      YGNodeFreeRecursive(n);
    }
    for (YGConfigRef c : configs) {
      YGConfigFree(c);
    }
  }
};

static void row(YGNodeRef n) {
  YGNodeStyleSetFlexDirection(n, YGFlexDirectionRow);
}

// RN's layoutTree: surface constraints become the root's min/max size, then
// YGNodeCalculateLayout(root, maxW, maxH).
static void rnSetSurface(YGNodeRef root, float w, float h) {
  YGNodeStyleSetMinWidth(root, w);
  YGNodeStyleSetMaxWidth(root, w);
  YGNodeStyleSetMinHeight(root, h);
  YGNodeStyleSetMaxHeight(root, h);
}

// Surface root → ScrollView (overflow: scroll) → content container.
static YGNodeRef rnScrollContent(Tree& t) {
  t.root = t.node();
  YGNodeRef scroll = t.node(t.root);
  YGNodeStyleSetFlex(scroll, 1);
  YGNodeStyleSetOverflow(scroll, YGOverflowScroll);
  YGNodeRef content = t.node(scroll);
  YGNodeStyleSetPadding(content, YGEdgeBottom, 100);
  return content;
}

// A social-app-like feed post (PostFeedItem): avatar column, meta row with
// shrinking name/handle, post text, an embed (image with aspect ratio, link
// card, quote post) and an action row with space-between.
struct FeedCell {
  YGNodeRef wrapper;
  YGNodeRef postText;
};

static FeedCell buildPost(Tree& t, YGNodeRef parent, int i) {
  Rng r(static_cast<uint64_t>(i) + 7);
  YGNodeRef wrapper = t.node(parent); // FlatList CellRenderer
  YGNodeRef post = t.node(wrapper);
  YGNodeStyleSetPadding(post, YGEdgeLeft, 10);
  YGNodeStyleSetPadding(post, YGEdgeRight, 15);
  YGNodeStyleSetPadding(post, YGEdgeTop, 8);
  YGNodeStyleSetBorder(post, YGEdgeTop, 1);

  if (i % 5 == 0) { // "Reposted by" header
    YGNodeRef hdr = t.node(post);
    row(hdr);
    YGNodeStyleSetAlignItems(hdr, YGAlignCenter);
    YGNodeStyleSetGap(hdr, YGGutterColumn, 4);
    YGNodeStyleSetPadding(hdr, YGEdgeLeft, 30);
    YGNodeStyleSetMargin(hdr, YGEdgeBottom, 2);
    t.box(hdr, 13, 13);
    YGNodeStyleSetFlexShrink(t.text(hdr, r.range(15, 40), 13), 1);
  }

  YGNodeRef main = t.node(post);
  row(main);
  YGNodeStyleSetGap(main, YGGutterColumn, 10);

  YGNodeRef avatarCol = t.node(main);
  YGNodeStyleSetPadding(avatarCol, YGEdgeLeft, 3);
  t.box(avatarCol, 42, 42);

  YGNodeRef content = t.node(main);
  YGNodeStyleSetFlex(content, 1);

  YGNodeRef meta = t.node(content);
  row(meta);
  YGNodeStyleSetAlignItems(meta, YGAlignCenter);
  YGNodeStyleSetGap(meta, YGGutterColumn, 4);
  YGNodeStyleSetPadding(meta, YGEdgeBottom, 2);
  YGNodeStyleSetFlexShrink(t.text(meta, r.range(6, 24)), 1);
  YGNodeStyleSetFlexShrink(t.text(meta, r.range(10, 30)), 10);
  YGNodeStyleSetFlexShrink(t.text(meta, 1), 0);
  YGNodeStyleSetFlexShrink(t.text(meta, r.range(2, 3)), 0);

  YGNodeRef postText = t.text(content, r.range(10, 300));

  switch (i % 4) {
    case 1: { // image embed
      YGNodeRef wrap = t.node(content);
      YGNodeStyleSetMargin(wrap, YGEdgeTop, 8);
      YGNodeStyleSetOverflow(wrap, YGOverflowHidden);
      YGNodeRef img = t.node(wrap);
      YGNodeStyleSetWidthPercent(img, 100);
      static const float ratios[] = {1.333f, 0.75f, 1.0f, 1.777f};
      YGNodeStyleSetAspectRatio(img, ratios[r.range(0, 3)]);
      break;
    }
    case 2: { // external link card
      YGNodeRef card = t.node(content);
      YGNodeStyleSetMargin(card, YGEdgeTop, 8);
      YGNodeStyleSetBorder(card, YGEdgeAll, 1);
      YGNodeStyleSetOverflow(card, YGOverflowHidden);
      YGNodeRef thumb = t.node(card);
      YGNodeStyleSetWidthPercent(thumb, 100);
      YGNodeStyleSetAspectRatio(thumb, 1.91f);
      YGNodeRef body = t.node(card);
      YGNodeStyleSetPadding(body, YGEdgeAll, 12);
      YGNodeStyleSetGap(body, YGGutterRow, 3);
      t.text(body, r.range(20, 90), 15);
      t.text(body, r.range(40, 200), 13);
      YGNodeRef div = t.node(body);
      YGNodeStyleSetHeight(div, 1);
      YGNodeStyleSetMargin(div, YGEdgeVertical, 6);
      YGNodeRef dom = t.node(body);
      row(dom);
      YGNodeStyleSetAlignItems(dom, YGAlignCenter);
      YGNodeStyleSetGap(dom, YGGutterColumn, 4);
      t.box(dom, 12, 12);
      YGNodeStyleSetFlexShrink(t.text(dom, r.range(8, 30), 12), 1);
      break;
    }
    case 3: { // quote post
      YGNodeRef q = t.node(content);
      YGNodeStyleSetMargin(q, YGEdgeTop, 8);
      YGNodeStyleSetBorder(q, YGEdgeAll, 1);
      YGNodeStyleSetPadding(q, YGEdgeAll, 12);
      YGNodeRef qm = t.node(q);
      row(qm);
      YGNodeStyleSetAlignItems(qm, YGAlignCenter);
      YGNodeStyleSetGap(qm, YGGutterColumn, 4);
      t.box(qm, 16, 16);
      YGNodeStyleSetFlexShrink(t.text(qm, r.range(6, 24)), 1);
      YGNodeStyleSetFlexShrink(t.text(qm, r.range(10, 30)), 10);
      YGNodeStyleSetFlexShrink(t.text(qm, 2), 0);
      YGNodeStyleSetMargin(t.text(q, r.range(20, 200)), YGEdgeTop, 4);
      break;
    }
    default:
      break;
  }

  YGNodeRef actions = t.node(content);
  row(actions);
  YGNodeStyleSetJustifyContent(actions, YGJustifySpaceBetween);
  YGNodeStyleSetAlignItems(actions, YGAlignCenter);
  YGNodeStyleSetMargin(actions, YGEdgeTop, 8);
  YGNodeStyleSetPadding(actions, YGEdgeBottom, 4);
  YGNodeStyleSetPadding(actions, YGEdgeRight, 20);
  for (int b = 0; b < 4; b++) {
    YGNodeRef btn = t.node(actions);
    row(btn);
    YGNodeStyleSetAlignItems(btn, YGAlignCenter);
    YGNodeStyleSetGap(btn, YGGutterColumn, 4);
    YGNodeStyleSetPadding(btn, YGEdgeAll, 5);
    t.box(btn, 18, 18);
    if (b < 3) {
      t.text(btn, r.range(1, 3), 13);
    }
  }
  return {wrapper, postText};
}

struct Feed {
  std::unique_ptr<Tree> t;
  YGNodeRef content;
  std::vector<FeedCell> cells;
};

static Feed buildFeed(int n) {
  Feed f;
  f.t = std::make_unique<Tree>();
  f.content = rnScrollContent(*f.t);
  for (int i = 0; i < n; i++) {
    f.cells.push_back(buildPost(*f.t, f.content, i));
  }
  rnSetSurface(f.t->root, 390, 844);
  return f;
}

// Deep: 25 nested levels alternating row/column, each with a text sibling,
// flexShrink everywhere (the yoga#2007 min-content shape).
static std::unique_ptr<Tree> buildDeep(int depth) {
  auto t = std::make_unique<Tree>();
  t->root = t->node();
  YGNodeRef cur = t->root;
  for (int d = 0; d < depth; d++) {
    YGNodeRef lvl = t->node(cur);
    YGNodeStyleSetFlexGrow(lvl, 1);
    YGNodeStyleSetFlexShrink(lvl, 1);
    YGNodeStyleSetPadding(lvl, YGEdgeAll, 4);
    YGNodeStyleSetMargin(lvl, YGEdgeAll, 2);
    YGNodeStyleSetFlexDirection(
        lvl, d % 2 ? YGFlexDirectionRow : YGFlexDirectionColumn);
    YGNodeStyleSetAlignItems(lvl, d % 3 ? YGAlignStretch : YGAlignFlexStart);
    YGNodeStyleSetFlexShrink(t->text(lvl, 12 + (d * 7) % 30), 1);
    cur = lvl;
  }
  t->text(cur, 80);
  rnSetSurface(t->root, 390, 844);
  return t;
}

// Wide column: a settings-style list of 1000 rows in a ScrollView.
static std::unique_ptr<Tree> buildWideColumn(int n) {
  auto t = std::make_unique<Tree>();
  YGNodeRef content = rnScrollContent(*t);
  Rng r(3);
  for (int i = 0; i < n; i++) {
    YGNodeRef rw = t->node(content);
    row(rw);
    YGNodeStyleSetAlignItems(rw, YGAlignCenter);
    YGNodeStyleSetHeight(rw, 44);
    YGNodeStyleSetPadding(rw, YGEdgeHorizontal, 16);
    YGNodeStyleSetGap(rw, YGGutterColumn, 12);
    t->box(rw, 24, 24);
    YGNodeStyleSetFlex(t->text(rw, r.range(10, 40)), 1);
    t->box(rw, 8, 14);
  }
  rnSetSurface(t->root, 390, 844);
  return t;
}

// Wide row: 200 shrinking measured items on one line.
static std::unique_ptr<Tree> buildWideRow(int n) {
  auto t = std::make_unique<Tree>();
  t->root = t->node();
  row(t->root);
  YGNodeStyleSetAlignItems(t->root, YGAlignFlexStart);
  Rng r(5);
  for (int i = 0; i < n; i++) {
    YGNodeRef c = t->text(t->root, r.range(3, 20));
    YGNodeStyleSetFlexShrink(c, 1);
    YGNodeStyleSetFlexGrow(c, i % 3 == 0 ? 1 : 0);
    YGNodeStyleSetMargin(c, YGEdgeHorizontal, 1);
  }
  rnSetSurface(t->root, 390, 844);
  return t;
}

// Wrap grid: 300 photo tiles (31% wide, square), absolute image fill and a
// caption overlay, with gaps.
static std::unique_ptr<Tree> buildWrapGrid(int n) {
  auto t = std::make_unique<Tree>();
  YGNodeRef content = rnScrollContent(*t);
  row(content);
  YGNodeStyleSetFlexWrap(content, YGWrapWrap);
  YGNodeStyleSetAlignContent(content, YGAlignFlexStart);
  YGNodeStyleSetGap(content, YGGutterAll, 4);
  YGNodeStyleSetPadding(content, YGEdgeHorizontal, 6);
  Rng r(11);
  for (int i = 0; i < n; i++) {
    YGNodeRef tile = t->node(content);
    YGNodeStyleSetWidthPercent(tile, 31);
    YGNodeStyleSetAspectRatio(tile, 1);
    YGNodeRef img = t->node(tile);
    YGNodeStyleSetPositionType(img, YGPositionTypeAbsolute);
    YGNodeStyleSetPosition(img, YGEdgeAll, 0);
    YGNodeRef cap = t->node(tile);
    YGNodeStyleSetPositionType(cap, YGPositionTypeAbsolute);
    YGNodeStyleSetPosition(cap, YGEdgeLeft, 4);
    YGNodeStyleSetPosition(cap, YGEdgeRight, 4);
    YGNodeStyleSetPosition(cap, YGEdgeBottom, 4);
    t->text(cap, r.range(3, 30), 11);
  }
  rnSetSurface(t->root, 390, 844);
  return t;
}

// Absolute overlays: 100 media cards with a badge, a gradient and a centred
// play button (percent insets, negative margins).
static std::unique_ptr<Tree> buildAbsolute(int n) {
  auto t = std::make_unique<Tree>();
  YGNodeRef content = rnScrollContent(*t);
  Rng r(13);
  for (int i = 0; i < n; i++) {
    YGNodeRef card = t->node(content);
    YGNodeStyleSetMargin(card, YGEdgeAll, 8);
    YGNodeRef media = t->node(card);
    YGNodeStyleSetAspectRatio(media, 1.777f);
    YGNodeRef badge = t->node(media);
    YGNodeStyleSetPositionType(badge, YGPositionTypeAbsolute);
    YGNodeStyleSetPosition(badge, YGEdgeTop, 8);
    YGNodeStyleSetPosition(badge, YGEdgeRight, 8);
    YGNodeStyleSetPadding(badge, YGEdgeAll, 4);
    t->text(badge, r.range(2, 6), 11);
    YGNodeRef grad = t->node(media);
    YGNodeStyleSetPositionType(grad, YGPositionTypeAbsolute);
    YGNodeStyleSetPosition(grad, YGEdgeLeft, 0);
    YGNodeStyleSetPosition(grad, YGEdgeRight, 0);
    YGNodeStyleSetPosition(grad, YGEdgeBottom, 0);
    YGNodeStyleSetHeightPercent(grad, 30);
    YGNodeRef play = t->node(media);
    YGNodeStyleSetPositionType(play, YGPositionTypeAbsolute);
    YGNodeStyleSetPositionPercent(play, YGEdgeLeft, 50);
    YGNodeStyleSetPositionPercent(play, YGEdgeTop, 50);
    YGNodeStyleSetMargin(play, YGEdgeLeft, -20);
    YGNodeStyleSetMargin(play, YGEdgeTop, -20);
    YGNodeStyleSetWidth(play, 40);
    YGNodeStyleSetHeight(play, 40);
    t->text(card, r.range(20, 120));
  }
  rnSetSurface(t->root, 390, 844);
  return t;
}

// ---------------------------------------------------------------------------
// Workloads

struct Workload {
  virtual ~Workload() = default;
  // true: prepare/after/cleanup are no-ops, so iterations can be timed in
  // batches (needed for sub-microsecond operations).
  bool batchable = false;
  // true: after() does RN's hasNewLayout readback.
  bool rn = false;
  virtual void setup() {}
  virtual void prepare() {}
  virtual void iter() = 0; // the timed part: normally one YGNodeCalculateLayout
  virtual size_t after() { return 0; } // untimed; returns new-layout nodes
  virtual void cleanup() {}
  virtual YGNodeRef root() { return nullptr; }
  // Default checksum: hash the tree under root().
  virtual bool checksum(Hasher& h, size_t& nodes) {
    if (root() == nullptr) {
      return false;
    }
    hashTree(root(), h, nodes);
    return true;
  }
};

using Factory = std::function<std::unique_ptr<Workload>()>;
static std::vector<std::pair<std::string, Factory>> gWorkloads;
static std::string gCapturesDir;

// --- Yoga's YGBenchmark.c, verbatim blocks (create + layout + free) --------

struct YGBenchBlock : Workload {
  std::function<YGNodeRef()> build;
  explicit YGBenchBlock(std::function<YGNodeRef()> b) : build(std::move(b)) {
    batchable = true;
  }
  void iter() override {
    YGNodeRef root = build();
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    YGNodeFreeRecursive(root);
  }
  bool checksum(Hasher& h, size_t& nodes) override {
    YGNodeRef root = build();
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
    hashTree(root, h, nodes);
    YGNodeFreeRecursive(root);
    return true;
  }
};

static YGNodeRef ygStackWithFlex() {
  const YGNodeRef root = YGNodeNew();
  YGNodeStyleSetWidth(root, 100);
  YGNodeStyleSetHeight(root, 100);
  for (uint32_t i = 0; i < 10; i++) {
    const YGNodeRef child = YGNodeNew();
    YGNodeSetMeasureFunc(child, measureYGBench);
    YGNodeStyleSetFlex(child, 1);
    YGNodeInsertChild(root, child, 0);
  }
  return root;
}

static YGNodeRef ygAlignStretch() {
  const YGNodeRef root = YGNodeNew();
  for (uint32_t i = 0; i < 10; i++) {
    const YGNodeRef child = YGNodeNew();
    YGNodeStyleSetHeight(child, 20);
    YGNodeSetMeasureFunc(child, measureYGBench);
    YGNodeInsertChild(root, child, 0);
  }
  return root;
}

static YGNodeRef ygNestedFlex() {
  const YGNodeRef root = YGNodeNew();
  for (uint32_t i = 0; i < 10; i++) {
    const YGNodeRef child = YGNodeNew();
    YGNodeStyleSetFlex(child, 1);
    YGNodeInsertChild(root, child, 0);
    for (uint32_t ii = 0; ii < 10; ii++) {
      const YGNodeRef grandChild = YGNodeNew();
      YGNodeSetMeasureFunc(grandChild, measureYGBench);
      YGNodeStyleSetFlex(grandChild, 1);
      YGNodeInsertChild(child, grandChild, 0);
    }
  }
  return root;
}

static YGNodeRef ygHugeNested() {
  const YGNodeRef root = YGNodeNew();
  for (uint32_t i = 0; i < 10; i++) {
    const YGNodeRef child = YGNodeNew();
    YGNodeStyleSetFlexGrow(child, 1);
    YGNodeStyleSetWidth(child, 10);
    YGNodeStyleSetHeight(child, 10);
    YGNodeInsertChild(root, child, 0);
    for (uint32_t ii = 0; ii < 10; ii++) {
      const YGNodeRef grandChild = YGNodeNew();
      YGNodeStyleSetFlexDirection(grandChild, YGFlexDirectionRow);
      YGNodeStyleSetFlexGrow(grandChild, 1);
      YGNodeStyleSetWidth(grandChild, 10);
      YGNodeStyleSetHeight(grandChild, 10);
      YGNodeInsertChild(child, grandChild, 0);
      for (uint32_t iii = 0; iii < 10; iii++) {
        const YGNodeRef ggc = YGNodeNew();
        YGNodeStyleSetFlexGrow(ggc, 1);
        YGNodeStyleSetWidth(ggc, 10);
        YGNodeStyleSetHeight(ggc, 10);
        YGNodeInsertChild(grandChild, ggc, 0);
        for (uint32_t iiii = 0; iiii < 10; iiii++) {
          const YGNodeRef gggc = YGNodeNew();
          YGNodeStyleSetFlexDirection(gggc, YGFlexDirectionRow);
          YGNodeStyleSetFlexGrow(gggc, 1);
          YGNodeStyleSetWidth(gggc, 10);
          YGNodeStyleSetHeight(gggc, 10);
          YGNodeInsertChild(ggc, gggc, 0);
        }
      }
    }
  }
  return root;
}

// Layout only, on a fresh tree built (untimed) before each iteration.
struct FreshLayout : Workload {
  std::function<YGNodeRef()> build;
  YGNodeRef r = nullptr;
  explicit FreshLayout(std::function<YGNodeRef()> b) : build(std::move(b)) {}
  void prepare() override { r = build(); }
  void iter() override {
    YGNodeCalculateLayout(r, YGUndefined, YGUndefined, YGDirectionLTR);
  }
  void cleanup() override {
    YGNodeFreeRecursive(r);
    r = nullptr;
  }
  YGNodeRef root() override { return r; }
};

// --- Yoga's benchmark captures (vendor/yoga/benchmark/captures) ------------

struct CaptureLayout : Workload {
  std::string file;
  std::unique_ptr<bench::Capture> cap;
  std::unique_ptr<bench::CaptureTree> tree;
  explicit CaptureLayout(std::string f) : file(std::move(f)) {}
  void setup() override {
    cap = bench::loadCapture(gCapturesDir + "/" + file, &gCaptureMisses);
  }
  void prepare() override { tree = cap->build(); }
  void iter() override {
    YGNodeCalculateLayout(
        tree->root(), cap->availableWidth, cap->availableHeight, cap->direction);
  }
  void cleanup() override { tree.reset(); }
  YGNodeRef root() override { return tree ? tree->root() : nullptr; }
};

struct CaptureClean : CaptureLayout {
  using CaptureLayout::CaptureLayout;
  void setup() override {
    CaptureLayout::setup();
    tree = cap->build();
    CaptureLayout::iter();
    batchable = true;
  }
  void prepare() override {}
  void cleanup() override {}
};

// --- RN scenarios ------------------------------------------------------------

struct RnFresh : Workload {
  std::function<std::unique_ptr<Tree>()> build;
  std::unique_ptr<Tree> t;
  explicit RnFresh(std::function<std::unique_ptr<Tree>()> b)
      : build(std::move(b)) {
    rn = true;
  }
  void prepare() override { t = build(); }
  void iter() override {
    YGNodeCalculateLayout(t->root, 390, 844, YGDirectionLTR);
  }
  size_t after() override { return rnLayout(t->root); }
  void cleanup() override { t.reset(); }
  YGNodeRef root() override { return t ? t->root : nullptr; }
};

struct RnClean : Workload {
  std::function<std::unique_ptr<Tree>()> build;
  std::unique_ptr<Tree> t;
  explicit RnClean(std::function<std::unique_ptr<Tree>()> b)
      : build(std::move(b)) {
    rn = true;
    batchable = true;
  }
  void setup() override {
    t = build();
    YGNodeCalculateLayout(t->root, 390, 844, YGDirectionLTR);
    rnLayout(t->root);
  }
  void iter() override {
    YGNodeCalculateLayout(t->root, 390, 844, YGDirectionLTR);
  }
  size_t after() override { return rnLayout(t->root); }
  YGNodeRef root() override { return t->root; }
};

// A committed feed, then one mutation per iteration (applied untimed in
// prepare), then a timed YGNodeCalculateLayout.
struct FeedRelayout : Workload {
  enum Kind { TextChange, TextSameSize, Rotate, Resize, InsertRemove } kind;
  Feed f;
  uint64_t n = 0;
  float w = 390, h = 844;
  explicit FeedRelayout(Kind k) : kind(k) { rn = true; }
  void setup() override {
    f = buildFeed(50);
    YGNodeCalculateLayout(f.t->root, w, h, YGDirectionLTR);
    rnLayout(f.t->root);
  }
  void prepare() override {
    n++;
    bool odd = n & 1;
    switch (kind) {
      case TextChange: { // 120 -> 200 glyphs: line count changes
        YGNodeRef leaf = f.cells[25].postText;
        static_cast<Text*>(YGNodeGetContext(leaf))->chars = odd ? 200 : 120;
        YGNodeMarkDirty(leaf);
        break;
      }
      case TextSameSize: { // edit that doesn't change the measured size
        YGNodeRef leaf = f.cells[25].postText;
        static_cast<Text*>(YGNodeGetContext(leaf))->chars = odd ? 121 : 120;
        YGNodeMarkDirty(leaf);
        break;
      }
      case Rotate:
        w = odd ? 844 : 390;
        h = odd ? 390 : 844;
        rnSetSurface(f.t->root, w, h);
        break;
      case Resize: // window drag: a new width every layout, 40 distinct
                   // widths so Yoga's 8-entry measure cache can't keep up
        w = 360 + static_cast<float>(n % 40) * 3;
        rnSetSurface(f.t->root, w, h);
        break;
      case InsertRemove: {
        YGNodeRef cell = f.cells[25].wrapper;
        if (odd) {
          YGNodeRemoveChild(f.content, cell);
          f.t->detached.push_back(cell);
        } else {
          f.t->detached.pop_back();
          YGNodeInsertChild(f.content, cell, 25);
        }
        break;
      }
    }
  }
  void iter() override {
    YGNodeCalculateLayout(f.t->root, w, h, YGDirectionLTR);
  }
  size_t after() override { return rnLayout(f.t->root); }
  YGNodeRef root() override { return f.t->root; }
};

// --- Fabric's commit pattern -------------------------------------------------
//
// React Native's Fabric never mutates a committed tree. A commit clones the
// path from the root to the changed node: YogaLayoutableShadowNode's clone
// constructor copy-constructs the yoga::Node (layout results and measure cache
// included), gives it a new yoga::Config with the same settings and the clone
// callback, and swaps the cloned child in. Layout then clones every child of a
// node it lays out whose owner is not that node, through the clone callback
// (Node::cloneChildrenIfNeeded -> yogaNodeCloneCallbackConnector), which does
// the same. So a node's cache entries come from an earlier commit's layout of
// *another* node object, and a detector of repeated (node, constraint) pairs
// within one pass cannot see re-measurement across commits.
//
// The tree: a scroll view of `cells` posts with `flex: 1` columns (one inside
// an auto-height column, the T-0006 shape), three `flex: 1` texts sharing a
// row, text in a card with sub-pixel padding, and a 1/3-pt left padding, so
// text leaves get available widths on the 1/3-pt grid and off it. Each commit
// edits one leaf: odd commits toggle a post text between 120 and 200 glyphs
// (its line count changes), even commits toggle a like count between 1 and 2
// glyphs (its width changes); the edited cell moves on every two commits. The
// edited leaf gets YGNodeMarkDirty, which dirties the cloned path. (Fabric
// calls yoga::Node::setDirty on each cloned node instead, which does not reset
// `computedFlexBasis`; the C API has no equivalent.)
//
// Replaced nodes are freed after each layout once no reachable node names them
// as its owner, so a freed node's address is never mistaken for a live owner
// by cloneChildrenIfNeeded.
struct FabricCommit;
static FabricCommit* gFabric = nullptr;
static YGNodeRef fabricCloneNode(YGNodeConstRef old, YGNodeConstRef, size_t);

struct FabricCommit : Workload {
  int cells;
  std::unique_ptr<Tree> t; // keeps the Text records alive
  YGNodeRef r = nullptr;
  std::vector<YGNodeRef> live; // every node not yet freed
  std::map<YGNodeRef, YGConfigRef> configOf;
  std::vector<std::vector<size_t>> postPaths, likePaths;
  std::deque<Text> alt; // the second Text of every edited leaf
  std::map<const Text*, Text*> other;
  uint64_t n = 0;
  explicit FabricCommit(int c) : cells(c) { rn = true; }

  static YGConfigRef newConfig() {
    YGConfigRef c = YGConfigNew();
    YGConfigSetPointScaleFactor(c, 3.0f);
    YGConfigSetErrata(c, static_cast<YGErrata>(1 | 2 | 4 | 8));
    YGConfigSetCloneNodeFunc(c, fabricCloneNode);
    return c;
  }
  void adopt(YGNodeRef x) {
    YGConfigRef c = newConfig();
    YGNodeSetConfig(x, c);
    live.push_back(x);
    configOf[x] = c;
  }
  YGNodeRef cloneWithConfig(YGNodeConstRef old) {
    YGNodeRef c = YGNodeClone(old);
    adopt(c);
    return c;
  }
  static std::vector<size_t> pathTo(YGNodeRef leaf) {
    std::vector<size_t> p;
    for (YGNodeRef x = leaf; YGNodeGetOwner(x) != nullptr;
         x = YGNodeGetOwner(x)) {
      YGNodeRef o = YGNodeGetOwner(x);
      size_t i = 0;
      while (YGNodeGetChild(o, i) != x) {
        i++;
      }
      p.insert(p.begin(), i);
    }
    return p;
  }
  void addAlt(YGNodeRef leaf, float chars) {
    Text* a = static_cast<Text*>(YGNodeGetContext(leaf));
    alt.push_back(Text{chars, a->charW, a->lineH});
    other[a] = &alt.back();
    other[&alt.back()] = a;
  }

  // Returns {post text, like count}.
  std::pair<YGNodeRef, YGNodeRef> buildCell(Tree& tr, YGNodeRef parent, int i) {
    Rng rng(static_cast<uint64_t>(i) + 101);
    YGNodeRef wrapper = tr.node(parent);
    YGNodeRef post = tr.node(wrapper);
    row(post);
    YGNodeStyleSetPadding(post, YGEdgeLeft, 31.0f / 3.0f);
    YGNodeStyleSetPadding(post, YGEdgeRight, 15);
    YGNodeStyleSetPadding(post, YGEdgeTop, 8);
    YGNodeStyleSetGap(post, YGGutterColumn, 10);
    tr.box(post, 42, 42);
    YGNodeRef content = tr.node(post);
    YGNodeStyleSetFlex(content, 1);

    YGNodeRef meta = tr.node(content);
    row(meta);
    YGNodeStyleSetAlignItems(meta, YGAlignCenter);
    YGNodeStyleSetGap(meta, YGGutterColumn, 4);
    YGNodeStyleSetFlexShrink(tr.text(meta, rng.range(6, 24)), 1);
    YGNodeStyleSetFlexShrink(tr.text(meta, rng.range(10, 30)), 10);
    YGNodeStyleSetFlexShrink(tr.text(meta, 2), 0);

    // A `flex: 1` wrapper in an auto-height column (basis 0, retainable).
    YGNodeRef body = tr.node(content);
    YGNodeStyleSetFlex(body, 1);
    YGNodeRef postText = tr.text(body, 120);
    addAlt(postText, 200);

    // Three `flex: 1` texts sharing a row: widths on the 1/3-pt grid.
    YGNodeRef tags = tr.node(content);
    row(tags);
    YGNodeStyleSetMargin(tags, YGEdgeTop, 4);
    for (int k = 0; k < 3; k++) {
      YGNodeStyleSetFlex(tr.text(tags, rng.range(4, 30), 13), 1);
    }

    // A card with sub-pixel padding: widths off the grid.
    YGNodeRef card = tr.node(content);
    YGNodeStyleSetMargin(card, YGEdgeTop, 8);
    YGNodeStyleSetBorder(card, YGEdgeAll, 1);
    YGNodeStyleSetPadding(card, YGEdgeAll, 12.1f);
    YGNodeRef cardBody = tr.node(card);
    YGNodeStyleSetFlex(cardBody, 1);
    tr.text(cardBody, rng.range(20, 90), 15);
    tr.text(cardBody, rng.range(40, 200), 13);

    YGNodeRef actions = tr.node(content);
    row(actions);
    YGNodeStyleSetJustifyContent(actions, YGJustifySpaceBetween);
    YGNodeStyleSetAlignItems(actions, YGAlignCenter);
    YGNodeStyleSetMargin(actions, YGEdgeTop, 8);
    YGNodeStyleSetPadding(actions, YGEdgeRight, 20);
    YGNodeRef like = nullptr;
    for (int b = 0; b < 3; b++) {
      YGNodeRef btn = tr.node(actions);
      row(btn);
      YGNodeStyleSetAlignItems(btn, YGAlignCenter);
      YGNodeStyleSetGap(btn, YGGutterColumn, 4);
      YGNodeStyleSetPadding(btn, YGEdgeAll, 5);
      tr.box(btn, 18, 18);
      like = tr.text(btn, 1, 13);
    }
    addAlt(like, 2);
    return {postText, like};
  }

  void setup() override {
    gFabric = this;
    t = std::make_unique<Tree>();
    YGNodeRef content = rnScrollContent(*t);
    std::vector<std::pair<YGNodeRef, YGNodeRef>> leaves;
    for (int i = 0; i < cells; i++) {
      leaves.push_back(buildCell(*t, content, i));
    }
    rnSetSurface(t->root, 390, 844);
    // Every node gets its own config with the clone callback (as Fabric's
    // do); from here the nodes are managed by this workload, not the Tree.
    std::vector<YGNodeRef> stack{t->root};
    while (!stack.empty()) {
      YGNodeRef x = stack.back();
      stack.pop_back();
      adopt(x);
      for (size_t i = 0; i < YGNodeGetChildCount(x); i++) {
        stack.push_back(YGNodeGetChild(x, i));
      }
    }
    for (YGConfigRef c : t->configs) {
      YGConfigFree(c);
    }
    t->configs.clear();
    r = t->root;
    t->root = nullptr;
    for (auto& [postText, like] : leaves) {
      postPaths.push_back(pathTo(postText));
      likePaths.push_back(pathTo(like));
    }
    YGNodeCalculateLayout(r, 390, 844, YGDirectionLTR);
    rnLayout(r);
    collect();
  }

  // One commit: clone the root-to-leaf path, edit the leaf, dirty it.
  void prepare() override {
    n++;
    size_t cell = static_cast<size_t>((n / 2) % static_cast<uint64_t>(cells));
    const std::vector<size_t>& path = (n & 1) ? postPaths[cell] : likePaths[cell];
    std::vector<YGNodeRef> old{r};
    for (size_t i : path) {
      old.push_back(YGNodeGetChild(old.back(), i));
    }
    YGNodeRef leaf = cloneWithConfig(old.back());
    YGNodeSetContext(
        leaf, other.at(static_cast<Text*>(YGNodeGetContext(old.back()))));
    YGNodeRef child = leaf;
    for (size_t d = path.size(); d-- > 0;) {
      YGNodeRef p = cloneWithConfig(old[d]);
      YGNodeSwapChild(p, child, path[d]);
      child = p;
    }
    r = child;
    YGNodeMarkDirty(leaf);
  }
  void iter() override { YGNodeCalculateLayout(r, 390, 844, YGDirectionLTR); }
  size_t after() override {
    size_t count = rnLayout(r);
    collect();
    return count;
  }

  // Frees every node unreachable from the root that no reachable node names
  // as its owner.
  void collect() {
    std::map<YGNodeRef, bool> keep;
    std::vector<YGNodeRef> stack{r};
    while (!stack.empty()) {
      YGNodeRef x = stack.back();
      stack.pop_back();
      keep[x] = true;
      for (size_t i = 0; i < YGNodeGetChildCount(x); i++) {
        stack.push_back(YGNodeGetChild(x, i));
      }
    }
    std::vector<YGNodeRef> owners;
    for (auto& [x, _] : keep) {
      if (YGNodeRef o = YGNodeGetOwner(x)) {
        owners.push_back(o);
      }
    }
    for (YGNodeRef o : owners) {
      keep[o] = true;
    }
    std::vector<YGNodeRef> still;
    for (YGNodeRef x : live) {
      if (keep.count(x) != 0) {
        still.push_back(x);
      } else {
        YGNodeFinalize(x);
        YGConfigFree(configOf.at(x));
        configOf.erase(x);
      }
    }
    live.swap(still);
  }
  YGNodeRef root() override { return r; }
  ~FabricCommit() override {
    for (YGNodeRef x : live) {
      YGNodeFinalize(x);
      YGConfigFree(configOf.at(x));
    }
    gFabric = nullptr;
  }
};

static YGNodeRef fabricCloneNode(YGNodeConstRef old, YGNodeConstRef, size_t) {
  return gFabric->cloneWithConfig(old);
}

// --- Micro: C API costs -------------------------------------------------------

struct MicroNodeNewFree : Workload {
  YGConfigRef c = nullptr;
  MicroNodeNewFree() { batchable = true; }
  void setup() override { c = YGConfigNew(); }
  void iter() override { YGNodeFree(YGNodeNewWithConfig(c)); }
  bool checksum(Hasher&, size_t&) override { return false; }
};

struct MicroStyleSet : Workload {
  YGNodeRef n = nullptr;
  uint64_t i = 0;
  MicroStyleSet() { batchable = true; }
  void setup() override { n = YGNodeNew(); }
  void iter() override { // 20 setters; values alternate so every set changes
    float v = (i++ & 1) ? 1.0f : 2.0f;
    YGNodeStyleSetFlexDirection(
        n, v > 1 ? YGFlexDirectionRow : YGFlexDirectionColumn);
    YGNodeStyleSetAlignItems(n, v > 1 ? YGAlignCenter : YGAlignStretch);
    YGNodeStyleSetJustifyContent(
        n, v > 1 ? YGJustifySpaceBetween : YGJustifyFlexStart);
    YGNodeStyleSetFlexGrow(n, v);
    YGNodeStyleSetFlexShrink(n, v);
    YGNodeStyleSetWidth(n, 100 * v);
    YGNodeStyleSetHeightPercent(n, 10 * v);
    YGNodeStyleSetMinWidth(n, v);
    YGNodeStyleSetMaxWidth(n, 1000 * v);
    YGNodeStyleSetPadding(n, YGEdgeLeft, v);
    YGNodeStyleSetPadding(n, YGEdgeRight, v);
    YGNodeStyleSetPadding(n, YGEdgeTop, v);
    YGNodeStyleSetPadding(n, YGEdgeBottom, v);
    YGNodeStyleSetMargin(n, YGEdgeHorizontal, v);
    YGNodeStyleSetMargin(n, YGEdgeVertical, v);
    YGNodeStyleSetBorder(n, YGEdgeAll, v);
    YGNodeStyleSetGap(n, YGGutterColumn, v);
    YGNodeStyleSetAspectRatio(n, v);
    YGNodeStyleSetPosition(n, YGEdgeTop, v);
    YGNodeStyleSetOverflow(n, v > 1 ? YGOverflowHidden : YGOverflowVisible);
  }
  bool checksum(Hasher&, size_t&) override { return false; }
};

struct MicroStyleGet : Workload {
  YGNodeRef n = nullptr;
  MicroStyleGet() { batchable = true; }
  void setup() override {
    n = YGNodeNew();
    YGNodeStyleSetWidth(n, 10);
    YGNodeStyleSetPadding(n, YGEdgeAll, 3);
  }
  void iter() override {
    float acc = 0;
    acc += YGNodeStyleGetWidth(n).value + YGNodeStyleGetHeight(n).value;
    acc += YGNodeStyleGetMinWidth(n).value + YGNodeStyleGetMaxWidth(n).value;
    acc += YGNodeStyleGetFlexGrow(n) + YGNodeStyleGetFlexShrink(n);
    acc += YGNodeStyleGetFlexBasis(n).value + YGNodeStyleGetAspectRatio(n);
    for (YGEdge e : {YGEdgeLeft, YGEdgeTop, YGEdgeRight, YGEdgeBottom}) {
      acc += YGNodeStyleGetPadding(n, e).value +
          YGNodeStyleGetMargin(n, e).value + YGNodeStyleGetBorder(n, e);
    }
    gSink = acc;
  }
  bool checksum(Hasher&, size_t&) override { return false; }
};

struct MicroBuildFeed : Workload {
  MicroBuildFeed() { batchable = true; }
  void iter() override { Feed f = buildFeed(50); }
  bool checksum(Hasher& h, size_t& nodes) override {
    Feed f = buildFeed(50);
    YGNodeCalculateLayout(f.t->root, 390, 844, YGDirectionLTR);
    hashTree(f.t->root, h, nodes);
    return true;
  }
};

struct MicroInsertRemove : Workload {
  YGNodeRef parent = nullptr, child = nullptr;
  MicroInsertRemove() { batchable = true; }
  void setup() override {
    parent = YGNodeNew();
    for (int i = 0; i < 50; i++) {
      YGNodeInsertChild(parent, YGNodeNew(), static_cast<size_t>(i));
    }
    child = YGNodeNew();
  }
  void iter() override {
    YGNodeInsertChild(parent, child, 25);
    YGNodeRemoveChild(parent, child);
  }
  bool checksum(Hasher&, size_t&) override { return false; }
};

struct MicroLayoutGetAll : Workload {
  Feed f;
  MicroLayoutGetAll() { batchable = true; }
  void setup() override {
    f = buildFeed(50);
    YGNodeCalculateLayout(f.t->root, 390, 844, YGDirectionLTR);
  }
  static float walk(YGNodeRef n) {
    float acc = YGNodeLayoutGetLeft(n) + YGNodeLayoutGetTop(n) +
        YGNodeLayoutGetWidth(n) + YGNodeLayoutGetHeight(n);
    size_t c = YGNodeGetChildCount(n);
    for (size_t i = 0; i < c; i++) {
      acc += walk(YGNodeGetChild(n, i));
    }
    return acc;
  }
  void iter() override { gSink = walk(f.t->root); }
  bool checksum(Hasher&, size_t&) override { return false; }
};

static void registerWorkloads() {
  auto add = [](std::string name, Factory f) {
    gWorkloads.emplace_back(std::move(name), std::move(f));
  };
  // Yoga's YGBenchmark.c (each sample: build + layout + free, as Yoga times it)
  add("yoga/stack_with_flex",
      [] { return std::make_unique<YGBenchBlock>(ygStackWithFlex); });
  add("yoga/align_stretch_undefined_axis",
      [] { return std::make_unique<YGBenchBlock>(ygAlignStretch); });
  add("yoga/nested_flex",
      [] { return std::make_unique<YGBenchBlock>(ygNestedFlex); });
  add("yoga/huge_nested",
      [] { return std::make_unique<YGBenchBlock>(ygHugeNested); });
  add("yoga/huge_nested.layout_only",
      [] { return std::make_unique<FreshLayout>(ygHugeNested); });
  // Yoga's captured trees (Benchmark.cpp), measure funcs replayed without the
  // recorded sleep.
  for (const char* c :
       {"feed-android", "profile-ios", "rendering-sample-mac", "chat-mac"}) {
    std::string file = std::string(c) + ".json";
    add(std::string("capture/") + c,
        [file] { return std::make_unique<CaptureLayout>(file); });
    add(std::string("capture/") + c + ".clean",
        [file] { return std::make_unique<CaptureClean>(file); });
  }
  // RN-shaped trees
  add("rn/feed50/fresh", [] {
    return std::make_unique<RnFresh>([] { return std::move(buildFeed(50).t); });
  });
  add("rn/feed50/clean", [] {
    return std::make_unique<RnClean>([] { return std::move(buildFeed(50).t); });
  });
  add("rn/feed50/text_change",
      [] { return std::make_unique<FeedRelayout>(FeedRelayout::TextChange); });
  add("rn/feed50/text_same_size",
      [] { return std::make_unique<FeedRelayout>(FeedRelayout::TextSameSize); });
  add("rn/feed50/rotate",
      [] { return std::make_unique<FeedRelayout>(FeedRelayout::Rotate); });
  add("rn/feed50/resize",
      [] { return std::make_unique<FeedRelayout>(FeedRelayout::Resize); });
  add("rn/feed50/insert_remove", [] {
    return std::make_unique<FeedRelayout>(FeedRelayout::InsertRemove);
  });
  // Fabric's commits: clone the root-to-leaf path, edit one text, relayout.
  add("rn/fabric50/commit", [] { return std::make_unique<FabricCommit>(50); });
  // Depth scaling: measure calls grow geometrically with depth in this shape
  // (alternating row/column, shrinkable measured siblings).
  for (int d : {5, 10, 15, 20}) {
    add("rn/deep" + std::to_string(d) + "/fresh", [d] {
      return std::make_unique<RnFresh>([d] { return buildDeep(d); });
    });
  }
  add("rn/deep20/clean",
      [] { return std::make_unique<RnClean>([] { return buildDeep(20); }); });
  add("rn/wide_column1000/fresh", [] {
    return std::make_unique<RnFresh>([] { return buildWideColumn(1000); });
  });
  add("rn/wide_row200/fresh",
      [] { return std::make_unique<RnFresh>([] { return buildWideRow(200); }); });
  add("rn/wrap_grid300/fresh", [] {
    return std::make_unique<RnFresh>([] { return buildWrapGrid(300); });
  });
  add("rn/absolute100/fresh", [] {
    return std::make_unique<RnFresh>([] { return buildAbsolute(100); });
  });
  // Micro: C API call costs
  add("micro/node_new_free", [] { return std::make_unique<MicroNodeNewFree>(); });
  add("micro/style_set_x20", [] { return std::make_unique<MicroStyleSet>(); });
  add("micro/style_get_x20", [] { return std::make_unique<MicroStyleGet>(); });
  add("micro/insert_remove_child",
      [] { return std::make_unique<MicroInsertRemove>(); });
  add("micro/build_feed50_tree",
      [] { return std::make_unique<MicroBuildFeed>(); });
  add("micro/layout_get_feed50",
      [] { return std::make_unique<MicroLayoutGetAll>(); });
}

// ---------------------------------------------------------------------------
// Driver

static int64_t nowNs() {
  return std::chrono::duration_cast<std::chrono::nanoseconds>(
             Clock::now().time_since_epoch())
      .count();
}

int main(int argc, char** argv) {
  pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
  registerWorkloads();

  std::string run, dump;
  double timeMs = 300, warmupMs = 100;
  int checkIters = 4;
  size_t maxSamples = 20000;
  for (int i = 1; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() -> std::string {
      if (i + 1 >= argc) {
        fprintf(stderr, "missing value for %s\n", a.c_str());
        exit(2);
      }
      return argv[++i];
    };
    if (a == "--list") {
      for (auto& [name, _] : gWorkloads) {
        printf("%s\n", name.c_str());
      }
      return 0;
    } else if (a == "--run") {
      run = next();
    } else if (a == "--time-ms") {
      timeMs = std::stod(next());
    } else if (a == "--warmup-ms") {
      warmupMs = std::stod(next());
    } else if (a == "--captures") {
      gCapturesDir = next();
    } else if (a == "--dump") {
      dump = next();
    } else if (a == "--max-samples") {
      maxSamples = std::stoul(next());
    } else {
      fprintf(stderr, "unknown argument %s\n", a.c_str());
      return 2;
    }
  }
  Factory factory;
  for (auto& [name, f] : gWorkloads) {
    if (name == run) {
      factory = f;
    }
  }
  if (!factory) {
    fprintf(stderr, "unknown workload '%s' (see --list)\n", run.c_str());
    return 2;
  }

  std::unique_ptr<Workload> w = factory();
  w->setup();

  // Check phase: a fixed number of iterations, identical on both engines, so
  // the checksum and the per-layout callback counts are comparable.
  Hasher h;
  size_t nodes = 0;
  bool hasChecksum = true;
  std::vector<uint64_t> measurePerIter, newLayoutPerIter;
  for (int i = 0; i < checkIters; i++) {
    w->prepare();
    uint64_t m0 = gMeasureCalls;
    w->iter();
    measurePerIter.push_back(gMeasureCalls - m0);
    newLayoutPerIter.push_back(w->after());
    if (i == checkIters - 1 && !dump.empty() && w->root() != nullptr) {
      FILE* f = fopen(dump.c_str(), "w");
      if (f != nullptr) {
        dumpTree(w->root(), "", f);
        fclose(f);
      }
    }
    size_t n = 0;
    hasChecksum = w->checksum(h, n) && hasChecksum;
    nodes = n;
    w->cleanup();
  }
  uint64_t missesAfterCheck = gCaptureMisses;

  // Warmup + calibration.
  int64_t t0 = nowNs();
  uint64_t warmIters = 0;
  int64_t warmIterNs = 0;
  while (nowNs() - t0 < static_cast<int64_t>(warmupMs * 1e6) || warmIters < 3) {
    w->prepare();
    int64_t a = nowNs();
    w->iter();
    warmIterNs += nowNs() - a;
    w->after();
    w->cleanup();
    warmIters++;
  }
  (void)warmIterNs;
  // Batchable workloads: double the batch until one batch spans >= 20 us, so
  // timer overhead (~20 ns per read, 41.7 ns ticks on Apple silicon) stays
  // below ~0.2%. Others are timed one iteration at a time (each >= ~10 us).
  uint64_t batch = 1;
  if (w->batchable) {
    for (;;) {
      int64_t a = nowNs();
      for (uint64_t k = 0; k < batch; k++) {
        w->iter();
      }
      if (nowNs() - a >= 20000 || batch >= (1u << 24)) {
        break;
      }
      batch *= 2;
    }
  }

  std::vector<double> samples;
  uint64_t timedLayouts = 0;
  uint64_t measureTimed = 0;
  int64_t start = nowNs();
  const int64_t budget = static_cast<int64_t>(timeMs * 1e6);
  while ((nowNs() - start < budget || samples.size() < 5) &&
         samples.size() < maxSamples) {
    if (batch == 1) {
      w->prepare();
      uint64_t m0 = gMeasureCalls;
      int64_t a = nowNs();
      w->iter();
      int64_t b = nowNs();
      measureTimed += gMeasureCalls - m0;
      w->after();
      w->cleanup();
      samples.push_back(static_cast<double>(b - a));
    } else {
      uint64_t m0 = gMeasureCalls;
      int64_t a = nowNs();
      for (uint64_t k = 0; k < batch; k++) {
        w->iter();
      }
      int64_t b = nowNs();
      measureTimed += gMeasureCalls - m0;
      samples.push_back(static_cast<double>(b - a) / static_cast<double>(batch));
    }
    timedLayouts += batch;
  }
  bool isRn = w->rn;
  w.reset();

  char hex[17];
  snprintf(hex, sizeof hex, "%016llx", static_cast<unsigned long long>(h.h));
  printf("{\"name\":\"%s\",\"batch\":%llu,\"iters\":%llu,", run.c_str(),
         static_cast<unsigned long long>(batch),
         static_cast<unsigned long long>(timedLayouts));
  printf("\"checksum\":%s%s%s,\"nodes\":%zu,", hasChecksum ? "\"" : "",
         hasChecksum ? hex : "null", hasChecksum ? "\"" : "", nodes);
  auto list = [](const std::vector<uint64_t>& v) {
    std::string s = "[";
    for (size_t i = 0; i < v.size(); i++) {
      s += (i ? "," : "") + std::to_string(v[i]);
    }
    return s + "]";
  };
  printf("\"measure_check\":%s,\"new_layout_check\":%s,\"rn\":%s,",
         list(measurePerIter).c_str(), list(newLayoutPerIter).c_str(),
         isRn ? "true" : "false");
  printf("\"measure_timed_per_iter\":%.4f,\"capture_misses\":%llu,",
         static_cast<double>(measureTimed) / static_cast<double>(timedLayouts),
         static_cast<unsigned long long>(missesAfterCheck));
  printf("\"samples_ns\":[");
  for (size_t i = 0; i < samples.size(); i++) {
    printf(i ? ",%.1f" : "%.1f", samples[i]);
  }
  printf("]}\n");
  return 0;
}
