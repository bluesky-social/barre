// Link shim for Yoga's internal event API, used only when the suite is built
// against barre (never with --yoga, where yoga/event/event.cpp is linked).
//
// vendor/yoga/tests/util/TestUtil.cpp (needed by the generated tests for
// IntrinsicSizeMeasure) references facebook::yoga::Event::subscribe/reset.
// barre's C ABI has no C++ event API, so this shim satisfies the linker with a
// subscriber list that is never published to. Tests whose assertions depend
// on events (node counting, EventsTest) are listed in conformance/skip.txt:
// with this shim they would observe nothing, which could pass vacuously.

#include <yoga/event/event.h>

#include <functional>
#include <vector>

namespace facebook::yoga {

namespace {
std::vector<std::function<Event::Subscriber>>& subscribers() {
  static std::vector<std::function<Event::Subscriber>> s;
  return s;
}
} // namespace

void Event::reset() {
  subscribers().clear();
}

void Event::subscribe(std::function<Subscriber>&& subscriber) {
  subscribers().push_back(std::move(subscriber));
}

void Event::publish(YGNodeConstRef node, Type eventType, const Data& eventData) {
  for (auto& s : subscribers()) {
    s(node, eventType, eventData);
  }
}

} // namespace facebook::yoga
