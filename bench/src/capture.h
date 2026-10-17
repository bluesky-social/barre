// Replays Yoga's benchmark captures (vendor/yoga/benchmark/captures/*.json)
// through the C API. Adapted from vendor/yoga/benchmark/Benchmark.cpp
// (Copyright (c) Meta Platforms, Inc. and affiliates, MIT licence), with one
// deliberate change: recorded measure funcs return their recorded output
// without sleeping for the recorded duration, so the numbers measure the
// engine rather than std::this_thread::sleep_for.

#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include <yoga/Yoga.h>

namespace bench {

class CaptureTree {
 public:
  ~CaptureTree();
  YGNodeRef root() const { return root_; }

 private:
  friend class Capture;
  YGNodeRef root_ = nullptr;
  std::vector<YGConfigRef> configs_;
};

class Capture {
 public:
  ~Capture();
  float availableWidth = 0;
  float availableHeight = 0;
  YGDirection direction = YGDirectionLTR;
  std::unique_ptr<CaptureTree> build() const;

  struct Impl;
  std::unique_ptr<Impl> impl;
};

std::unique_ptr<Capture> loadCapture(const std::string& path, uint64_t* misses);

} // namespace bench
