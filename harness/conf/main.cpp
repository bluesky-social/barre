// gtest main for the conformance runner (./conf, harness/conf/conf.py).
//
// Replaces gtest_main. Differences:
// - CONF_NONCE (env, removed before any test runs): after the tests ran, a
//   line "CONF-RESULT <nonce> ran=N passed=N failed=N skipped=N" is printed.
//   The runner only counts a test as passing if this line, with the nonce it
//   chose for this process, says exactly one test ran and passed.
// - CONF_MEM_LIMIT_MB (env, removed): memory cap. On Linux it is an
//   RLIMIT_AS; macOS has no working address-space limit, so a watchdog thread
//   polls the process footprint and exits with code 97 when it is exceeded.

#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <thread>

#if defined(__APPLE__)
#include <mach/mach.h>
#else
#include <sys/resource.h>
#endif

namespace {

constexpr int kMemExitCode = 97;

long long footprintBytes() {
#if defined(__APPLE__)
  task_vm_info_data_t info;
  mach_msg_type_number_t count = TASK_VM_INFO_COUNT;
  if (task_info(
          mach_task_self(),
          TASK_VM_INFO,
          reinterpret_cast<task_info_t>(&info),
          &count) != KERN_SUCCESS) {
    return 0;
  }
  return static_cast<long long>(info.phys_footprint);
#else
  FILE* f = std::fopen("/proc/self/statm", "r");
  if (f == nullptr) {
    return 0;
  }
  long long pages = 0;
  long long resident = 0;
  if (std::fscanf(f, "%lld %lld", &pages, &resident) != 2) {
    resident = 0;
  }
  std::fclose(f);
  return resident * 4096;
#endif
}

void startMemoryWatchdog(long long limitMb) {
  const long long limit = limitMb * 1024 * 1024;
#if !defined(__APPLE__)
  struct rlimit rl;
  rl.rlim_cur = static_cast<rlim_t>(limit) + (256LL << 20);
  rl.rlim_max = rl.rlim_cur;
  setrlimit(RLIMIT_AS, &rl);
#endif
  std::thread([limit, limitMb] {
    for (;;) {
      if (footprintBytes() > limit) {
        std::fprintf(
            stderr, "\nconf: memory limit of %lld MB exceeded\n", limitMb);
        std::fflush(stderr);
        std::_Exit(kMemExitCode);
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(5));
    }
  }).detach();
}

std::string takeEnv(const char* name) {
  const char* v = std::getenv(name);
  std::string out = v != nullptr ? v : "";
  unsetenv(name);
  return out;
}

} // namespace

int main(int argc, char** argv) {
  const std::string nonce = takeEnv("CONF_NONCE");
  const std::string mem = takeEnv("CONF_MEM_LIMIT_MB");
  if (!mem.empty() && std::atoll(mem.c_str()) > 0) {
    startMemoryWatchdog(std::atoll(mem.c_str()));
  }

  testing::InitGoogleTest(&argc, argv);
  const int rc = RUN_ALL_TESTS();

  if (!nonce.empty()) {
    const auto* u = testing::UnitTest::GetInstance();
    std::fflush(stderr);
    std::printf(
        "\nCONF-RESULT %s ran=%d passed=%d failed=%d skipped=%d\n",
        nonce.c_str(),
        u->test_to_run_count(),
        u->successful_test_count(),
        u->failed_test_count(),
        u->skipped_test_count());
    std::fflush(stdout);
  }
  return rc;
}
