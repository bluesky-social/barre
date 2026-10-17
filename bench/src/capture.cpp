// See capture.h. The JSON → C API code is vendor/yoga/benchmark/Benchmark.cpp
// (MIT, Meta Platforms) restructured so the JSON is parsed once and the tree
// can be rebuilt cheaply per iteration. The string→enum helpers come from
// Yoga's own TreeDeserialization.cpp, compiled unmodified.

#include "capture.h"

#include <cstdio>
#include <fstream>
#include <stdexcept>

#include <benchmark/TreeDeserialization.h>
#include <capture/CaptureTree.h>
#include <nlohmann/json.hpp>

using nlohmann::json;

extern uint64_t gMeasureCalls; // bench.cpp
using facebook::yoga::SerializedMeasureFunc;

namespace bench {

namespace {

// Per-node measure context: the recorded (input → output) pairs.
struct MeasureCtx {
  std::vector<SerializedMeasureFunc> fns;
  uint64_t* misses;
};

bool inputsMatch(
    float aw, float ew, float ah, float eh,
    YGMeasureMode awm, YGMeasureMode ewm, YGMeasureMode ahm, YGMeasureMode ehm) {
  bool wEq = (YGFloatIsUndefined(aw) && YGFloatIsUndefined(ew)) || aw == ew;
  bool hEq = (YGFloatIsUndefined(ah) && YGFloatIsUndefined(eh)) || ah == eh;
  return wEq && hEq && awm == ewm && ahm == ehm;
}

YGSize replayMeasure(
    YGNodeConstRef node, float w, YGMeasureMode wm, float h, YGMeasureMode hm) {
  gMeasureCalls++;
  auto* ctx = static_cast<MeasureCtx*>(YGNodeGetContext(node));
  for (const auto& f : ctx->fns) {
    if (inputsMatch(w, f.inputWidth, h, f.inputHeight, wm, f.widthMode, hm,
                    f.heightMode)) {
      return {f.outputWidth, f.outputHeight};
    }
  }
  // Benchmark.cpp prints and returns {10, 10}; we count instead. A miss means
  // the engine asked a question the captured Yoga never asked.
  (*ctx->misses)++;
  return {10.0f, 10.0f};
}

} // namespace

// Pre-parsed node: everything needed to rebuild it without touching JSON.
struct Capture::Impl {
  struct Node {
    json config;
    json style;
    bool hasAlwaysFormsCB = false;
    bool alwaysFormsCB = false;
    std::unique_ptr<MeasureCtx> measure;
    std::vector<Node> children;
  };
  Node root;
  uint64_t* misses = nullptr;
};

namespace {

using namespace facebook::yoga;

YGConfigRef buildConfig(const json& jsonConfig) {
  YGConfigRef config = YGConfigNew();
  for (auto it = jsonConfig.begin(); it != jsonConfig.end(); it++) {
    if (it.key() == "use-web-defaults") {
      YGConfigSetUseWebDefaults(config, it.value());
    } else if (it.key() == "point-scale-factor") {
      YGConfigSetPointScaleFactor(config, it.value());
    } else if (it.key() == "errata") {
      YGConfigSetErrata(config, errataFromString(it.value()));
    } else if (it.key() == "experimental-features") {
      for (auto efIt = it.value().begin(); efIt != it.value().end(); efIt++) {
        YGConfigSetExperimentalFeatureEnabled(
            config, experimentalFeatureFromString(efIt.value()), true);
      }
    }
  }
  return config;
}

std::string edgeString(const std::string& key, const std::string& prop) {
  return key.substr(prop.length() + 1);
}

// Verbatim from Benchmark.cpp's setStylesFromJson.
void setStyles(const json& style, YGNodeRef node) {
  for (const auto& [key, value] : style.items()) {
    json v = value; // the helpers take non-const refs
    if (key == "flex-direction") {
      YGNodeStyleSetFlexDirection(node, flexDirectionFromString(value));
    } else if (key == "justify-content") {
      YGNodeStyleSetJustifyContent(node, justifyContentFromString(value));
    } else if (key == "align-items") {
      YGNodeStyleSetAlignItems(node, alignFromString(value));
    } else if (key == "align-content") {
      YGNodeStyleSetAlignContent(node, alignFromString(value));
    } else if (key == "align-self") {
      YGNodeStyleSetAlignSelf(node, alignFromString(value));
    } else if (key == "flex-wrap") {
      YGNodeStyleSetFlexWrap(node, wrapFromString(value));
    } else if (key == "overflow") {
      YGNodeStyleSetOverflow(node, overflowFromString(value));
    } else if (key == "display") {
      YGNodeStyleSetDisplay(node, displayFromString(value));
    } else if (key == "position-type") {
      YGNodeStyleSetPositionType(node, positionTypeFromString(value));
    } else if (key == "flex-grow") {
      YGNodeStyleSetFlexGrow(node, value);
    } else if (key == "flex-shrink") {
      YGNodeStyleSetFlexShrink(node, value);
    } else if (key == "flex") {
      YGNodeStyleSetFlex(node, value);
    } else if (key == "flex-basis") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitAuto) {
        YGNodeStyleSetFlexBasisAuto(node);
      } else if (unit == YGUnitPoint) {
        YGNodeStyleSetFlexBasis(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetFlexBasisPercent(node, value["value"]);
      }
    } else if (key.starts_with("position")) {
      YGEdge edge = edgeFromString(edgeString(key, "position"));
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetPosition(node, edge, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetPositionPercent(node, edge, value["value"]);
      }
    } else if (key.starts_with("padding")) {
      YGEdge edge = edgeFromString(edgeString(key, "padding"));
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetPadding(node, edge, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetPaddingPercent(node, edge, value["value"]);
      }
    } else if (key.starts_with("border")) {
      YGEdge edge = edgeFromString(edgeString(key, "border"));
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetBorder(node, edge, value["value"]);
      }
    } else if (key.starts_with("margin")) {
      YGEdge edge = edgeFromString(edgeString(key, "margin"));
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetMargin(node, edge, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetMarginPercent(node, edge, value["value"]);
      } else if (unit == YGUnitAuto) {
        YGNodeStyleSetMarginAuto(node, edge);
      }
    } else if (key == "gap") {
      if (unitFromJson(v) == YGUnitPoint) {
        YGNodeStyleSetGap(node, YGGutterAll, value["value"]);
      }
    } else if (key == "column-gap") {
      if (unitFromJson(v) == YGUnitPoint) {
        YGNodeStyleSetGap(node, YGGutterColumn, value["value"]);
      }
    } else if (key == "row-gap") {
      if (unitFromJson(v) == YGUnitPoint) {
        YGNodeStyleSetGap(node, YGGutterRow, value["value"]);
      }
    } else if (key == "height") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitAuto) {
        YGNodeStyleSetHeightAuto(node);
      } else if (unit == YGUnitPoint) {
        YGNodeStyleSetHeight(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetHeightPercent(node, value["value"]);
      }
    } else if (key == "width") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitAuto) {
        YGNodeStyleSetWidthAuto(node);
      } else if (unit == YGUnitPoint) {
        YGNodeStyleSetWidth(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetWidthPercent(node, value["value"]);
      }
    } else if (key == "min-height") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetMinHeight(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetMinHeightPercent(node, value["value"]);
      }
    } else if (key == "min-width") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetMinWidth(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetMinWidthPercent(node, value["value"]);
      }
    } else if (key == "max-height") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetMaxHeight(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetMaxHeightPercent(node, value["value"]);
      }
    } else if (key == "max-width") {
      YGUnit unit = unitFromJson(v);
      if (unit == YGUnitPoint) {
        YGNodeStyleSetMaxWidth(node, value["value"]);
      } else if (unit == YGUnitPercent) {
        YGNodeStyleSetMaxWidthPercent(node, value["value"]);
      }
    }
  }
}

void parseNode(const json& j, Capture::Impl::Node& out, uint64_t* misses) {
  out.config = j.contains("config") ? j["config"] : json::object();
  out.style = j.contains("style") ? j["style"] : json::object();
  if (j.contains("node") && !j["node"].is_null()) {
    for (auto it = j["node"].begin(); it != j["node"].end(); it++) {
      if (it.key() == "always-forms-containing-block") {
        out.hasAlwaysFormsCB = true;
        out.alwaysFormsCB = it.value();
      } else if (it.key() == "measure-funcs" && !it.value().is_null()) {
        out.measure = std::make_unique<MeasureCtx>();
        out.measure->misses = misses;
        for (auto mf : it.value()) {
          out.measure->fns.push_back(serializedMeasureFuncFromJson(mf));
        }
      }
    }
  }
  if (j.contains("children")) {
    for (const json& c : j["children"]) {
      out.children.emplace_back();
      parseNode(c, out.children.back(), misses);
    }
  }
}

// Same order of operations as Benchmark.cpp's buildTreeFromJson: new node with
// its own config, node state, insert into parent, styles, then children.
YGNodeRef buildNode(
    const Capture::Impl::Node& n,
    YGNodeRef parent,
    size_t index,
    std::vector<YGConfigRef>& configs) {
  YGConfigRef config = buildConfig(n.config);
  configs.push_back(config);
  YGNodeRef node = YGNodeNewWithConfig(config);
  if (n.hasAlwaysFormsCB) {
    YGNodeSetAlwaysFormsContainingBlock(node, n.alwaysFormsCB);
  }
  if (n.measure) {
    YGNodeSetContext(node, n.measure.get());
    YGNodeSetMeasureFunc(node, replayMeasure);
  }
  if (parent != nullptr) {
    YGNodeInsertChild(parent, node, index);
  }
  setStyles(n.style, node);
  for (size_t i = 0; i < n.children.size(); i++) {
    buildNode(n.children[i], node, i, configs);
  }
  return node;
}

} // namespace

CaptureTree::~CaptureTree() {
  if (root_ != nullptr) {
    YGNodeFreeRecursive(root_);
  }
  for (YGConfigRef c : configs_) {
    YGConfigFree(c);
  }
}

Capture::~Capture() = default;

std::unique_ptr<CaptureTree> Capture::build() const {
  auto t = std::make_unique<CaptureTree>();
  t->root_ = buildNode(impl->root, nullptr, 0, t->configs_);
  return t;
}

std::unique_ptr<Capture> loadCapture(const std::string& path, uint64_t* misses) {
  std::ifstream f(path);
  if (!f) {
    fprintf(stderr, "cannot open capture %s (pass --captures DIR)\n", path.c_str());
    exit(2);
  }
  json j = json::parse(f);
  auto c = std::make_unique<Capture>();
  c->impl = std::make_unique<Capture::Impl>();
  c->impl->misses = misses;
  parseNode(j["tree"], c->impl->root, misses);
  json in = j["layout-inputs"];
  c->availableWidth = in["available-width"];
  c->availableHeight = in["available-height"];
  c->direction = directionFromString(in["owner-direction"]);
  return c;
}

} // namespace bench
