// The two things barre-ffi cannot do in Rust:
//
// - call a YGLogger, which takes a va_list (Rust cannot build one on stable);
// - throw std::logic_error, which is what Yoga's fatal assertions do and
//   what Yoga's test suite asserts with ASSERT_THROW.
//
// Both are reached only from Rust (crates/barre-ffi/src/lib.rs).

#include <cstdarg>
#include <cstddef>
#include <stdexcept>
#include <string>

extern "C" {

typedef int (*BarreLogger)(
    const void* config,
    const void* node,
    int level,
    const char* format,
    va_list args);

static int barre_call_logger_v(
    BarreLogger logger,
    const void* config,
    const void* node,
    int level,
    const char* format,
    ...) {
  va_list args;
  va_start(args, format);
  const int result = logger(config, node, level, format, args);
  va_end(args);
  return result;
}

// Calls `logger` with format "%s" and `message` as its only argument.
int barre_call_logger(
    BarreLogger logger,
    const void* config,
    const void* node,
    int level,
    const char* message) {
  return barre_call_logger_v(logger, config, node, level, "%s", message);
}

[[noreturn]] void barre_throw_logic_error(const char* message, size_t length) {
  throw std::logic_error(std::string(message, length));
}

} // extern "C"
