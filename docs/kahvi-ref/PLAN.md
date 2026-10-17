# kahvi — plan

A JavaScript engine in Rust, written mostly by cheap models (Haiku) under a
harness that makes cheating unprofitable and measures progress against the
official conformance suite.

## Thesis

1. **test262 is the only judge.** It is external, pinned (`vendor/test262`)
   and unmodifiable by agents. The pass count is the score and it only goes up.
2. **The spec is pseudocode.** Every builtin is a numbered algorithm. If our
   code mirrors spec naming, a ticket is "translate these 12 steps", not
   "design something". Spec `?` (ReturnIfAbrupt) ≈ Rust `?`.
3. **test262's directory tree is the ticket breakdown.**
   `built-ins/Array/prototype/flatMap/` is one ticket with an objective
   done-condition.

## Pipeline

```
architect (Opus) ─┐
triage (Haiku)  ──┴─▶ ticket pool ─▶ Haiku workers ×N   (own worktree, file-scoped)
                                          │ diff only
                                          ▼
                   mechanical gates: scope · lint · build · test262 delta
                                          │ diff + numbers (no agent narrative)
                                          ▼
                                   blind verifier
       good / incomplete ─▶ merge bot ─▶ main, ratchet baseline
       salvageable ───────▶ back to pool with diff + verifier notes
       garbage ───────────▶ discard; 2 strikes → escalate to Sonnet or split
                                          │
      failing test262 on main ─▶ clustered by error ─▶ new tickets (loop)
```

| Role | Model | When | Job |
|---|---|---|---|
| Architect | Opus | phase starts, every ~50 merges, escalations | spine code, docs, skeleton, writes/splits tickets |
| Triage | Haiku | continuously | clusters failures by error → drafts tickets from a template |
| Worker | Haiku | the bulk | one ticket, one scope, fresh session |
| Verifier | Haiku screens every submission; Sonnet confirms anything that would merge | every submission | blind verdict, binding checklist |
| Auditor | Opus | every ~50 merges | samples merged code for rot, tightens lints |
| Merge bot | script | every verdict | rebase, full suite, ratchet, merge |

## Engine architecture (fixed up front; workers never make design calls)

- **Bytecode VM, stack-based.** Generators/async are suspended frames.
- **Spec-mirroring code.** Abstract ops named after the spec
  (`to_property_key`, `ordinary_define_own_property`, …), every step commented
  with its spec text.
- **Handles, not references.** `ObjectId(u32)` into an arena. No lifetimes or
  `unsafe` in any API a worker touches. Leak-everything heap first; real GC is
  a later architect-tier ticket.
- **UTF-16 strings** from day one.
- **Skeleton-first.** A generator emits every builtin as a stub from
  `spec.html` with steps as comments and correct `name`/`length`/attributes.
  Stubs throw `InternalError: not implemented: <name>`, which the runner
  counts separately. A ticket = fill one stub.
- **Crates:** `ast`, `parser`, `compiler`, `vm`, `builtins`, `regexp`, `host`,
  `cli`; `runtime` later.
- **Parser scoreboard from day one** (test262 parse-only + negative syntax
  tests), since nothing in test262 proper passes until `assert.js` runs.

### Embeddability (so React Native via JSI stays possible)

Cheap to honour now, expensive to retrofit:

- **Library first.** `kahvi` is an embedding API (create runtime, evaluate,
  call, host functions, host objects); the CLI is just one host.
- **No process-global state.** Everything lives in the runtime; several
  runtimes per process. No `Rc` in the runtime, so it is `Send` (JSI creates a
  runtime on one thread and uses it on another).
- **The host owns the outside world.** Job queue draining, promise rejection
  tracking, module loading, clocks and I/O are host hooks. The engine crates
  never touch files, env or processes (the lint already enforces this).
- **ESM only.** Modules are ECMAScript modules: no CommonJS anywhere (no
  `require`, `module`, `exports`, `.cjs`, no CJS interop in the loader). The
  runtime's entry file is a module. Scripts stay, because the spec and
  test262 need them; Metro bundles are scripts with their own registry, so
  React Native does not need CJS either.
- **Host objects.** Exotic-object dispatch lets the host implement
  [[Get]]/[[Set]]/[[OwnPropertyKeys]] (JSI `HostObject`). ArrayBuffers can
  wrap host-owned memory.
- **Explicit roots.** Values the host holds go through rooted handles, so a
  real GC can arrive later without changing the API.
- **Source positions in bytecode**, for `error.stack`.

## Anti-faking

Deterministic gates run first; an LLM looks only at what survives.

- Diff confined to the ticket's `scope`; protected paths untouched
  (`vendor/`, `harness/`, `conformance/`, `.claude/`, `Cargo.toml`/`Cargo.lock`,
  CI).
- Added-line lint: `todo!`/`unimplemented!`/`panic!`, test262/harness strings
  outside the host crate, `TODO|for now|simplified|stub|hack|placeholder`,
  new `#[test]`, `#[allow]`, `unsafe`, `std::process`.
- Build + clippy `-D warnings`; target tests before/after; full suite at merge
  with zero regressions (regressions re-run once).
- All numbers computed by the orchestrator in a clean checkout. The worker's
  own claims are never read.
- Runner hardening: tests arrive at random temp paths; positive tests get a
  random completion nonce appended that must be printed; randomized should-fail
  canaries invalidate the run if they "pass"; the engine runs with an empty
  environment; crashes/timeouts are failures.
- **Blind verifier** sees ticket + spec text + diff + gate report, never the
  worker's transcript. It does not pick a verdict: it answers yes/no questions
  (special-cased inputs? made-up results? swallowed errors? skipped observable
  steps? contradicts the spec? unrelated changes? worth keeping?) and code
  derives the decision: `accept` (all no → merge), `revise` (defects but worth
  keeping → next attempt starts from it), `reject` (fakery or not worth
  keeping → discard, strike). Whether a ticket is *finished* is decided by its
  test262 target, not the verifier. Fakery escalates the ticket a tier at once.
  (The first version had model-picked labels good/incomplete/salvageable/
  garbage; calibration showed models describing a defect and then picking a
  lenient label anyway.)
- **Verifier cascade:** Haiku screens every attempt; Sonnet re-judges anything
  that would merge; the stricter decision wins.
- **Verifier calibration:** a red-team set of planted cheats and good diffs
  (`harness/redteam/`). A configuration that accepts any plant is not used.
- **Honest escape hatch:** `BLOCKED: <reason>` is not penalised and files a
  dependency ticket.

## Test policy

- Workers cannot write tests. Conformance is test262's job.
- Architect-owned invariant tests only (heap, interning), plus later a small
  end-to-end JS suite for runtime features test262 doesn't cover. Each must
  state the bug it catches that test262 wouldn't.

## Cost levers

- Haiku 5.5 bills 5× above 100K-token prompts → worker contexts stay under
  100K: fresh session per ticket, `read` hints in tickets, `docs/API.md` cheat
  sheet, small files, turn cap.
- Identical static prompt prefix across workers → cache hits.
- Track $ per newly passing test, per ticket kind; route kinds with high
  Haiku garbage rates straight to Sonnet.
- Compile time is the real bottleneck: persistent slots with warm `target/`.

## Usage limits and where the swarm runs

- **Subscription plans (Pro/Max).** Every run reports the plan's usage windows
  (5-hour, 7-day). The orchestrator stops starting work when a window passes
  its cap (default 90% / 80%, leaving the rest for the human sharing the plan),
  waits for the reset, and resumes. Pauses persist across restarts. A run cut
  off by a limit doesn't count against its ticket: its partial diff seeds the
  next attempt, and an attempt that only lacked verification resumes there.
- **Scale follows billing.** On a subscription, every worker shares one
  account's windows, so more than a few slots buys nothing: one persistent box
  is right. With API billing the limit becomes money and compute. Then slots
  can move to a container platform (e.g. Cloudflare Containers, up to
  4 vCPU / 12 GiB / 20 GB each, many in parallel). That needs an image with
  the toolchain, the `claude` CLI and test262, plus a shared build cache
  (sccache in object storage). The orchestrator and merge bot stay central.
  The merge bot's full-suite run is serial, so at that scale merges get
  batched (test N accepted attempts together, bisect on regression).

## Milestones

- **M0 Harness** — workspace, pinned test262 + spec, runner + ratchet, spec
  lookup, orchestrator + gates + verifier + merge bot, prompts, red-team set.
  Exit: a ticket flows end to end; every planted cheat is rejected.
- **M1 Spine** (architect, serial) — values/objects/realm, core abstract ops,
  parser + compiler + VM for what `assert.js`/`sta.js` need, `$262` host,
  skeleton generator, `docs/API.md` + golden builtins. Exit: harness loads,
  first few % pass, ~300 tickets queued.
- **M2 Fan-out** (Haiku swarm) — builtins, parser track, simple language
  features; triage keeps the pool full.
- **M3 Hard tail** (Sonnet/Opus) — generators/async, Proxy/Reflect, RegExp
  engine, modules, Date, number→string, Annex B, GC.
- **M4 Runtime** — event loop, console, timers, ESM loading, own e2e suite.
- **M5 (stretch) React Native via JSI** — RN talks to its engine through JSI,
  a C++ `jsi::Runtime` interface; Hermes is just the default implementation
  (react-native-v8 proves others work). So "Hermes compat" means: a C ABI
  crate over the embedding API, a C++ `jsi::Runtime` shim over that, an RN
  integration package, and running Metro's plain-JS bundles (not Hermes
  bytecode). Plus whatever Hermes-isms RN code expects (e.g. a
  `HermesInternal` stub), found by running real apps.

## Standing decisions (defaults; revisit any time)

- **From scratch:** we write everything JS-semantic (parser, VM, regex,
  builtins). Small math/data crates are allowed in engine crates only with
  architect sign-off (bigint arithmetic, Unicode tables, shortest float
  formatting). Harness code may use whatever it likes.
- **Engine first**, thin runtime in M4. No Node compat goal.
- **Model access:** the orchestrator shells out to `claude -p`, so it works
  with either an API key or a subscription. API key recommended for the swarm.
