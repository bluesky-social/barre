// relayout == fresh fuzzer for barre's C ABI (docs/FUZZ.md).
//
// A case is a tree description, a sequence of root sizes, a resize mode, a
// point scale factor and errata. The tree is laid out at each size in turn
// and, after every step, compared with a fresh tree built from the same
// description and laid out directly at that size. Any difference in
// left/top/width/height beyond 1e-3, anywhere in the tree, is a failure.
//
// Cases come from a seed (deterministic generator, the scratch fuzzer's at the
// default knobs, so its seed numbers carry over) or from a .case file. The
// minimiser is in-process delta debugging over the description: every
// candidate is re-run and kept only if the same failure still occurs.
//
// Built by ./fuzz (fuzzer/fuzz.py) against barre-ffi, or real Yoga with --yoga.

#include <yoga/Yoga.h>

#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <functional>
#include <limits>
#include <map>
#include <random>
#include <set>
#include <sstream>
#include <string>
#include <vector>

// ---------------------------------------------------------------------------
// Tree description

enum Field {
  F_DIR, F_FLEX, F_GROW, F_SHRINK, F_BASIS, F_W, F_H, F_MINW, F_MINH, F_MAXW, F_MAXH,
  F_PAD, F_MAR, F_OVF, F_AI, F_AS, F_JC, F_AC, F_WRAP, F_AR, F_GAP, F_POS, F_TEXT, NF
};
enum Kind { K_NUM, K_LEN, K_ENUM };

struct EnumName {
  const char* name;
  const char* cid;
  int value;
};

static const EnumName kDir[] = {{"column", "YGFlexDirectionColumn", YGFlexDirectionColumn},
                                {"column-reverse", "YGFlexDirectionColumnReverse", YGFlexDirectionColumnReverse},
                                {"row", "YGFlexDirectionRow", YGFlexDirectionRow},
                                {"row-reverse", "YGFlexDirectionRowReverse", YGFlexDirectionRowReverse},
                                {nullptr, nullptr, 0}};
static const EnumName kOverflow[] = {{"visible", "YGOverflowVisible", YGOverflowVisible},
                                     {"hidden", "YGOverflowHidden", YGOverflowHidden},
                                     {"scroll", "YGOverflowScroll", YGOverflowScroll},
                                     {nullptr, nullptr, 0}};
static const EnumName kAlign[] = {{"auto", "YGAlignAuto", YGAlignAuto},
                                  {"flex-start", "YGAlignFlexStart", YGAlignFlexStart},
                                  {"center", "YGAlignCenter", YGAlignCenter},
                                  {"flex-end", "YGAlignFlexEnd", YGAlignFlexEnd},
                                  {"stretch", "YGAlignStretch", YGAlignStretch},
                                  {"baseline", "YGAlignBaseline", YGAlignBaseline},
                                  {"space-between", "YGAlignSpaceBetween", YGAlignSpaceBetween},
                                  {"space-around", "YGAlignSpaceAround", YGAlignSpaceAround},
                                  {"space-evenly", "YGAlignSpaceEvenly", YGAlignSpaceEvenly},
                                  {nullptr, nullptr, 0}};
static const EnumName kJustify[] = {{"flex-start", "YGJustifyFlexStart", YGJustifyFlexStart},
                                    {"center", "YGJustifyCenter", YGJustifyCenter},
                                    {"flex-end", "YGJustifyFlexEnd", YGJustifyFlexEnd},
                                    {"space-between", "YGJustifySpaceBetween", YGJustifySpaceBetween},
                                    {"space-around", "YGJustifySpaceAround", YGJustifySpaceAround},
                                    {"space-evenly", "YGJustifySpaceEvenly", YGJustifySpaceEvenly},
                                    {nullptr, nullptr, 0}};
static const EnumName kWrap[] = {{"nowrap", "YGWrapNoWrap", YGWrapNoWrap},
                                 {"wrap", "YGWrapWrap", YGWrapWrap},
                                 {"wrap-reverse", "YGWrapWrapReverse", YGWrapWrapReverse},
                                 {nullptr, nullptr, 0}};
static const EnumName kPosition[] = {{"static", "YGPositionTypeStatic", YGPositionTypeStatic},
                                     {"relative", "YGPositionTypeRelative", YGPositionTypeRelative},
                                     {"absolute", "YGPositionTypeAbsolute", YGPositionTypeAbsolute},
                                     {nullptr, nullptr, 0}};

struct FieldInfo {
  const char* key;
  Kind kind;
  const EnumName* names;
  const char* label;  // for diffs and docs
};

// Indexed by Field. Keys are what .case files use.
static const FieldInfo kFields[NF] = {
    {"dir", K_ENUM, kDir, "flexDirection"},
    {"flex", K_NUM, nullptr, "flex"},
    {"grow", K_NUM, nullptr, "flexGrow"},
    {"shrink", K_NUM, nullptr, "flexShrink"},
    {"basis", K_LEN, nullptr, "flexBasis"},
    {"w", K_LEN, nullptr, "width"},
    {"h", K_LEN, nullptr, "height"},
    {"minw", K_LEN, nullptr, "minWidth"},
    {"minh", K_LEN, nullptr, "minHeight"},
    {"maxw", K_LEN, nullptr, "maxWidth"},
    {"maxh", K_LEN, nullptr, "maxHeight"},
    {"pad", K_LEN, nullptr, "padding (all edges)"},
    {"mar", K_LEN, nullptr, "margin (all edges)"},
    {"ovf", K_ENUM, kOverflow, "overflow"},
    {"ai", K_ENUM, kAlign, "alignItems"},
    {"as", K_ENUM, kAlign, "alignSelf"},
    {"jc", K_ENUM, kJustify, "justifyContent"},
    {"ac", K_ENUM, kAlign, "alignContent"},
    {"wrap", K_ENUM, kWrap, "flexWrap"},
    {"ar", K_NUM, nullptr, "aspectRatio"},
    {"gap", K_NUM, nullptr, "gap (all gutters)"},
    {"pos", K_ENUM, kPosition, "positionType"},
    {"text", K_NUM, nullptr, "measure func: text of this length"},
};

struct Val {
  bool set = false;
  bool pct = false;
  float v = 0;
};

struct NodeDesc {
  Val f[NF];
  std::vector<NodeDesc> kids;
  int id = -1;  // pre-order index when the case was made; kept by the minimiser
};

static int assignIds(NodeDesc& d, int next = 0) {
  d.id = next++;
  for (auto& k : d.kids) next = assignIds(k, next);
  return next;
}

struct Case {
  NodeDesc root;
  std::vector<std::pair<float, float>> sizes;
  int mode = 0;
  float scale = 0;
  int errata = YGErrataAll;
};

static const char* kFieldNames[4] = {"left", "top", "width", "height"};

static const float kDefaultSizes[][2] = {{400, 800}, {800, 400}, {400, 800}, {320, 640},
                                         {1000, 500}, {400, 800}, {401, 800}};

static int countNodes(const NodeDesc& d) {
  int n = 1;
  for (auto& k : d.kids) n += countNodes(k);
  return n;
}

// ---------------------------------------------------------------------------
// Building and laying out a tree through the C API

struct TextCtx {
  float len;
  int calls;  // measure callbacks since the counter was last reset
};

static YGSize measureText(YGNodeConstRef n, float w, YGMeasureMode wm, float h, YGMeasureMode hm) {
  auto* t = static_cast<TextCtx*>(YGNodeGetContext(n));
  t->calls++;
  float len = t->len;
  float maxW = (wm == YGMeasureModeUndefined) ? std::numeric_limits<float>::infinity() : w;
  float width = std::isfinite(maxW) ? std::fmin(maxW, len) : len;
  float lines = width > 0 ? std::ceil(len / width) : 1.0f;
  YGSize out{width, lines * 20.0f};
  if (wm == YGMeasureModeExactly) out.width = w;
  if (hm == YGMeasureModeExactly)
    out.height = h;
  else if (hm == YGMeasureModeAtMost && out.height > h)
    out.height = h;
  return out;
}

static void applyField(YGNodeRef n, int f, const Val& v) {
  float x = v.v;
  switch (f) {
    case F_DIR: YGNodeStyleSetFlexDirection(n, (YGFlexDirection)(int)x); break;
    case F_FLEX: YGNodeStyleSetFlex(n, x); break;
    case F_GROW: YGNodeStyleSetFlexGrow(n, x); break;
    case F_SHRINK: YGNodeStyleSetFlexShrink(n, x); break;
    case F_BASIS: v.pct ? YGNodeStyleSetFlexBasisPercent(n, x) : YGNodeStyleSetFlexBasis(n, x); break;
    case F_W: v.pct ? YGNodeStyleSetWidthPercent(n, x) : YGNodeStyleSetWidth(n, x); break;
    case F_H: v.pct ? YGNodeStyleSetHeightPercent(n, x) : YGNodeStyleSetHeight(n, x); break;
    case F_MINW: v.pct ? YGNodeStyleSetMinWidthPercent(n, x) : YGNodeStyleSetMinWidth(n, x); break;
    case F_MINH: v.pct ? YGNodeStyleSetMinHeightPercent(n, x) : YGNodeStyleSetMinHeight(n, x); break;
    case F_MAXW: v.pct ? YGNodeStyleSetMaxWidthPercent(n, x) : YGNodeStyleSetMaxWidth(n, x); break;
    case F_MAXH: v.pct ? YGNodeStyleSetMaxHeightPercent(n, x) : YGNodeStyleSetMaxHeight(n, x); break;
    case F_PAD:
      v.pct ? YGNodeStyleSetPaddingPercent(n, YGEdgeAll, x) : YGNodeStyleSetPadding(n, YGEdgeAll, x);
      break;
    case F_MAR:
      v.pct ? YGNodeStyleSetMarginPercent(n, YGEdgeAll, x) : YGNodeStyleSetMargin(n, YGEdgeAll, x);
      break;
    case F_OVF: YGNodeStyleSetOverflow(n, (YGOverflow)(int)x); break;
    case F_AI: YGNodeStyleSetAlignItems(n, (YGAlign)(int)x); break;
    case F_AS: YGNodeStyleSetAlignSelf(n, (YGAlign)(int)x); break;
    case F_JC: YGNodeStyleSetJustifyContent(n, (YGJustify)(int)x); break;
    case F_AC: YGNodeStyleSetAlignContent(n, (YGAlign)(int)x); break;
    case F_WRAP: YGNodeStyleSetFlexWrap(n, (YGWrap)(int)x); break;
    case F_AR: YGNodeStyleSetAspectRatio(n, x); break;
    case F_GAP: YGNodeStyleSetGap(n, YGGutterAll, x); break;
    case F_POS: YGNodeStyleSetPositionType(n, (YGPositionType)(int)x); break;
    case F_TEXT: break;  // handled by the builder
  }
}

static std::string fmtNum(float x) {
  char buf[64];
  snprintf(buf, sizeof buf, "%.9g", x);
  return buf;
}

static const EnumName* enumByValue(const EnumName* names, int value) {
  for (const EnumName* e = names; e->name; e++)
    if (e->value == value) return e;
  return nullptr;
}

// The C statement that applyField makes, for snippets.
static std::string fieldCall(int f, const Val& v, const std::string& var) {
  std::string x = fmtNum(v.v);
  auto len = [&](const char* setter) {
    return std::string(setter) + (v.pct ? "Percent(" : "(") + var + ", " + x + ");";
  };
  auto edge = [&](const char* setter) {
    return std::string(setter) + (v.pct ? "Percent(" : "(") + var + ", YGEdgeAll, " + x + ");";
  };
  auto en = [&](const char* setter) {
    const EnumName* e = enumByValue(kFields[f].names, (int)v.v);
    return std::string(setter) + "(" + var + ", " + (e ? e->cid : x.c_str()) + ");";
  };
  switch (f) {
    case F_DIR: return en("YGNodeStyleSetFlexDirection");
    case F_FLEX: return "YGNodeStyleSetFlex(" + var + ", " + x + ");";
    case F_GROW: return "YGNodeStyleSetFlexGrow(" + var + ", " + x + ");";
    case F_SHRINK: return "YGNodeStyleSetFlexShrink(" + var + ", " + x + ");";
    case F_BASIS: return len("YGNodeStyleSetFlexBasis");
    case F_W: return len("YGNodeStyleSetWidth");
    case F_H: return len("YGNodeStyleSetHeight");
    case F_MINW: return len("YGNodeStyleSetMinWidth");
    case F_MINH: return len("YGNodeStyleSetMinHeight");
    case F_MAXW: return len("YGNodeStyleSetMaxWidth");
    case F_MAXH: return len("YGNodeStyleSetMaxHeight");
    case F_PAD: return edge("YGNodeStyleSetPadding");
    case F_MAR: return edge("YGNodeStyleSetMargin");
    case F_OVF: return en("YGNodeStyleSetOverflow");
    case F_AI: return en("YGNodeStyleSetAlignItems");
    case F_AS: return en("YGNodeStyleSetAlignSelf");
    case F_JC: return en("YGNodeStyleSetJustifyContent");
    case F_AC: return en("YGNodeStyleSetAlignContent");
    case F_WRAP: return en("YGNodeStyleSetFlexWrap");
    case F_AR: return "YGNodeStyleSetAspectRatio(" + var + ", " + x + ");";
    case F_GAP: return "YGNodeStyleSetGap(" + var + ", YGGutterAll, " + x + ");";
    case F_POS: return en("YGNodeStyleSetPositionType");
    case F_TEXT:
      return "YGNodeSetContext(" + var + ", (void*)(intptr_t)" + x + "); YGNodeSetMeasureFunc(" + var +
             ", measureText);";
  }
  return "";
}

struct Built {
  YGNodeRef root = nullptr;
  std::deque<TextCtx> texts;

  Built(const NodeDesc& d, YGConfigRef cfg) { root = build(d, cfg); }
  ~Built() { YGNodeFreeRecursive(root); }
  Built(const Built&) = delete;
  Built& operator=(const Built&) = delete;

  YGNodeRef build(const NodeDesc& d, YGConfigRef cfg) {
    YGNodeRef n = YGNodeNewWithConfig(cfg);
    for (int f = 0; f < NF; f++)
      if (d.f[f].set) applyField(n, f, d.f[f]);
    if (d.f[F_TEXT].set && d.kids.empty()) {
      texts.push_back({d.f[F_TEXT].v, 0});
      YGNodeSetContext(n, &texts.back());
      YGNodeSetMeasureFunc(n, measureText);
    }
    for (size_t i = 0; i < d.kids.size(); i++) YGNodeInsertChild(n, build(d.kids[i], cfg), i);
    return n;
  }

  int sumCalls() {
    int sum = 0;
    for (auto& t : texts) sum += t.calls;
    return sum;
  }

  int resetCalls() {
    int m = 0;
    for (auto& t : texts) {
      m = std::max(m, t.calls);
      t.calls = 0;
    }
    return m;
  }
};

static void layoutAt(YGNodeRef root, int mode, float w, float h) {
  if (mode == 0) {
    YGNodeStyleSetMinWidth(root, w);
    YGNodeStyleSetMaxWidth(root, w);
    YGNodeStyleSetMinHeight(root, h);
    YGNodeStyleSetMaxHeight(root, h);
    YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
  } else if (mode == 1) {
    YGNodeCalculateLayout(root, w, h, YGDirectionLTR);
  } else {
    YGNodeStyleSetWidth(root, w);
    YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);
  }
}

struct Diff {
  std::string path;
  int field;
  float relayout, fresh;
  int node = -1;  // NodeDesc::id
};

static void diffTrees(YGNodeRef a, YGNodeRef b, const NodeDesc& d, const std::string& path, std::vector<Diff>& out,
                      size_t cap) {
  float va[4] = {YGNodeLayoutGetLeft(a), YGNodeLayoutGetTop(a), YGNodeLayoutGetWidth(a), YGNodeLayoutGetHeight(a)};
  float vb[4] = {YGNodeLayoutGetLeft(b), YGNodeLayoutGetTop(b), YGNodeLayoutGetWidth(b), YGNodeLayoutGetHeight(b)};
  for (int i = 0; i < 4; i++) {
    // equal infinities are the same (their difference is NaN)
    bool same = va[i] == vb[i] || (std::isnan(va[i]) && std::isnan(vb[i])) || std::fabs(va[i] - vb[i]) <= 1e-3f;
    if (!same && out.size() < cap) out.push_back({path.empty() ? "/" : path, i, va[i], vb[i], d.id});
  }
  for (size_t i = 0; i < YGNodeGetChildCount(a); i++)
    diffTrees(YGNodeGetChild(a, i), YGNodeGetChild(b, i), d.kids[i], path + "/" + std::to_string(i), out, cap);
}

// Where a run is, for the parent to report on a timeout or crash.
struct Where {
  volatile int seed, scale_index, mode, step, phase;
};
enum Phase { P_IDLE, P_GEN, P_RELAYOUT, P_FRESH, P_COMPARE, P_MINIMISE, P_DONE };
static const char* kPhaseNames[] = {"idle", "generate", "relayout", "fresh layout", "compare", "minimise", "done"};
static Where* gWhere = nullptr;
static void at(int phase) {
  if (gWhere) gWhere->phase = phase;
}

// The first left/top/width/height that is NaN or infinite, in pre-order.
static bool findNonFinite(YGNodeRef n, const std::string& path, Diff& out) {
  float v[4] = {YGNodeLayoutGetLeft(n), YGNodeLayoutGetTop(n), YGNodeLayoutGetWidth(n), YGNodeLayoutGetHeight(n)};
  for (int i = 0; i < 4; i++)
    if (!std::isfinite(v[i])) {
      out = {path.empty() ? "/" : path, i, v[i], v[i]};
      return true;
    }
  for (size_t i = 0; i < YGNodeGetChildCount(n); i++)
    if (findNonFinite(YGNodeGetChild(n, i), path + "/" + std::to_string(i), out)) return true;
  return false;
}

struct Outcome {
  int nonFiniteStep = -1;  // first step whose fresh layout has a NaN or infinite value
  Diff nonFinite;
  bool fail = false;
  int step = -1;  // first failing step
  float w = 0, h = 0;
  std::vector<Diff> diffs;  // at the failing step
  int maxCalls = 0;         // most measure callbacks one leaf got in one relayout
};

static long gChecks = 0;  // cases run (for minimiser stats)

static Outcome runCase(const Case& c, size_t diffCap = 1) {
  gChecks++;
  Outcome o;
  YGConfigRef cfg = YGConfigNew();
  YGConfigSetPointScaleFactor(cfg, c.scale);
  YGConfigSetErrata(cfg, (YGErrata)c.errata);
  {
    Built tree(c.root, cfg);
    for (size_t i = 0; i < c.sizes.size(); i++) {
      float w = c.sizes[i].first, h = c.sizes[i].second;
      if (gWhere) gWhere->step = (int)i;
      at(P_RELAYOUT);
      layoutAt(tree.root, c.mode, w, h);
      o.maxCalls = std::max(o.maxCalls, tree.resetCalls());
      at(P_FRESH);
      Built fresh(c.root, cfg);
      layoutAt(fresh.root, c.mode, w, h);
      at(P_COMPARE);
      if (o.nonFiniteStep < 0 && findNonFinite(fresh.root, "", o.nonFinite)) o.nonFiniteStep = (int)i;
      std::vector<Diff> d;
      diffTrees(tree.root, fresh.root, c.root, "", d, diffCap);
      if (!d.empty()) {
        o.fail = true;
        o.step = (int)i;
        o.w = w;
        o.h = h;
        o.diffs = std::move(d);
        break;
      }
    }
  }
  YGConfigFree(cfg);
  return o;
}

// ---------------------------------------------------------------------------
// Generator

struct Knobs {
  int depth = 4;       // nodes at this depth (root children are depth 1) get no children
  int kids = 3;        // a node below max depth gets 0..kids children
  int rootKids = 3;    // the root gets 1..rootKids children
  int measureP = 50;   // percent of childless nodes that get a measure func
  int siblings = 0;    // half the containers also get this many extra measure-func leaves
  int steps = 0;       // 0: the default size sequence; N: N random sizes
  bool wide = false;   // also generate justify/alignSelf/alignContent/absolute/minW/maxH/gap

  bool isDefault() const {
    return depth == 4 && kids == 3 && rootKids == 3 && measureP == 50 && siblings == 0 && steps == 0 && !wide;
  }
  std::string describe() const {
    if (isDefault()) return "default";
    char buf[200];
    snprintf(buf, sizeof buf, "--depth %d --kids %d --root-kids %d --measure %d --siblings %d --steps %d%s", depth,
             kids, rootKids, measureP, siblings, steps, wide ? " --wide" : "");
    return buf;
  }
};

struct Rng {
  std::mt19937 r;
  explicit Rng(uint32_t seed) : r(seed) {}
  int pick(int k) { return (int)(r() % (uint32_t)k); }
};

static void setNum(NodeDesc& d, int f, float v) { d.f[f] = {true, false, v}; }
static void setPct(NodeDesc& d, int f, float v) { d.f[f] = {true, true, v}; }

// At the default knobs this draws exactly what the scratch fuzzer's buildNode
// drew, in the same order, so seed numbers mean the same trees.
static void genNode(NodeDesc& d, Rng& r, int depth, const Knobs& k) {
  if (r.pick(3) == 0) setNum(d, F_DIR, YGFlexDirectionRow);
  switch (r.pick(6)) {
    case 0: setNum(d, F_FLEX, 1); break;
    case 1: setNum(d, F_GROW, 1); break;
    case 2: setNum(d, F_SHRINK, 1); break;
    case 3: setNum(d, F_GROW, 1); setNum(d, F_SHRINK, 1); break;
    default: break;
  }
  switch (r.pick(6)) {
    case 0: setPct(d, F_BASIS, 10.0f * (1 + r.pick(10))); break;
    case 1: setNum(d, F_BASIS, 20.0f * (1 + r.pick(10))); break;
    default: break;
  }
  switch (r.pick(8)) {
    case 0: setPct(d, F_W, 10.0f * (1 + r.pick(10))); break;
    case 1: setNum(d, F_W, 30.0f * (1 + r.pick(10))); break;
    default: break;
  }
  switch (r.pick(8)) {
    case 0: setPct(d, F_H, 10.0f * (1 + r.pick(10))); break;
    case 1: setNum(d, F_H, 20.0f * (1 + r.pick(10))); break;
    default: break;
  }
  if (r.pick(8) == 0) setNum(d, F_MINH, 10.0f * r.pick(10));
  if (r.pick(8) == 0) setNum(d, F_MAXW, 50.0f * (1 + r.pick(10)));
  if (r.pick(5) == 0) setNum(d, F_PAD, (float)r.pick(10));
  if (r.pick(6) == 0) setNum(d, F_MAR, (float)r.pick(10));
  if (r.pick(6) == 0) setNum(d, F_OVF, YGOverflowScroll);
  if (r.pick(6) == 0) setNum(d, F_AI, YGAlignFlexStart);
  if (r.pick(8) == 0) setNum(d, F_WRAP, YGWrapWrap);
  if (r.pick(10) == 0) setNum(d, F_AR, 1.0f + r.pick(3));
  if (k.wide) {
    static const int jcs[] = {YGJustifyCenter, YGJustifyFlexEnd, YGJustifySpaceBetween, YGJustifySpaceAround};
    static const int as[] = {YGAlignFlexStart, YGAlignCenter, YGAlignFlexEnd, YGAlignStretch, YGAlignBaseline};
    static const int acs[] = {YGAlignFlexStart, YGAlignCenter, YGAlignStretch, YGAlignSpaceBetween};
    if (r.pick(6) == 0) setNum(d, F_JC, (float)jcs[r.pick(4)]);
    if (r.pick(8) == 0) setNum(d, F_AS, (float)as[r.pick(5)]);
    if (r.pick(8) == 0) setNum(d, F_AC, (float)acs[r.pick(4)]);
    if (r.pick(16) == 0) setNum(d, F_POS, YGPositionTypeAbsolute);
    if (r.pick(10) == 0) setNum(d, F_MINW, 20.0f * r.pick(10));
    if (r.pick(10) == 0) setNum(d, F_MAXH, 50.0f * (1 + r.pick(10)));
    if (r.pick(8) == 0) setNum(d, F_GAP, (float)r.pick(12));
  }
  int kids = depth >= k.depth ? 0 : r.pick(k.kids + 1);
  if (kids == 0) {
    bool measured = k.measureP == 50 ? r.pick(2) == 0 : r.pick(100) < k.measureP;
    if (measured) setNum(d, F_TEXT, 200.0f + 100.0f * r.pick(20));
    return;
  }
  d.kids.resize(kids);
  for (int i = 0; i < kids; i++) genNode(d.kids[i], r, depth + 1, k);
  if (k.siblings > 0 && r.pick(2) == 0) {
    for (int i = 0; i < k.siblings; i++) {
      NodeDesc leaf;
      switch (r.pick(4)) {
        case 0: setNum(leaf, F_GROW, 1); break;
        case 1: setNum(leaf, F_SHRINK, 1); break;
        case 2: setNum(leaf, F_FLEX, 1); break;
        default: break;
      }
      if (r.pick(4) == 0) setNum(leaf, F_PAD, (float)r.pick(10));
      setNum(leaf, F_TEXT, 100.0f + 100.0f * r.pick(20));
      d.kids.push_back(leaf);
    }
  }
}

static Case generate(uint32_t seed, const Knobs& k) {
  Case c;
  Rng r(seed);
  int kids = 1 + r.pick(k.rootKids);
  c.root.kids.resize(kids);
  for (int i = 0; i < kids; i++) genNode(c.root.kids[i], r, 1, k);
  if (k.steps <= 0) {
    for (auto& s : kDefaultSizes) c.sizes.push_back({s[0], s[1]});
  } else {
    static const float ws[] = {320, 360, 375, 400, 401, 414, 600, 640, 768, 800, 1000, 1024};
    static const float hs[] = {400, 500, 568, 640, 667, 800, 812, 896, 1000};
    Rng sr(seed * 2654435761u + 17);
    for (int i = 0; i < k.steps; i++)
      c.sizes.push_back({ws[sr.pick(12)], hs[sr.pick(9)]});
  }
  assignIds(c.root);
  return c;
}

// ---------------------------------------------------------------------------
// Case files

static std::string errataName(int e) {
  if (e == YGErrataAll) return "all";
  if (e == YGErrataNone) return "none";
  if (e == YGErrataClassic) return "classic";
  return std::to_string(e);
}

static std::string valText(int f, const Val& v) {
  if (kFields[f].kind == K_ENUM) {
    const EnumName* e = enumByValue(kFields[f].names, (int)v.v);
    if (e) return e->name;
  }
  return fmtNum(v.v) + (v.pct ? "%" : "");
}

static void dumpNode(std::ostream& os, const NodeDesc& d, int depth, const std::string& path) {
  std::string line(depth * 2, ' ');
  line += depth == 0 ? "root" : "node";
  for (int f = 0; f < NF; f++)
    if (d.f[f].set) line += std::string(" ") + kFields[f].key + "=" + valText(f, d.f[f]);
  os << line;
  if (depth > 0) os << std::string(line.size() < 60 ? 60 - line.size() : 1, ' ') << "# " << path;
  os << "\n";
  for (size_t i = 0; i < d.kids.size(); i++) dumpNode(os, d.kids[i], depth + 1, path + "/" + std::to_string(i));
}

static std::string dumpCase(const Case& c, const std::string& header = "") {
  std::ostringstream os;
  os << "# barre fuzz case (docs/FUZZ.md)\n";
  if (!header.empty()) os << header;
  os << "mode " << c.mode;
  static const char* modeNames[] = {"root pinned through min/max", "available size only",
                                    "root style width, undefined available size; height unused"};
  os << std::string(14, ' ') << "# " << modeNames[c.mode] << "\n";
  os << "scale " << fmtNum(c.scale) << "\n";
  os << "errata " << errataName(c.errata) << "\n";
  os << "sizes";
  for (auto& s : c.sizes) os << " " << fmtNum(s.first) << "x" << fmtNum(s.second);
  os << "\n";
  dumpNode(os, c.root, 0, "");
  return os.str();
}

// The tree part only, without comments: used to dedupe repros.
static std::string canonicalBody(const Case& c) {
  std::ostringstream os;
  os << "mode " << c.mode << " errata " << errataName(c.errata) << " sizes";
  for (auto& s : c.sizes) os << " " << fmtNum(s.first) << "x" << fmtNum(s.second);
  os << "\n";
  std::function<void(const NodeDesc&, int)> walk = [&](const NodeDesc& d, int depth) {
    os << depth;
    for (int f = 0; f < NF; f++)
      if (d.f[f].set) os << " " << kFields[f].key << "=" << valText(f, d.f[f]);
    os << "\n";
    for (auto& k : d.kids) walk(k, depth + 1);
  };
  walk(c.root, 0);
  return os.str();
}

// Structure plus which fields are set, without values: a coarser dedupe key.
static std::string shapeKey(const Case& c) {
  std::ostringstream os;
  os << "m" << c.mode << " steps" << c.sizes.size() << "|";
  std::function<void(const NodeDesc&)> walk = [&](const NodeDesc& d) {
    os << "(";
    for (int f = 0; f < NF; f++)
      if (d.f[f].set) os << kFields[f].key << (kFields[f].kind == K_ENUM ? "=" + valText(f, d.f[f]) : "") << ",";
    for (auto& k : d.kids) walk(k);
    os << ")";
  };
  walk(c.root);
  return os.str();
}

static uint64_t fnv1a(const std::string& s) {
  uint64_t h = 1469598103934665603ull;
  for (unsigned char ch : s) {
    h ^= ch;
    h *= 1099511628211ull;
  }
  return h;
}

static bool parseVal(int f, const std::string& s, Val& v, std::string& err) {
  v.set = true;
  v.pct = false;
  if (kFields[f].kind == K_ENUM) {
    for (const EnumName* e = kFields[f].names; e->name; e++)
      if (s == e->name) {
        v.v = (float)e->value;
        return true;
      }
    err = "unknown value '" + s + "' for " + kFields[f].key;
    return false;
  }
  std::string num = s;
  if (!num.empty() && num.back() == '%') {
    if (kFields[f].kind != K_LEN) {
      err = std::string(kFields[f].key) + " takes no percent";
      return false;
    }
    v.pct = true;
    num.pop_back();
  }
  char* end = nullptr;
  v.v = strtof(num.c_str(), &end);
  if (num.empty() || *end) {
    err = "bad number '" + s + "' for " + kFields[f].key;
    return false;
  }
  return true;
}

static bool parseCase(const std::string& text, Case& c, std::string& err) {
  c = Case();
  std::istringstream is(text);
  std::string raw;
  std::vector<NodeDesc*> stack;  // stack[d] = last node at depth d
  bool haveRoot = false;
  int lineNo = 0;
  while (std::getline(is, raw)) {
    lineNo++;
    std::string line = raw.substr(0, raw.find('#'));
    size_t indent = line.find_first_not_of(' ');
    if (indent == std::string::npos) continue;
    std::istringstream ls(line);
    std::string word;
    ls >> word;
    auto fail = [&](const std::string& m) {
      err = "line " + std::to_string(lineNo) + ": " + m;
      return false;
    };
    if (word == "mode") {
      if (!(ls >> c.mode) || c.mode < 0 || c.mode > 2) return fail("mode must be 0, 1 or 2");
    } else if (word == "scale") {
      if (!(ls >> c.scale)) return fail("bad scale");
    } else if (word == "errata") {
      std::string e;
      ls >> e;
      if (e == "all") c.errata = YGErrataAll;
      else if (e == "none") c.errata = YGErrataNone;
      else if (e == "classic") c.errata = YGErrataClassic;
      else c.errata = atoi(e.c_str());
    } else if (word == "sizes") {
      std::string s;
      c.sizes.clear();
      while (ls >> s) {
        size_t x = s.find('x');
        if (x == std::string::npos) return fail("size '" + s + "' is not WxH");
        c.sizes.push_back({strtof(s.substr(0, x).c_str(), nullptr), strtof(s.substr(x + 1).c_str(), nullptr)});
      }
    } else if (word == "root" || word == "node") {
      if (indent % 2) return fail("odd indentation");
      size_t depth = indent / 2;
      NodeDesc* n;
      if (word == "root") {
        if (depth != 0 || haveRoot) return fail("one root, at column 0");
        haveRoot = true;
        n = &c.root;
        stack.assign(1, n);
      } else {
        if (depth == 0 || depth > stack.size()) return fail("node indented too deep (or no root)");
        stack.resize(depth);
        NodeDesc* parent = stack[depth - 1];
        parent->kids.emplace_back();
        n = &parent->kids.back();
        stack.push_back(n);
      }
      std::string kv;
      while (ls >> kv) {
        size_t eq = kv.find('=');
        if (eq == std::string::npos) return fail("expected key=value, got '" + kv + "'");
        std::string key = kv.substr(0, eq);
        int f = -1;
        for (int i = 0; i < NF; i++)
          if (key == kFields[i].key) f = i;
        if (f < 0) return fail("unknown key '" + key + "'");
        std::string e;
        if (!parseVal(f, kv.substr(eq + 1), n->f[f], e)) return fail(e);
      }
    } else {
      return fail("unknown directive '" + word + "'");
    }
  }
  if (!haveRoot) {
    err = "no root";
    return false;
  }
  if (c.sizes.empty()) {
    err = "no sizes";
    return false;
  }
  assignIds(c.root);
  return true;
}

static bool readFile(const std::string& path, std::string& out) {
  std::ifstream f(path);
  if (!f) return false;
  std::stringstream ss;
  ss << f.rdbuf();
  out = ss.str();
  return true;
}

// ---------------------------------------------------------------------------
// C++ snippet

static std::string varName(const std::string& path) {
  if (path.empty()) return "root";
  std::string v = "n";
  for (char ch : path) v += ch == '/' ? '_' : ch;
  return v;
}

static void snippetNode(std::ostream& os, const NodeDesc& d, const std::string& path, const std::string& parent,
                        size_t index) {
  std::string v = varName(path);
  os << "  YGNodeRef " << v << " = YGNodeNewWithConfig(cfg);";
  if (!path.empty()) os << "  // " << path;
  os << "\n";
  if (!parent.empty()) os << "  YGNodeInsertChild(" << parent << ", " << v << ", " << index << ");\n";
  for (int f = 0; f < NF; f++) {
    if (!d.f[f].set) continue;
    if (f == F_TEXT && !d.kids.empty()) continue;  // a measure func only on leaves
    os << "  " << fieldCall(f, d.f[f], v) << "\n";
  }
  for (size_t i = 0; i < d.kids.size(); i++) snippetNode(os, d.kids[i], path + "/" + std::to_string(i), v, i);
}

static std::string snippet(const Case& c, const std::string& note) {
  std::ostringstream os;
  os << "// relayout == fresh repro, generated by `./fuzz snippet` (docs/FUZZ.md).\n";
  if (!note.empty()) os << "// " << note << "\n";
  os << "// Lays the tree out at each size in turn and compares it with a fresh tree\n"
        "// laid out at that size. Exit status 1 and a DIFF line mean relayout != fresh.\n";
  os << "#include <yoga/Yoga.h>\n\n#include <cmath>\n#include <cstdint>\n#include <cstdio>\n#include <limits>\n\n";
  os << "static YGSize measureText(YGNodeConstRef n, float w, YGMeasureMode wm, float h, YGMeasureMode hm) {\n"
        "  float len = (float)(intptr_t)YGNodeGetContext(n);\n"
        "  float maxW = (wm == YGMeasureModeUndefined) ? std::numeric_limits<float>::infinity() : w;\n"
        "  float width = std::isfinite(maxW) ? std::fmin(maxW, len) : len;\n"
        "  float lines = width > 0 ? std::ceil(len / width) : 1.0f;\n"
        "  YGSize out{width, lines * 20.0f};\n"
        "  if (wm == YGMeasureModeExactly) out.width = w;\n"
        "  if (hm == YGMeasureModeExactly) out.height = h;\n"
        "  else if (hm == YGMeasureModeAtMost && out.height > h) out.height = h;\n"
        "  return out;\n"
        "}\n\n";
  os << "static YGNodeRef build(YGConfigRef cfg) {\n";
  snippetNode(os, c.root, "", "", 0);
  os << "  return root;\n}\n\n";
  os << "static void layoutAt(YGNodeRef root, float w, float h) {\n";
  if (c.mode == 0)
    os << "  YGNodeStyleSetMinWidth(root, w);\n  YGNodeStyleSetMaxWidth(root, w);\n"
          "  YGNodeStyleSetMinHeight(root, h);\n  YGNodeStyleSetMaxHeight(root, h);\n"
          "  YGNodeCalculateLayout(root, w, h, YGDirectionLTR);\n";
  else if (c.mode == 1)
    os << "  YGNodeCalculateLayout(root, w, h, YGDirectionLTR);\n";
  else
    os << "  (void)h;\n  YGNodeStyleSetWidth(root, w);\n"
          "  YGNodeCalculateLayout(root, YGUndefined, YGUndefined, YGDirectionLTR);\n";
  os << "}\n\n";
  os << "static int compare(YGNodeRef a, YGNodeRef b, const char* path) {\n"
        "  static const char* names[4] = {\"left\", \"top\", \"width\", \"height\"};\n"
        "  float va[4] = {YGNodeLayoutGetLeft(a), YGNodeLayoutGetTop(a), YGNodeLayoutGetWidth(a), "
        "YGNodeLayoutGetHeight(a)};\n"
        "  float vb[4] = {YGNodeLayoutGetLeft(b), YGNodeLayoutGetTop(b), YGNodeLayoutGetWidth(b), "
        "YGNodeLayoutGetHeight(b)};\n"
        "  int bad = 0;\n"
        "  for (int i = 0; i < 4; i++)\n"
        "    if (!(va[i] == vb[i] || (std::isnan(va[i]) && std::isnan(vb[i])) || std::fabs(va[i] - vb[i]) <= 1e-3f)) {\n"
        "      printf(\"DIFF %s %s: relayout %g fresh %g\\n\", path[0] ? path : \"/\", names[i], va[i], vb[i]);\n"
        "      bad = 1;\n"
        "    }\n"
        "  for (size_t i = 0; i < YGNodeGetChildCount(a); i++) {\n"
        "    char p[256];\n"
        "    snprintf(p, sizeof p, \"%s/%zu\", path, i);\n"
        "    bad |= compare(YGNodeGetChild(a, i), YGNodeGetChild(b, i), p);\n"
        "  }\n"
        "  return bad;\n"
        "}\n\n";
  os << "int main() {\n";
  os << "  YGConfigRef cfg = YGConfigNew();\n";
  os << "  YGConfigSetPointScaleFactor(cfg, " << fmtNum(c.scale) << ");\n";
  if (c.errata == YGErrataAll) os << "  YGConfigSetErrata(cfg, YGErrataAll);\n";
  else if (c.errata == YGErrataClassic) os << "  YGConfigSetErrata(cfg, YGErrataClassic);\n";
  else if (c.errata != YGErrataNone) os << "  YGConfigSetErrata(cfg, (YGErrata)" << c.errata << ");\n";
  os << "  const float sizes[][2] = {";
  for (size_t i = 0; i < c.sizes.size(); i++)
    os << (i ? ", " : "") << "{" << fmtNum(c.sizes[i].first) << ", " << fmtNum(c.sizes[i].second) << "}";
  os << "};\n";
  os << "  YGNodeRef root = build(cfg);\n"
        "  int bad = 0;\n"
        "  for (auto& s : sizes) {\n"
        "    layoutAt(root, s[0], s[1]);\n"
        "    YGNodeRef fresh = build(cfg);\n"
        "    layoutAt(fresh, s[0], s[1]);\n"
        "    printf(\"layout at %gx%g\\n\", s[0], s[1]);\n"
        "    bad = compare(root, fresh, \"\");\n"
        "    YGNodeFreeRecursive(fresh);\n"
        "    if (bad) break;\n"
        "  }\n"
        "  YGNodeFreeRecursive(root);\n"
        "  YGConfigFree(cfg);\n"
        "  printf(bad ? \"relayout != fresh\\n\" : \"relayout == fresh at every size\\n\");\n"
        "  return bad;\n"
        "}\n";
  return os.str();
}

// ---------------------------------------------------------------------------
// Classes and the minimiser

// A failure's class is which of the two scales (0 and the nonzero one) fail.
enum Cls { C_PASS = 0, C_SCALED_ONLY = 1, C_ZERO_ONLY = 2, C_BOTH = 3, C_NONFINITE = 4 };

static std::string clsName(int cls, float scale) {
  switch (cls) {
    case C_SCALED_ONLY: return "scale" + fmtNum(scale) + "-only";
    case C_ZERO_ONLY: return "scale0-only";
    case C_BOTH: return "both";
    case C_NONFINITE: return "nonfinite";
  }
  return "pass";
}

static int clsOf(bool f0, bool fs) { return (f0 ? 2 : 0) | (fs ? 1 : 0); }

// What "the same failure" means while minimising: the same mode, the same
// class over scales {0, scale}, and, at the primary scale (the failing one;
// for class both, the one that fails later, else the nonzero one), the first
// difference is in the same field with the same sign and shows up first at
// the last size step.
struct Target {
  int cls;
  float scale;    // the nonzero scale
  float primary;  // 0 or scale
  int field;
  int sign;
  int node;  // NodeDesc::id of the first differing node; -1 with --loose
};

static bool gLoose = false;  // --loose: any node may show the first difference

static Case withScale(Case c, float s) {
  c.scale = s;
  return c;
}

static float primaryScale(const Target& t) { return t.primary; }

// Optional second oracle for the minimiser: another build of this fuzzer
// (e.g. against real Yoga) that must PASS the candidate at the primary scale,
// so a "barre fails where Yoga passes" case does not drift into a mechanism
// both share. It is given the candidate itself, as a file.
static std::vector<std::string> gPassOn;
static bool gNonFinite = false;  // --nonfinite: hunt NaN/infinite fresh layouts, not relayout differences
static bool passesOn(const std::string& exe, const Case& c) {
  char path[] = "/tmp/barre-fuzz-cand-XXXXXX";
  int fd = mkstemp(path);
  if (fd < 0) return false;
  std::string text = dumpCase(c, "");
  bool ok = write(fd, text.data(), text.size()) == (ssize_t)text.size();
  close(fd);
  std::string cmd = "'" + exe + "' check --at-scale " + fmtNum(c.scale) + (gNonFinite ? " --nonfinite" : "") + " '" +
                    path + "' >/dev/null 2>&1";
  int rc = ok ? system(cmd.c_str()) : -1;
  unlink(path);
  return rc == 0;
}

// The difference that identifies the target failure in an outcome: the
// target node's (or, with --loose, the first) difference in the target field
// with the target sign. Other nodes may differ too.
static const Diff* targetDiff(const Outcome& o, const Target& t) {
  for (const Diff& d : o.diffs) {
    if (t.node >= 0 && d.node != t.node) continue;
    if (d.field == t.field && ((d.relayout > d.fresh) ? 1 : -1) == t.sign) return &d;
    if (t.node < 0) return nullptr;  // loose: only the first difference counts
  }
  return nullptr;
}

static bool reproduces(const Case& c, const Target& t) {
  Case primary = withScale(c, primaryScale(t));
  Outcome o = runCase(primary, t.node >= 0 ? (size_t)1 << 20 : 1);
  if (t.cls == C_NONFINITE) {
    if (o.nonFiniteStep != (int)c.sizes.size() - 1 || o.nonFinite.field != t.field) return false;
    for (auto& exe : gPassOn)
      if (!passesOn(exe, primary)) return false;
    return true;
  }
  if (!o.fail || o.step != (int)c.sizes.size() - 1 || !targetDiff(o, t)) return false;
  Outcome other = runCase(withScale(c, t.primary == 0 ? t.scale : 0.0f));
  bool otherShouldFail = t.cls == C_BOTH;
  if (other.fail != otherShouldFail) return false;
  for (auto& exe : gPassOn)
    if (!passesOn(exe, primary)) return false;
  return true;
}

// Every node path in pre-order, root excluded.
static void collectPaths(const NodeDesc& d, std::vector<int>& cur, std::vector<std::vector<int>>& out) {
  for (size_t i = 0; i < d.kids.size(); i++) {
    cur.push_back((int)i);
    out.push_back(cur);
    collectPaths(d.kids[i], cur, out);
    cur.pop_back();
  }
}

static NodeDesc* nodeAt(NodeDesc& root, const std::vector<int>& p) {
  NodeDesc* n = &root;
  for (int i : p) n = &n->kids[i];
  return n;
}

static std::vector<float> simplerValues(float v, bool allowZero) {
  static const float round[] = {1, 2, 5, 10, 20, 25, 50, 100, 200, 300, 500, 1000};
  std::vector<float> out;
  float a = std::fabs(v);
  if (allowZero && a > 0) out.push_back(0);
  for (float r : round)
    if (r < a) out.push_back(v < 0 ? -r : r);
  float half = std::round(v / 2);
  if (std::fabs(half) < a && std::fabs(half) > 0) out.push_back(half);
  // Only ever smaller in magnitude, so the minimiser cannot cycle.
  float r10 = std::trunc(v / 10) * 10, r100 = std::trunc(v / 100) * 100;
  if (std::fabs(r10) < a && r10 != 0) out.push_back(r10);
  if (std::fabs(r100) < a && r100 != 0) out.push_back(r100);
  std::sort(out.begin(), out.end(), [](float x, float y) { return std::fabs(x) < std::fabs(y); });
  out.erase(std::unique(out.begin(), out.end()), out.end());
  return out;
}

struct MinStats {
  long checks = 0;
  int nodesBefore = 0, nodesAfter = 0;
  int stepsBefore = 0, stepsAfter = 0;
};

static bool minimise(Case& c, Target& t, MinStats& st) {
  long checks0 = gChecks;
  at(P_MINIMISE);
  st.nodesBefore = countNodes(c.root);
  if (!st.stepsBefore) st.stepsBefore = (int)c.sizes.size();
  if (!reproduces(c, t)) return false;
  auto tryCase = [&](const Case& cand) {
    if (!reproduces(cand, t)) return false;
    c = cand;
    return true;
  };
  bool changed = true;
  while (changed) {
    changed = false;
    // 1. drop subtrees, biggest (closest to the root) first
    for (bool again = true; again;) {
      again = false;
      std::vector<std::vector<int>> paths;
      std::vector<int> cur;
      collectPaths(c.root, cur, paths);
      for (auto& p : paths) {
        Case cand = c;
        std::vector<int> parent(p.begin(), p.end() - 1);
        NodeDesc* par = nodeAt(cand.root, parent);
        par->kids.erase(par->kids.begin() + p.back());
        if (tryCase(cand)) {
          changed = again = true;
          break;
        }
      }
    }
    // 2. replace a node by its children
    for (bool again = true; again;) {
      again = false;
      std::vector<std::vector<int>> paths;
      std::vector<int> cur;
      collectPaths(c.root, cur, paths);
      for (auto& p : paths) {
        if (nodeAt(c.root, p)->kids.empty()) continue;
        Case cand = c;
        std::vector<int> parent(p.begin(), p.end() - 1);
        NodeDesc* par = nodeAt(cand.root, parent);
        NodeDesc moved = par->kids[p.back()];
        par->kids.erase(par->kids.begin() + p.back());
        par->kids.insert(par->kids.begin() + p.back(), moved.kids.begin(), moved.kids.end());
        if (tryCase(cand)) {
          changed = again = true;
          break;
        }
      }
    }
    // 3. drop earlier size steps
    for (size_t i = 0; i + 1 < c.sizes.size();) {
      Case cand = c;
      cand.sizes.erase(cand.sizes.begin() + i);
      if (tryCase(cand))
        changed = true;
      else
        i++;
    }
    // 4. unset style fields
    {
      std::vector<std::vector<int>> paths{{}};
      std::vector<int> cur;
      collectPaths(c.root, cur, paths);
      for (auto& p : paths)
        for (int f = 0; f < NF; f++) {
          if (!nodeAt(c.root, p)->f[f].set) continue;
          Case cand = c;
          nodeAt(cand.root, p)->f[f] = Val();
          if (tryCase(cand)) changed = true;
        }
    }
    // 5. simpler values: percent to points, smaller and rounder numbers
    {
      std::vector<std::vector<int>> paths{{}};
      std::vector<int> cur;
      collectPaths(c.root, cur, paths);
      for (auto& p : paths)
        for (int f = 0; f < NF; f++) {
          Val v = nodeAt(c.root, p)->f[f];
          if (!v.set || kFields[f].kind == K_ENUM) continue;
          if (v.pct) {
            Case cand = c;
            nodeAt(cand.root, p)->f[f].pct = false;
            if (tryCase(cand)) changed = true;
            v = nodeAt(c.root, p)->f[f];
          }
          bool zeroOk = f == F_PAD || f == F_MAR || f == F_MINH || f == F_MINW || f == F_GAP || f == F_FLEX ||
                        f == F_GROW || f == F_SHRINK;
          for (float x : simplerValues(v.v, zeroOk)) {
            if (f == F_AR && x <= 0) continue;
            Case cand = c;
            nodeAt(cand.root, p)->f[f].v = x;
            if (tryCase(cand)) {
              changed = true;
              break;
            }
          }
        }
    }
    // 6. simpler sizes: smaller, but whole hundreds and at least 100, so a
    // repro stays a plausible screen rather than a degenerate 1x1 root
    for (size_t i = 0; i < c.sizes.size(); i++)
      for (int dim = 0; dim < 2; dim++) {
        float v = dim ? c.sizes[i].second : c.sizes[i].first;
        if (c.mode == 2 && dim == 1) continue;  // height unused
        for (float x : simplerValues(v, false)) {
          if (x < 100 || std::fmod(x, 100.0f) != 0) continue;
          Case cand = c;
          (dim ? cand.sizes[i].second : cand.sizes[i].first) = x;
          if (tryCase(cand)) {
            changed = true;
            break;
          }
        }
      }
  }
  // Renumber in pre-order so the ids match what parsing the printed case gives.
  std::function<int(const NodeDesc&, int)> newIdOf = [&](const NodeDesc& d, int next) {
    if (d.id == t.node) t.node = -2 - next;  // mark, so a later id cannot match again
    next++;
    for (auto& k : d.kids) next = newIdOf(k, next);
    return next;
  };
  if (t.node >= 0) {
    newIdOf(c.root, 0);
    t.node = -2 - t.node;
  }
  assignIds(c.root);
  st.checks = gChecks - checks0;
  st.nodesAfter = countNodes(c.root);
  st.stepsAfter = (int)c.sizes.size();
  return true;
}

static float gPrimary = -1;  // --primary: which scale a class-both case is minimised at

// Classify one (case, mode) over scales {0, scale}; fill the target for the
// minimiser. Returns the class.
static int classify(const Case& base, float scale, Outcome& o0, Outcome& os, Target& t, Case& trimmed) {
  o0 = runCase(withScale(base, 0));
  os = runCase(withScale(base, scale));
  int cls = clsOf(o0.fail, os.fail);
  if (cls == C_PASS) return cls;
  bool zeroPrimary = cls == C_ZERO_ONLY || (cls == C_BOTH && o0.step > os.step);
  if (cls == C_BOTH && gPrimary >= 0) zeroPrimary = gPrimary == 0;
  const Outcome& p = zeroPrimary ? o0 : os;
  t = {cls,
       scale,
       zeroPrimary ? 0.0f : scale,
       p.diffs[0].field,
       p.diffs[0].relayout > p.diffs[0].fresh ? 1 : -1,
       gLoose ? -1 : p.diffs[0].node};
  trimmed = base;
  trimmed.sizes.resize(p.step + 1);
  return cls;
}

// ---------------------------------------------------------------------------
// Command line

struct Options {
  std::string cmd;
  std::vector<std::string> positional;
  Knobs knobs;
  uint32_t seedLo = 1, seedHi = 1000;
  bool haveSeed = false;
  uint32_t seed = 0;
  std::vector<int> modes{0, 1, 2};
  float scale = 1;  // the nonzero scale classes compare against 0
  int mode = -1;
  int jobs = 1;
  double timeout = -1;
  bool doMin = false;
  bool verbose = false;
  std::string saveDir;
  std::string jsonPath;
  std::string out;
  long injectSeed = -1;
  std::string injectKind;
  int diffs = 10;
  float atScale = -1;
};

static void usage() {
  fprintf(stderr,
          "usage: fuzz COMMAND [options]   (normally run through ./fuzz; see docs/FUZZ.md)\n"
          "  run [--seeds A-B|N] [--modes 0,1,2] [--scale S] [--jobs J] [--timeout SEC]\n"
          "      [--minimise] [--save DIR] [--json FILE] [-v] [knobs]\n"
          "  show --seed S [knobs]              print the case a seed generates\n"
          "  check FILE | --seed S --mode M [--scale S] [knobs]\n"
          "                                     run one case at scales 0 and S, print the diffs\n"
          "  min FILE | --seed S --mode M [--scale S] [--out FILE] [knobs]\n"
          "                                     minimise one failing case\n"
          "  snippet FILE                       print a standalone C++ repro\n"
          "  calls [--seeds A-B|N] [--modes 0,1,2] [--scale S] [knobs]\n"
          "                                     measure callbacks per relayout step (no comparison)\n"
          "min/run --minimise also take --primary 0|S (scale a class-both case is minimised at)\n"
          "and --pass-on EXE (another fuzz build, e.g. against Yoga, must pass every candidate)\n"
          "min --nonfinite [--at-scale S]: minimise a NaN/infinite value in a fresh layout instead\n"
          "knobs: --depth D (4) --kids K (3) --root-kids K (3) --measure PCT (50)\n"
          "       --siblings N (0) --steps N (0 = default sizes) --wide\n");
}

static std::vector<int> parseList(const std::string& s) {
  std::vector<int> v;
  std::stringstream ss(s);
  std::string x;
  while (std::getline(ss, x, ',')) v.push_back(atoi(x.c_str()));
  return v;
}

static bool parseArgs(int argc, char** argv, Options& o) {
  if (argc < 2) return false;
  o.cmd = argv[1];
  for (int i = 2; i < argc; i++) {
    std::string a = argv[i];
    auto next = [&]() -> std::string {
      if (i + 1 >= argc) {
        fprintf(stderr, "fuzz: %s needs a value\n", a.c_str());
        exit(2);
      }
      return argv[++i];
    };
    if (a == "--seeds") {
      std::string s = next();
      size_t dash = s.find('-');
      if (dash == std::string::npos) {
        o.seedLo = 1;
        o.seedHi = (uint32_t)atol(s.c_str());
      } else {
        o.seedLo = (uint32_t)atol(s.substr(0, dash).c_str());
        o.seedHi = (uint32_t)atol(s.substr(dash + 1).c_str());
      }
    } else if (a == "--seed") {
      o.haveSeed = true;
      o.seed = (uint32_t)atol(next().c_str());
    } else if (a == "--modes") o.modes = parseList(next());
    else if (a == "--mode") o.mode = atoi(next().c_str());
    else if (a == "--scale") o.scale = (float)atof(next().c_str());
    else if (a == "--jobs" || a == "-j") o.jobs = std::max(1, atoi(next().c_str()));
    else if (a == "--timeout") o.timeout = atof(next().c_str());
    else if (a == "--minimise" || a == "--minimize") o.doMin = true;
    else if (a == "--save") o.saveDir = next();
    else if (a == "--json") o.jsonPath = next();
    else if (a == "--out") o.out = next();
    else if (a == "--diffs") o.diffs = atoi(next().c_str());
    else if (a == "--at-scale") o.atScale = (float)atof(next().c_str());
    else if (a == "--primary") gPrimary = (float)atof(next().c_str());
    else if (a == "--pass-on") gPassOn.push_back(next());
    else if (a == "--nonfinite") gNonFinite = true;
    else if (a == "--loose") gLoose = true;
    else if (a == "-v" || a == "--verbose") o.verbose = true;
    else if (a == "--depth") o.knobs.depth = atoi(next().c_str());
    else if (a == "--kids") o.knobs.kids = atoi(next().c_str());
    else if (a == "--root-kids") o.knobs.rootKids = std::max(1, atoi(next().c_str()));
    else if (a == "--measure") o.knobs.measureP = atoi(next().c_str());
    else if (a == "--siblings") o.knobs.siblings = atoi(next().c_str());
    else if (a == "--steps") o.knobs.steps = atoi(next().c_str());
    else if (a == "--wide") o.knobs.wide = true;
    else if (a == "--inject-hang") {  // runner self-test: SEED:spin or SEED:sleep
      std::string s = next();
      size_t colon = s.find(':');
      o.injectSeed = atol(s.substr(0, colon).c_str());
      o.injectKind = colon == std::string::npos ? "sleep" : s.substr(colon + 1);
    } else if (!a.empty() && a[0] == '-') {
      fprintf(stderr, "fuzz: unknown option %s\n", a.c_str());
      return false;
    } else
      o.positional.push_back(a);
  }
  if (o.scale == 0) {
    fprintf(stderr, "fuzz: --scale is the nonzero scale compared against 0\n");
    return false;
  }
  return true;
}

static std::string diffText(const Diff& d) {
  return d.path + " " + kFieldNames[d.field] + ": relayout " + fmtNum(d.relayout) + " fresh " + fmtNum(d.fresh);
}

static std::string outcomeText(const Outcome& o) {
  if (!o.fail) return "pass";
  char buf[96];
  snprintf(buf, sizeof buf, "fails at step %d (%sx%s): ", o.step, fmtNum(o.w).c_str(), fmtNum(o.h).c_str());
  return buf + diffText(o.diffs[0]);
}

// Load a case from FILE or from --seed/--mode.
static bool loadCase(const Options& o, Case& c, std::string& origin) {
  if (!o.positional.empty()) {
    std::string text, err;
    if (!readFile(o.positional[0], text)) {
      fprintf(stderr, "fuzz: cannot read %s\n", o.positional[0].c_str());
      return false;
    }
    if (!parseCase(text, c, err)) {
      fprintf(stderr, "fuzz: %s: %s\n", o.positional[0].c_str(), err.c_str());
      return false;
    }
    if (o.mode >= 0) c.mode = o.mode;
    origin = o.positional[0];
    return true;
  }
  if (!o.haveSeed) {
    fprintf(stderr, "fuzz: give a case file or --seed\n");
    return false;
  }
  c = generate(o.seed, o.knobs);
  c.mode = o.mode >= 0 ? o.mode : 0;
  c.scale = o.scale;
  origin = "seed " + std::to_string(o.seed) + " mode " + std::to_string(c.mode) + ", knobs " + o.knobs.describe();
  return true;
}

// The scale to classify against: a file's own nonzero scale, else --scale.
static float classScale(const Options& o, const Case& c) { return c.scale != 0 ? c.scale : o.scale; }

static int cmdShow(const Options& o) {
  Case c;
  std::string origin;
  if (!loadCase(o, c, origin)) return 2;
  printf("%s", dumpCase(c, "# " + origin + "\n").c_str());
  return 0;
}

static int cmdCheck(const Options& o) {
  if (o.positional.size() > 1) {  // several files: one line each
    int any = 0;
    for (auto& file : o.positional) {
      Options one = o;
      one.positional = {file};
      Case c;
      std::string origin;
      if (!loadCase(one, c, origin)) return 2;
      float s = classScale(o, c);
      Outcome o0, os;
      Target t;
      Case trimmed;
      int cls = classify(c, s, o0, os, t, trimmed);
      const Outcome& p = cls == C_PASS ? os : (t.primary == 0 ? o0 : os);
      printf("%s: %s%s%s", file.c_str(), clsName(cls, s).c_str(), cls ? ", " : "", cls ? outcomeText(p).c_str() : "");
      for (const Outcome* r : {&o0, &os})
        if (r->nonFiniteStep >= 0) {
          printf("; fresh layout at scale %s has %s %s = %s", r == &o0 ? "0" : fmtNum(s).c_str(),
                 r->nonFinite.path.c_str(), kFieldNames[r->nonFinite.field], fmtNum(r->nonFinite.fresh).c_str());
          any = 1;
        }
      printf("\n");
      any |= cls != C_PASS;
    }
    return any ? 1 : 0;
  }
  Case c;
  std::string origin;
  if (!loadCase(o, c, origin)) return 2;
  if (o.atScale >= 0) {  // one scale only; the exit status is the answer
    Outcome r = runCase(withScale(c, o.atScale), (size_t)o.diffs);
    printf("scale %s: %s\n", fmtNum(o.atScale).c_str(), outcomeText(r).c_str());
    if (gNonFinite) return r.nonFiniteStep >= 0 ? 1 : 0;
    return r.fail ? 1 : 0;
  }
  float s = classScale(o, c);
  printf("%s: %d nodes, mode %d, %zu sizes\n", origin.c_str(), countNodes(c.root), c.mode, c.sizes.size());
  int worst = 0;
  for (float scale : {0.0f, s}) {
    Outcome r = runCase(withScale(c, scale), (size_t)o.diffs);
    printf("scale %s: %s\n", fmtNum(scale).c_str(), outcomeText(r).c_str());
    for (size_t i = 1; i < r.diffs.size(); i++) printf("    %s\n", diffText(r.diffs[i]).c_str());
    if (r.nonFiniteStep >= 0)
      printf("    note: the fresh layout at step %d has %s %s = %s\n", r.nonFiniteStep, r.nonFinite.path.c_str(),
             kFieldNames[r.nonFinite.field], fmtNum(r.nonFinite.fresh).c_str());
    worst |= r.fail;
  }
  Outcome o0, os;
  Target t;
  Case trimmed;
  printf("class: %s\n", clsName(classify(c, s, o0, os, t, trimmed), s).c_str());
  return worst ? 1 : 0;
}

static std::string minHeader(const std::string& origin, const Target& t, const MinStats& st, const Outcome& p) {
  std::ostringstream os;
  os << "# class " << clsName(t.cls, t.scale) << " (over scales 0 and " << fmtNum(t.scale) << ")\n";
  os << "# minimised from " << origin << ": " << st.nodesBefore << " -> " << st.nodesAfter << " nodes, "
     << st.stepsBefore << " -> " << st.stepsAfter << " sizes, " << st.checks << " checks\n";
  if (t.cls == C_NONFINITE)
    os << "# at scale " << fmtNum(primaryScale(t)) << ", last size, fresh layout: " << p.nonFinite.path << " "
       << kFieldNames[p.nonFinite.field] << " = " << fmtNum(p.nonFinite.fresh) << "\n";
  else {
    const Diff* d = targetDiff(p, t);
    os << "# at scale " << fmtNum(primaryScale(t)) << ", last size: " << diffText(d ? *d : p.diffs[0]);
    if (p.diffs.size() > 1) os << " (and " << p.diffs.size() - 1 << " more)";
    os << "\n";
  }
  return os.str();
}

static int cmdMin(const Options& o) {
  Case c;
  std::string origin;
  if (!loadCase(o, c, origin)) return 2;
  float s = classScale(o, c);
  Outcome o0, os;
  Target t;
  Case trimmed;
  int cls;
  if (gNonFinite) {  // at --scale only (0 allowed via the file's scale)
    float at = o.atScale >= 0 ? o.atScale : c.scale;
    Outcome r = runCase(withScale(c, at));
    if (r.nonFiniteStep < 0) {
      fprintf(stderr, "fuzz: %s has no NaN or infinite value at scale %s\n", origin.c_str(), fmtNum(at).c_str());
      return 1;
    }
    cls = C_NONFINITE;
    t = {cls, s, at, r.nonFinite.field, 1, -1};
    trimmed = c;
    trimmed.sizes.resize(r.nonFiniteStep + 1);
  } else {
    cls = classify(c, s, o0, os, t, trimmed);
  }
  if (cls == C_PASS) {
    fprintf(stderr, "fuzz: %s passes at scales 0 and %s; nothing to minimise\n", origin.c_str(),
            fmtNum(s).c_str());
    return 1;
  }
  MinStats st;
  st.stepsBefore = (int)c.sizes.size();
  if (!minimise(trimmed, t, st)) {
    if (!gPassOn.empty())
      fprintf(stderr, "fuzz: the case also fails on the --pass-on engine; nothing to minimise\n");
    else
      fprintf(stderr, "fuzz: the trimmed case does not reproduce (bug in the fuzzer)\n");
    return 3;
  }
  // Round-trip through the text format and check again, so what is printed is
  // what reproduces.
  Case back;
  std::string err;
  trimmed.scale = primaryScale(t);
  Outcome p = runCase(trimmed, (size_t)1 << 20);
  std::string text = dumpCase(trimmed, minHeader(origin, t, st, p));
  if (!parseCase(text, back, err) || !reproduces(back, t)) {
    fprintf(stderr, "fuzz: minimised case does not reproduce after a round trip (%s)\n", err.c_str());
    return 3;
  }
  printf("%s", text.c_str());
  if (!o.out.empty()) {
    std::ofstream f(o.out);
    f << text;
  }
  return 0;
}

static int cmdSnippet(const Options& o) {
  if (o.positional.empty()) {
    usage();
    return 2;
  }
  std::string text, err;
  Case c;
  if (!readFile(o.positional[0], text) || !parseCase(text, c, err)) {
    fprintf(stderr, "fuzz: %s: %s\n", o.positional[0].c_str(), err.empty() ? "cannot read" : err.c_str());
    return 2;
  }
  printf("%s", snippet(c, "from " + o.positional[0]).c_str());
  return 0;
}

// ---------------------------------------------------------------------------
// run: one forked child per seed, a pool of --jobs, a timeout per seed

struct Job {
  pid_t pid;
  uint32_t seed;
  int fd;
  int slot;
  double start;
  std::string buf;
};

static double now() {
  struct timeval tv;
  gettimeofday(&tv, nullptr);
  return tv.tv_sec + tv.tv_usec * 1e-6;
}

static void writeAll(int fd, const std::string& s) {
  const char* p = s.data();
  size_t n = s.size();
  while (n) {
    ssize_t w = write(fd, p, n);
    if (w < 0) {
      if (errno == EINTR) continue;
      _exit(4);
    }
    p += w;
    n -= (size_t)w;
  }
}

static std::string escapeLines(const std::string& s) {
  std::string o;
  for (char ch : s) o += ch == '\n' ? '\x1f' : ch;
  return o;
}
static std::string unescapeLines(const std::string& s) {
  std::string o;
  for (char ch : s) o += ch == '\x1f' ? '\n' : ch;
  return o;
}

// Child: run every mode of one seed at scales 0 and S, report over fd.
static void childSeed(const Options& o, uint32_t seed, int fd) {
  gWhere->seed = (int)seed;
  if ((long)seed == o.injectSeed) {
    at(P_RELAYOUT);
    if (o.injectKind == "spin") {
      volatile uint64_t x = 0;
      for (;;) x = x + 1;
    }
    for (;;) pause();
  }
  at(P_GEN);
  Case base = generate(seed, o.knobs);
  std::string out;
  for (int mode : o.modes) {
    gWhere->mode = mode;
    Case c = base;
    c.mode = mode;
    Outcome o0, os;
    Target t;
    Case trimmed;
    gWhere->scale_index = 0;
    int cls = classify(c, o.scale, o0, os, t, trimmed);
    char line[512];
    snprintf(line, sizeof line, "P %u %d %d %d\n", seed, mode, o0.maxCalls, os.maxCalls);
    out += line;
    for (const Outcome* r : {&o0, &os})
      if (r->nonFiniteStep >= 0) {
        snprintf(line, sizeof line, "N %u %d %s %d %s %s %s\n", seed, mode, r == &o0 ? "0" : fmtNum(o.scale).c_str(),
                 r->nonFiniteStep, r->nonFinite.path.c_str(), kFieldNames[r->nonFinite.field],
                 fmtNum(r->nonFinite.fresh).c_str());
        out += line;
      }
    if (cls == C_PASS) continue;
    const Outcome& p = t.primary == 0 ? o0 : os;
    snprintf(line, sizeof line, "F %u %d %d %d %s %s %s %d %s %s\n", seed, mode, cls, p.step, fmtNum(p.w).c_str(),
             fmtNum(p.h).c_str(), p.diffs[0].path.c_str(), p.diffs[0].field, fmtNum(p.diffs[0].relayout).c_str(),
             fmtNum(p.diffs[0].fresh).c_str());
    out += line;
    if (o.doMin) {
      MinStats st;
      st.stepsBefore = (int)c.sizes.size();
      if (!minimise(trimmed, t, st)) {
        if (!gPassOn.empty())  // fails on the --pass-on engine too: not what we are hunting
          out += "S " + std::to_string(seed) + " " + std::to_string(mode) + "\n";
        else
          out += "X " + std::to_string(seed) + " " + std::to_string(mode) +
                 " minimiser: trimmed case does not reproduce\n";
        continue;
      }
      trimmed.scale = primaryScale(t);
      Outcome pm = runCase(trimmed, (size_t)1 << 20);
      std::string origin = "seed " + std::to_string(seed) + " mode " + std::to_string(mode) + ", knobs " +
                           o.knobs.describe();
      std::string text = dumpCase(trimmed, minHeader(origin, t, st, pm));
      Case back;
      std::string err;
      bool ok = parseCase(text, back, err) && reproduces(back, t);
      out += std::string("M ") + std::to_string(seed) + " " + std::to_string(mode) + " " + std::to_string(cls) + " " +
             (ok ? "ok" : "bad") + " " + std::to_string(st.nodesAfter) + " " + std::to_string(st.stepsAfter) + " " +
             escapeLines(text) + "\n";
    }
  }
  at(P_DONE);
  out += "D " + std::to_string(seed) + "\n";
  writeAll(fd, out);
}

struct Failure {
  uint32_t seed;
  int mode, cls, step;
  std::string w, h, path;
  int field;
  std::string relayout, fresh;
};

struct Repro {
  std::string text;
  int cls, mode, nodes, steps;
  std::vector<std::pair<uint32_t, int>> from;  // (seed, mode)
};

static int cmdRun(const Options& o) {
  double timeout = o.timeout > 0 ? o.timeout : (o.doMin ? 300 : 10);
  int jobs = o.jobs;
  Where* where = (Where*)mmap(nullptr, sizeof(Where) * jobs, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANON, -1, 0);
  if (where == MAP_FAILED) {
    perror("mmap");
    return 2;
  }
  std::vector<int> freeSlots;
  for (int i = jobs - 1; i >= 0; i--) freeSlots.push_back(i);
  std::vector<Job> running;
  uint32_t nextSeed = o.seedLo;
  uint32_t total = o.seedHi >= o.seedLo ? o.seedHi - o.seedLo + 1 : 0;
  uint32_t done = 0;
  std::vector<Failure> fails;
  std::map<uint64_t, Repro> repros;      // exact text body
  std::map<std::string, int> shapeCount;  // shape key -> repros
  std::vector<std::string> problems;      // timeouts, crashes, minimiser errors
  long pairs = 0;
  long skipped = 0;  // failures the --pass-on engine fails too
  std::vector<std::string> nonFinite;  // "seed mode scale step path field value"
  std::map<int, long> callHist;  // max measure callbacks per leaf per layout -> (seed,mode,scale) count
  double t0 = now(), lastProgress = 0;
  FILE* json = o.jsonPath.empty() ? nullptr : fopen(o.jsonPath.c_str(), "w");

  auto handleLine = [&](const std::string& line) {
    std::istringstream ls(line);
    std::string tag;
    ls >> tag;
    if (tag == "P") {
      uint32_t seed;
      int mode, c0, cs;
      ls >> seed >> mode >> c0 >> cs;
      pairs++;
      callHist[std::min(c0, 64)]++;
      callHist[std::min(cs, 64)]++;
    } else if (tag == "F") {
      Failure f;
      ls >> f.seed >> f.mode >> f.cls >> f.step >> f.w >> f.h >> f.path >> f.field >> f.relayout >> f.fresh;
      fails.push_back(f);
      printf("seed %u mode %d %s: step %d %sx%s %s %s: relayout %s fresh %s\n", f.seed, f.mode,
             clsName(f.cls, o.scale).c_str(), f.step, f.w.c_str(), f.h.c_str(), f.path.c_str(),
             kFieldNames[f.field], f.relayout.c_str(), f.fresh.c_str());
      if (json)
        fprintf(json,
                "{\"seed\":%u,\"mode\":%d,\"class\":\"%s\",\"step\":%d,\"w\":\"%s\",\"h\":\"%s\",\"path\":\"%s\","
                "\"field\":\"%s\",\"relayout\":\"%s\",\"fresh\":\"%s\"}\n",
                f.seed, f.mode, clsName(f.cls, o.scale).c_str(), f.step, f.w.c_str(), f.h.c_str(), f.path.c_str(),
                kFieldNames[f.field], f.relayout.c_str(), f.fresh.c_str());
    } else if (tag == "M") {
      uint32_t seed;
      int mode, cls, nodes, steps;
      std::string ok;
      ls >> seed >> mode >> cls >> ok >> nodes >> steps;
      std::string rest;
      std::getline(ls, rest);
      std::string text = unescapeLines(rest.substr(1));
      if (ok != "ok") {
        problems.push_back("seed " + std::to_string(seed) + " mode " + std::to_string(mode) +
                           ": minimised case did not reproduce after a round trip");
        return;
      }
      Case c;
      std::string err;
      parseCase(text, c, err);
      uint64_t key = fnv1a(clsName(cls, o.scale) + "\n" + canonicalBody(c));
      auto it = repros.find(key);
      if (it == repros.end()) {
        repros[key] = {text, cls, mode, nodes, steps, {{seed, mode}}};
        shapeCount[clsName(cls, o.scale) + " " + shapeKey(c)]++;
      } else {
        it->second.from.push_back({seed, mode});
      }
      if (o.verbose) printf("  minimised to %d nodes, %d sizes\n", nodes, steps);
    } else if (tag == "N") {
      std::string rest;
      std::getline(ls, rest);
      nonFinite.push_back(rest.substr(1));
      if (json) {
        std::istringstream rs(rest);
        std::string seed, mode, scale, step, path, field, value;
        rs >> seed >> mode >> scale >> step >> path >> field >> value;
        fprintf(json,
                "{\"kind\":\"nonfinite\",\"seed\":%s,\"mode\":%s,\"scale\":\"%s\",\"step\":%s,\"path\":\"%s\","
                "\"field\":\"%s\",\"fresh\":\"%s\"}\n",
                seed.c_str(), mode.c_str(), scale.c_str(), step.c_str(), path.c_str(), field.c_str(), value.c_str());
      }
    } else if (tag == "S") {
      skipped++;
    } else if (tag == "X") {
      std::string rest;
      std::getline(ls, rest);
      problems.push_back(rest);
    }
  };

  auto reap = [&](Job& j, int status, double cpu, bool timedOut) {
    Where& w = where[j.slot];
    // drain what is left
    for (;;) {
      char buf[65536];
      ssize_t n = read(j.fd, buf, sizeof buf);
      if (n <= 0) break;
      j.buf.append(buf, n);
    }
    close(j.fd);
    bool sawDone = false;
    size_t pos;
    while ((pos = j.buf.find('\n')) != std::string::npos) {
      std::string line = j.buf.substr(0, pos);
      j.buf.erase(0, pos + 1);
      if (line.rfind("D ", 0) == 0) sawDone = true;
      handleLine(line);
    }
    char where_[200];
    snprintf(where_, sizeof where_, "mode %d step %d, in %s", w.mode, w.step,
             kPhaseNames[std::max(0, std::min(6, (int)w.phase))]);
    if (timedOut) {
      double wall = now() - j.start;
      char buf[400];
      snprintf(buf, sizeof buf, "TIMEOUT seed %u (%s) after %.1fs wall, %.2fs cpu: %s", j.seed, where_, wall, cpu,
               cpu > 0.5 * wall ? "spinning (CPU-bound)" : "blocked (sleeping, not using CPU)");
      problems.push_back(buf);
      fprintf(stderr, "%s\n", buf);
    } else if (WIFSIGNALED(status) || (WIFEXITED(status) && WEXITSTATUS(status) != 0) || !sawDone) {
      char buf[300];
      if (WIFSIGNALED(status))
        snprintf(buf, sizeof buf, "CRASH seed %u (%s): signal %d", j.seed, where_, WTERMSIG(status));
      else
        snprintf(buf, sizeof buf, "CRASH seed %u (%s): exit %d", j.seed, where_,
                 WIFEXITED(status) ? WEXITSTATUS(status) : -1);
      problems.push_back(buf);
      fprintf(stderr, "%s\n", buf);
    }
    freeSlots.push_back(j.slot);
    done++;
  };

  while (done < total) {
    while ((int)running.size() < jobs && nextSeed <= o.seedHi && nextSeed >= o.seedLo) {
      int fds[2];
      if (pipe(fds) != 0) {
        perror("pipe");
        return 2;
      }
      int slot = freeSlots.back();
      freeSlots.pop_back();
      Where& w = where[slot];
      w.seed = (int)nextSeed;
      w.mode = -1;
      w.step = -1;
      w.phase = P_IDLE;
      w.scale_index = 0;
      fflush(stdout);
      fflush(stderr);
      if (json) fflush(json);
      pid_t pid = fork();
      if (pid < 0) {
        perror("fork");
        return 2;
      }
      if (pid == 0) {
        close(fds[0]);
        for (auto& j : running) close(j.fd);
        // The child leaves the parent's stdio buffers alone: it only writes
        // to its pipe and ends with _exit, which flushes nothing.
        gWhere = &w;
        childSeed(o, nextSeed, fds[1]);
        _exit(0);
      }
      close(fds[1]);
      fcntl(fds[0], F_SETFL, O_NONBLOCK);
      running.push_back({pid, nextSeed, fds[0], slot, now(), ""});
      nextSeed++;
    }
    std::vector<pollfd> pfds;
    for (auto& j : running) pfds.push_back({j.fd, POLLIN, 0});
    poll(pfds.data(), pfds.size(), 100);
    for (size_t i = 0; i < running.size();) {
      Job& j = running[i];
      bool eof = false;
      for (;;) {
        char buf[65536];
        ssize_t n = read(j.fd, buf, sizeof buf);
        if (n > 0) {
          j.buf.append(buf, n);
          continue;
        }
        if (n == 0) eof = true;
        break;
      }
      int status = 0;
      struct rusage ru;
      bool timedOut = false;
      pid_t r = 0;
      if (eof) {
        r = wait4(j.pid, &status, 0, &ru);
      } else if (now() - j.start > timeout) {
        kill(j.pid, SIGKILL);
        r = wait4(j.pid, &status, 0, &ru);
        timedOut = true;
      }
      if (r == j.pid) {
        double cpu = ru.ru_utime.tv_sec + ru.ru_utime.tv_usec * 1e-6 + ru.ru_stime.tv_sec + ru.ru_stime.tv_usec * 1e-6;
        reap(j, status, cpu, timedOut);
        running.erase(running.begin() + i);
      } else {
        i++;
      }
    }
    double t = now();
    if (t - lastProgress >= 1.0 || done == total) {
      lastProgress = t;
      fprintf(stderr, "\r[fuzz] %u/%u seeds, %zu failing (seed,mode), %zu problems, %.0fs   ", done, total,
              fails.size(), problems.size(), t - t0);
      if (done == total) fprintf(stderr, "\n");
    }
  }
  if (json) fclose(json);

  // Summary.
  std::map<int, int> byCls;
  std::map<std::pair<int, int>, int> byClsMode;
  for (auto& f : fails) {
    byCls[f.cls]++;
    byClsMode[{f.cls, f.mode}]++;
  }
  printf("\n== %u seeds (%u-%u), modes", total, o.seedLo, o.seedHi);
  for (int m : o.modes) printf(" %d", m);
  printf(", scales 0 and %s, knobs %s\n", fmtNum(o.scale).c_str(), o.knobs.describe().c_str());
  printf("%zu of %ld (seed,mode) pairs fail (%.2f%%): fail at scale 0 %d, at scale %s %d\n", fails.size(), pairs,
         pairs ? 100.0 * fails.size() / pairs : 0.0, byCls[C_ZERO_ONLY] + byCls[C_BOTH], fmtNum(o.scale).c_str(),
         byCls[C_SCALED_ONLY] + byCls[C_BOTH]);
  for (int cls : {C_SCALED_ONLY, C_BOTH, C_ZERO_ONLY}) {
    printf("  %-14s %5d   by mode:", clsName(cls, o.scale).c_str(), byCls[cls]);
    for (int m : o.modes) printf(" %d:%d", m, byClsMode[{cls, m}]);
    printf("\n");
  }
  printf("measure pressure (most callbacks one leaf got in one layout, per (seed,mode,scale)):");
  long over8 = 0;
  for (auto& [k, v] : callHist)
    if (k >= 8) over8 += v;
  printf(" %ld of %ld runs reach 8 or more\n", over8, pairs * 2);
  int huge = 0;
  for (auto& f : fails) {
    double a = fabs(atof(f.relayout.c_str())), b = fabs(atof(f.fresh.c_str()));
    if (a > 1e5 || b > 1e5 || std::isnan(a) != std::isnan(b)) huge++;
  }
  printf("%d failures have a value above 1e5 or a NaN on one side (runaway sizes)\n", huge);
  if (!nonFinite.empty()) {
    printf("%zu (seed,mode,scale) runs have a NaN or infinite value in a fresh layout (not a relayout\n"
           "difference; check them against --yoga), first ones as seed mode scale step path field value:\n",
           nonFinite.size());
    for (size_t i = 0; i < nonFinite.size() && i < 10; i++) printf("  %s\n", nonFinite[i].c_str());
  }
  if (o.doMin) {
    if (!gPassOn.empty())
      printf("%ld failing (seed,mode) also fail on the --pass-on engine(s); not minimised\n", skipped);
    printf("minimised: %zu distinct repros (%zu distinct shapes)\n", repros.size(), shapeCount.size());
    std::map<int, int> rc, sc;
    for (auto& [k, r] : repros) rc[r.cls]++;
    for (auto& [k, n] : shapeCount) {
      for (int cls : {C_SCALED_ONLY, C_BOTH, C_ZERO_ONLY})
        if (k.rfind(clsName(cls, o.scale) + " ", 0) == 0) sc[cls]++;
    }
    for (int cls : {C_SCALED_ONLY, C_BOTH, C_ZERO_ONLY}) {
      int smallest = -1;
      for (auto& [k, r] : repros)
        if (r.cls == cls && (smallest < 0 || r.nodes < smallest)) smallest = r.nodes;
      printf("  %-14s %4d repros, %4d shapes, smallest %d nodes\n", clsName(cls, o.scale).c_str(), rc[cls], sc[cls],
             smallest);
    }
    if (!o.saveDir.empty()) {
      mkdir(o.saveDir.c_str(), 0755);
      for (auto& [k, r] : repros) {
        char name[200];
        snprintf(name, sizeof name, "%s/%s-m%d-n%02d-%08llx.case", o.saveDir.c_str(), clsName(r.cls, o.scale).c_str(),
                 r.mode, r.nodes, (unsigned long long)(k & 0xffffffffull));
        std::ofstream f(name);
        f << r.text;
        f << "# also found from:";
        for (auto& [s, m] : r.from) f << " " << s << "/" << m;
        f << " (seed/mode)\n";
      }
      printf("saved to %s/\n", o.saveDir.c_str());
    }
  }
  if (!problems.empty()) {
    printf("%zu problems:\n", problems.size());
    for (auto& p : problems) printf("  %s\n", p.c_str());
  }
  return problems.empty() ? 0 : 1;
}

// `calls`: the measure callbacks of each case's relayout sequence (every size
// step, no fresh layouts, nothing compared), one line per (seed, mode, scale):
// `seed mode scale total c1,c2,...` (c_i: callbacks of the layout at step i).
// fuzz.py's `calls-compare` lines up two engines' outputs. Report-only.
static int cmdCalls(const Options& o) {
  for (uint32_t seed = o.seedLo; seed <= o.seedHi; seed++) {
    Case base = generate(seed, o.knobs);
    for (int mode : o.modes) {
      for (float scale : {0.0f, o.scale}) {
        Case c = withScale(base, scale);
        c.mode = mode;
        YGConfigRef cfg = YGConfigNew();
        YGConfigSetPointScaleFactor(cfg, c.scale);
        YGConfigSetErrata(cfg, (YGErrata)c.errata);
        std::string steps;
        long total = 0;
        {
          Built tree(c.root, cfg);
          for (size_t i = 0; i < c.sizes.size(); i++) {
            layoutAt(tree.root, c.mode, c.sizes[i].first, c.sizes[i].second);
            int n = tree.sumCalls();
            tree.resetCalls();
            total += n;
            steps += (i ? "," : "") + std::to_string(n);
          }
        }
        YGConfigFree(cfg);
        printf("%u %d %s %ld %s\n", seed, mode, fmtNum(scale).c_str(), total, steps.c_str());
      }
    }
  }
  return 0;
}

int main(int argc, char** argv) {
  Options o;
  if (!parseArgs(argc, argv, o)) {
    usage();
    return 2;
  }
  if (o.cmd == "run") return cmdRun(o);
  if (o.cmd == "show") return cmdShow(o);
  if (o.cmd == "check") return cmdCheck(o);
  if (o.cmd == "min") return cmdMin(o);
  if (o.cmd == "snippet") return cmdSnippet(o);
  if (o.cmd == "calls") return cmdCalls(o);
  usage();
  return 2;
}
