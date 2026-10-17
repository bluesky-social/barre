/*
 * Ported from vendor/yoga/tests/YGPersistentNodeCloningTest.cpp at Yoga
 * 52f6d0bd3824296b8e852cb4fcdf3b9821ab2aa5 (the vendor/yoga pin). The
 * original is skipped (conformance/skip.txt) because its NodeWrapper builds
 * trees through yoga::Node (insertChild, setChildren, setOwner, setDirty),
 * which lets it put a node into a parent without taking ownership of it.
 * Test names and assertions are kept; NodeWrapper is rewritten through the
 * C API, producing the same child lists and ownership:
 *
 * - a new node takes ownership of its children (YGNodeInsertChild); the
 *   original also only claims children that have no owner yet, and no test
 *   gives a new node an owned child except as noted in
 *   clone_leaf_display_contents_node;
 * - a clone with current children is YGNodeClone;
 * - a clone with new children starts from YGNodeClone (which shares the
 *   source's children without owning them), removes the shared children that
 *   are not in the new list (YGNodeRemoveChild leaves a child it does not own
 *   untouched) and inserts the new, unowned ones in place. Shared children
 *   must therefore be children of the source, in the same order: the
 *   constructor asserts that. Like the original's setDirty(true), the
 *   mutations dirty the clone.
 */

#include <gtest/gtest.h>
#include <yoga/Yoga.h>

#include <functional>
#include <memory>
#include <vector>

struct YGPersistentNodeCloningTest : public ::testing::Test {
  struct NodeWrapper {
    explicit NodeWrapper(
        YGConfigRef config,
        std::vector<std::shared_ptr<NodeWrapper>> children = {})
        : node{YGNodeNewWithConfig(config)}, children{std::move(children)} {
      YGNodeSetContext(node, this);

      for (const auto& child : this->children) {
        EXPECT_EQ(YGNodeGetOwner(child->node), nullptr)
            << "a new node can only take unowned children through the C API";
        YGNodeInsertChild(node, child->node, YGNodeGetChildCount(node));
      }
    }

    // Clone, with current children, for mutation
    NodeWrapper(const NodeWrapper& other)
        : node{YGNodeClone(other.node)}, children{other.children} {
      YGNodeSetContext(node, this);
      EXPECT_EQ(YGNodeGetOwner(node), nullptr);
    }

    // Clone, with new children
    NodeWrapper(
        const NodeWrapper& other,
        std::vector<std::shared_ptr<NodeWrapper>> children)
        : node{YGNodeClone(other.node)}, children{std::move(children)} {
      YGNodeSetContext(node, this);
      EXPECT_EQ(YGNodeGetOwner(node), nullptr);

      // Drop the shared children that are not in the new list.
      for (const auto& old : other.children) {
        bool kept = false;
        for (const auto& child : this->children) {
          kept = kept || child == old;
        }
        if (!kept) {
          YGNodeRemoveChild(node, old->node);
        }
      }
      // Insert the new children in place; the kept ones are already there.
      for (size_t i = 0; i < this->children.size(); i++) {
        YGNodeRef child = this->children[i]->node;
        if (YGNodeGetOwner(child) == nullptr) {
          YGNodeInsertChild(node, child, i);
        }
      }
      EXPECT_EQ(YGNodeGetChildCount(node), this->children.size());
      for (size_t i = 0; i < this->children.size(); i++) {
        EXPECT_EQ(YGNodeGetChild(node, i), this->children[i]->node)
            << "shared children must keep their order";
      }
    }

    NodeWrapper(NodeWrapper&&) = delete;

    ~NodeWrapper() {
      YGNodeFree(node);
    }

    NodeWrapper& operator=(const NodeWrapper& other) = delete;
    NodeWrapper& operator=(NodeWrapper&& other) = delete;

    YGNodeRef node;
    std::vector<std::shared_ptr<NodeWrapper>> children;
  };

  struct ConfigWrapper {
    ConfigWrapper() {
      YGConfigSetCloneNodeFunc(
          config,
          [](YGNodeConstRef oldNode, YGNodeConstRef owner, size_t childIndex) {
            onClone(oldNode, owner, childIndex);
            auto wrapper = static_cast<NodeWrapper*>(YGNodeGetContext(owner));
            auto old = static_cast<NodeWrapper*>(YGNodeGetContext(oldNode));

            wrapper->children[childIndex] = std::make_shared<NodeWrapper>(*old);
            return wrapper->children[childIndex]->node;
          });
    }

    ConfigWrapper(const ConfigWrapper&) = delete;
    ConfigWrapper(ConfigWrapper&&) = delete;

    ~ConfigWrapper() {
      YGConfigFree(config);
    }

    ConfigWrapper& operator=(const ConfigWrapper&) = delete;
    ConfigWrapper& operator=(ConfigWrapper&&) = delete;

    YGConfigRef config{YGConfigNew()};
  };

  ConfigWrapper configWrapper;
  YGConfigRef config{configWrapper.config};

  void SetUp() override {
    onClone = [](...) {};
  }

  // NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
  static inline std::function<void(YGNodeConstRef, YGNodeConstRef, size_t)>
      onClone;
};

TEST_F(
    YGPersistentNodeCloningTest,
    changing_sibling_height_does_not_clone_neighbors) {
  // <ScrollView>
  //   <View id="Sibling" style={{ height: 1 }} />
  //   <View id="A" style={{ height: 1 }}>
  //     <View id="B">
  //       <View id="C">
  //         <View id="D"/>
  //       </View>
  //     </View>
  //   </View>
  // </ScrollView>

  auto sibling = std::make_shared<NodeWrapper>(config);
  YGNodeStyleSetHeight(sibling->node, 1);

  auto d = std::make_shared<NodeWrapper>(config);
  auto c = std::make_shared<NodeWrapper>(config, std::vector{d});
  auto b = std::make_shared<NodeWrapper>(config, std::vector{c});
  auto a = std::make_shared<NodeWrapper>(config, std::vector{b});
  YGNodeStyleSetHeight(a->node, 1);

  auto scrollContentView =
      std::make_shared<NodeWrapper>(config, std::vector{sibling, a});
  YGNodeStyleSetPositionType(scrollContentView->node, YGPositionTypeAbsolute);

  auto scrollView =
      std::make_shared<NodeWrapper>(config, std::vector{scrollContentView});
  YGNodeStyleSetWidth(scrollView->node, 100);
  YGNodeStyleSetHeight(scrollView->node, 100);

  // We don't expect any cloning during the first layout
  onClone = [](...) { FAIL(); };

  YGNodeCalculateLayout(
      scrollView->node, YGUndefined, YGUndefined, YGDirectionLTR);

  auto siblingPrime = std::make_shared<NodeWrapper>(config);
  YGNodeStyleSetHeight(siblingPrime->node, 2);

  auto scrollContentViewPrime = std::make_shared<NodeWrapper>(
      *scrollContentView, std::vector{siblingPrime, a});
  auto scrollViewPrime = std::make_shared<NodeWrapper>(
      *scrollView, std::vector{scrollContentViewPrime});

  std::vector<NodeWrapper*> nodesCloned;
  // We should only need to clone "A"
  onClone = [&](YGNodeConstRef oldNode,
                YGNodeConstRef /*owner*/,
                size_t /*childIndex*/) {
    nodesCloned.push_back(static_cast<NodeWrapper*>(YGNodeGetContext(oldNode)));
  };

  YGNodeCalculateLayout(
      scrollViewPrime->node, YGUndefined, YGUndefined, YGDirectionLTR);

  EXPECT_EQ(nodesCloned.size(), 1);
  EXPECT_EQ(nodesCloned[0], a.get());
}

TEST_F(YGPersistentNodeCloningTest, clone_leaf_display_contents_node) {
  // <View id="A">
  //   <View id="B" style={{ display: 'contents' }} />
  // </View>

  auto b = std::make_shared<NodeWrapper>(config);
  auto a = std::make_shared<NodeWrapper>(config, std::vector{b});
  YGNodeStyleSetDisplay(b->node, YGDisplayContents);

  // We don't expect any cloning during the first layout
  onClone = [](...) { FAIL(); };

  YGNodeCalculateLayout(a->node, YGUndefined, YGUndefined, YGDirectionLTR);

  // The original makes A' a new node holding B (still owned by A). A new node
  // cannot hold a child it does not own through the C API, so A' is a clone
  // of A with children {B}: A has the default style of a new node, so the
  // tree is the same.
  auto aPrime = std::make_shared<NodeWrapper>(*a, std::vector{b});

  std::vector<NodeWrapper*> nodesCloned;
  // We should clone "C"
  onClone = [&](YGNodeConstRef oldNode,
                YGNodeConstRef /*owner*/,
                size_t /*childIndex*/) {
    nodesCloned.push_back(static_cast<NodeWrapper*>(YGNodeGetContext(oldNode)));
  };

  YGNodeCalculateLayout(aPrime->node, 100, 100, YGDirectionLTR);

  EXPECT_EQ(nodesCloned.size(), 1);
  EXPECT_EQ(nodesCloned[0], b.get());
}
