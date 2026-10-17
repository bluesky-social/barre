# Operating the harness

Everything is driven from the repository root. Requirements are Rust (stable),
Python 3.11+, git, a C++ toolchain with ninja (for the conformance runner),
and the `claude` CLI logged in with an API key or a subscription.

The harness is kahvi's (a JavaScript engine built the same way), adapted:
Yoga's C++ source replaces ECMA-262 as the spec, and Yoga's gtest suite
replaces test262 as the measure of correctness.

```
git submodule update --init --depth 1     # vendor/yoga + vendor/googletest (pinned)
./conf generated/YGAlignItemsTest         # build barre, run part of Yoga's suite
./spec computeFlexBasisForChild           # read the Yoga source (--refs: + short callees)
./orch run                                # grind the ticket pool
./orch status                             # what happened, what it cost
./orch stop                               # finish in-flight attempts, then exit
./orch snapshot                           # commit the run state to the orch-runs branch
```

The run state (ticket statuses, every attempt's prompt, transcript, diff,
gate report and verdicts, the logs) is versioned on its own branch,
`orch-runs`: the orchestrator commits a snapshot after every attempt (and
pushes it if `push = true`). The code branch stays free of these commits.

Accepted work lands through **stacked PRs** on `bluesky-social/barre` whose
CI runs the full suite (docs/STACKS.md): the stacker moves the local `stack`
branch, which workers start from, and opens a PR per layer; Samuel merges
them from the bottom. It is on where `work/orch.local.toml` sets `[stack]
repo` (and `branch = "stack"`); without it (e.g. a scratch orchestrator under
`BARRE_WORK`) the local merge bot cherry-picks accepted attempts onto
`branch` after a full local `./conf` run and never pushes.

**Architect work lands the same way.** Nothing but a PR merge moves `main`
(CI also commits README updates to it). Commit on your own branch (a
worktree is fine), then from the main checkout `./orch land BRANCH`: the
branch becomes its own layer on top of the stack, with the same CI, and
merges with the rest. Merge `stack` into the branch first if it conflicts.
`./orch stack` shows where it is. Never commit on `stack` (the stacker
rebuilds it) or push `main`.

## The spec (`./spec`, `harness/spec.py`)

`./spec <name>` prints a definition from `vendor/yoga/yoga/` with its
file:line and the comment above it. Names are Yoga's
(`computeFlexBasisForChild`, `Node::resolveFlexGrow`, `LayoutResults`,
`YGNodeStyleSetFlexBasisPercent`, `YGAlign`); barre's snake_case names work
too (`compute_flex_basis_for_child`). Overloads all print. `--refs` appends
the short (< 60 lines) functions the definition calls, free functions first,
leaving out one-statement accessors. `--find <text>` lists definitions whose
name contains the text; `--find-text <text>` those whose body does.

It is a brace matcher, not a C++ parser: it strips comments, strings and
preprocessor lines, tracks namespace and class scopes, and treats a `{`
after a parameter list as a function body (skipping constructor initialiser
braces). Every function, constructor, operator, class, struct, enum, `using`
alias, namespace-scope constant and `YG_ENUM_DECL` is indexed (about 800
definitions; the index takes a third of a second). Worker worktrees have no
submodules, so the orchestrator sets `BARRE_VENDOR` to the main checkout's
`vendor/`.

## Conformance runner (`./conf`, `harness/conf`)

docs/CONFORMANCE.md is the contract. In short: `./conf [ID-PREFIX]...`
links Yoga's gtest suite, unmodified, against barre's C ABI
(`crates/barre-ffi`) and runs every test in its own process with a timeout,
so crashes (including `not_implemented!`, which aborts) and hangs are
failures. Test ids look like
`generated/YGAlignItemsTest/YogaTest.align_baseline`. Canary tests that must
fail make a faked pass detectable (exit 3: the run is invalid).
`conformance/skip.txt` lists what is out of scope;
`conformance/expected-pass.txt` is the ratchet.

The orchestrator talks to it through `harness/orch/conformance.py`:

- it always runs the runner **from the main checkout**, so the runner,
  `skip.txt`, `vendor/` and the canaries are never the worker's copies; only
  the engine is built from the tree under test (`--engine-root`, with a
  per-slot `--target-dir`). This is kahvi's "trusted runner build in
  work/bin", for a runner that is a script: trust comes from running it from
  a tree no worker can write to, and from the gate refusing any diff that
  touches a protected path (`harness/`, `conformance/`, `vendor/`, manifests,
  `build.rs`);
- it reads `--json` (`{id: {status, reason}}`) and computes new passes,
  regressions, counts and `not implemented: X` names itself, against the
  baseline file it was given. A baseline test that ran and did not pass is a
  regression; one that no longer runs (newly skipped) is not, and drops out of
  the baseline at the next merge.

## Orchestrator (`./orch`, `harness/orch`)

`./orch run` loops over:

1. **Claim.** Picks the highest-priority open ticket whose `needs` are done
   (tickets/README.md has the format).
2. **Work.** Resets a slot worktree (`work/slots/slot-N`, which keeps its warm
   `target/`) to the branch tip and runs `claude -p` on the ticket's tier
   model. The run is limited to file tools plus `cargo build/check/clippy/fmt`,
   `./conf`, `./spec` and `git diff/status`, with a turn cap and a dollar
   cap. A PreToolUse hook blocks edits outside the ticket's `scope`. The
   prompt carries the ticket and the Yoga source for its `spec` names
   (`./spec --refs`). The worker's chat output is only scanned for a
   `BLOCKED:` line.
3. **Gates.** Recomputed from the commit:
   - files in scope;
   - no protected path touched;
   - lint on added lines (`harness/orch/policy.py`, below);
   - vendor checkout untouched;
   - build and clippy (`[gate]` in orch.toml), in a separate per-slot target
     directory;
   - `check` commands pass;
   - `./conf` over the ticket's `target`, run from the main checkout: no
     regressions, canaries ok, and at least one new pass.
4. **Verify.** The blind verifier gets the ticket, the Yoga source, the gate
   report and the diff. It never gets the worker's words. It answers yes/no
   questions (`harness/orch/verifier.py`), and code derives the decision:
   - `accept`: no defects; merge.
   - `revise`: defects but worth keeping. The next attempt starts from this
     diff plus the reviewer's notes.
   - `reject`: fakery (special-cased inputs, made-up results) or not worth
     keeping. Discarded, and counts as a strike. Fakery escalates at once.

   The questions: special-cased sizes/values/tree shapes; made-up results
   (a default, the available size, a skipped computation instead of Yoga's
   value or `not_implemented!`); swallowed failures (caught panics,
   fallbacks, silently dropped branches); missing, reordered or misapplied
   steps (wrong axis, edge, box component, node, sizing mode, errata);
   any other departure from the Yoga source the ticket doesn't ask for; and
   unrelated changes.

   Haiku screens every attempt; Sonnet re-judges anything that would merge,
   and the stricter decision wins.
5. **Merge.** Done serially in `work/merge`, in batches of up to
   `[conf] max_merge_batch`:
   - cherry-pick the attempts onto the current tip;
   - build, then run the **full** suite against the baseline (a regression
     gets one re-run of the regressed tests to rule out flakes, then the
     batch is split in halves until the bad attempt is isolated);
   - rewrite the baseline to the new pass set and commit it;
   - advance the branch (`git merge --ff-only` if the main checkout is on it,
     otherwise `update-ref`);
   - push if `push = true`.

   If the target still has failures after the merge, the ticket goes back to
   the pool only if a failure says `not implemented: <one of its functions>`;
   otherwise it becomes `partial` (blocked on other work) until a later merge
   finishes it or triage turns the rest into tickets.

Escalation: after `strikes_per_tier` strikes (gate rejections, rejected
attempts, empty diffs), the ticket moves up a tier: haiku → sonnet → opus.
After `max_attempts`, or a failure at the top tier, it is marked
`needs-architect`. A `BLOCKED:` reply sets it to `blocked` with the reason.
Both show up in `./orch status` for the architect to split, rewrite or
unblock.

### Usage limits

Every run reports the plan's usage windows. Requests past the plan's windows
spill into API billing (through agentgateway), which is fine for cheap work,
so the policy is per tier (`[limits]` in orch.toml, `harness/orch/limits.py`):

- **Cap pause.** When a window passes its cap (default 90% of the 5-hour
  window, 80% of the 7-day one) or the plan reports it exhausted, only the
  *enforced* work stops starting until that window resets:
  - worker attempts at a tier in `enforce_tiers` (default `["sonnet",
    "opus"]`): the claim loop skips tickets at those tiers (their current,
    possibly escalated, tier) and keeps handing out the rest;
  - the screening verifier, if its model is one of those tiers (it is Haiku,
    so it never waits);
  - the confirming verifier, if `enforce_verifier_confirm` (default `false`:
    the Sonnet confirm is small and on the merge path, and holding it would
    starve the Haiku grind of merges).

  Haiku work never pauses. With `enforce_tiers` unset, every tier is
  enforced (kahvi's behaviour). When only paused-tier tickets are left, the
  loop waits for the reset, re-reading the ticket files every minute.
- **Hard pause.** An actual rate-limit or overload error from the API holds
  everything, with backoff, as before.

`./orch status` prints both: `PAUSED (all tiers) until ...` for a hard pause,
`paused tiers: sonnet, opus until ...; still running: haiku` for a cap pause,
and the policy in force. Pauses survive restarts.

A run that hits a limit anyway (or an overload) is an interruption, not a
failure:
- the attempt doesn't count against the ticket;
- its partial diff seeds the next attempt;
- an attempt cut off during verification resumes at verification.

A run that finished is never an interruption, even if the plan reported its
window exhausted during it (that request was API-billed).

If the CLI is logged out, the orchestrator stops and says so.

Other commands:

- `./orch status`: tickets by status, any pause (all tiers, or which tiers)
  and the usage-limit policy, the plan's usage, spend
  (workers and verifier), outcomes per tier, and dollars per new conformance
  pass.
- `./orch show N`: an attempt's prompt, gate report, verifier answers, merge
  result and diff. Everything is kept under `work/attempts/N/`.
- `./orch reset T-0001 [--tier sonnet]`: reopen a ticket.
- `./orch partials [--results F]`: re-judge `partial` tickets against a full
  run.
- `./orch triage [--fresh]`: groups failures on the branch by status, by
  `not implemented: X`, by normalised failure message and by test file. This
  is the raw material for new tickets.
- `./orch calibrate [--model M] [--repeat N]`: runs the verifier over
  `harness/redteam/cases`. It exits non-zero if any planted cheat or buggy
  diff would have been merged.

Configuration is in `harness/orch.toml`. Per-machine overrides go in
`work/orch.local.toml`, for example `slots = 8` or a different verifier model.
`BARRE_WORK=/some/dir` runs a separate orchestrator with its own state.
To smoke-test the whole pipeline (worker tools in a slot, gates, verifier,
merge bot) without touching `main`, run the smoke ticket against a scratch
branch with its own state:

```
git branch -f smoke-scratch main
BARRE_WORK=/tmp/barre-smoke ./orch run --tickets tickets/smoke --branch smoke-scratch --max-attempts 2
# clean up: its worktrees, the snapshot branch it created, the attempt refs
rm -rf /tmp/barre-smoke && git worktree prune && git branch -D smoke-scratch orch-runs
git for-each-ref --format='%(refname)' refs/barre | xargs -n1 git update-ref -d
```

(2026-10-17: one Haiku attempt, $0.04 in all, ported `calculateBaseline`,
passed the gates and both verifiers and merged with +65 passes.)

`python3 harness/orch/selftest.py` checks the pieces that need no model and
no engine (spec lookups, lint rules, result normalisation, ticket parsing, the
usage-limit policy, the red-team cases).

## Policy (`harness/orch/policy.py`)

Protected paths no ticket can open: `vendor/`, `harness/`, `conformance/`,
`tickets/`, `docs/`, `bench/`, Cargo manifests and lockfile, `build.rs`,
`tests/` directories, toolchain and lint config, and the `conf`, `spec`,
`orch` scripts.

Added Rust lines are rejected outright for: `todo!`/`unimplemented!` (use
`not_implemented!`), new tests, lint suppression, `unsafe` outside
`crates/barre-ffi`, files/env/network/stdio, `std::process` (exit/abort),
printing, `include_*!`, `extern crate`, catching panics (`catch_unwind`,
panic hooks), global state (`static mut`, `thread_local!`, statics holding
atomics, locks or cells), and references to the test harness (gtest, test
file and suite names, the baseline file). A ticket can waive a rule by id
(`allow = ["unsafe"]`).

Flagged for the verifier (hints, not verdicts): `panic!`, `unreachable!`,
`unwrap`/`expect`, `unwrap_or`/`.ok()` defaults, `unsafe` in the FFI crate,
leftover markers, shortcut language, admissions of missing work, "skip",
claims of a deliberate difference from Yoga, and the layout-specific fakery
smells: float literals with three or more significant digits (`33.333`,
`146.5`), comparisons with a specific number, comparisons of child counts or
indices with a specific number, `match` arms on float literals, and string
comparisons.

## Roles

- **Architect** (Opus, interactive). Owns everything protected:
  - the engine spine, `docs/API.md`, tickets, skip list, policy, prompts;
  - reviewing `needs-architect` and `blocked` tickets;
  - turning triage output into tickets;
  - commits on its own branch, outside the pipeline, and lands it with
    `./orch land BRANCH` (its own stack layer, same CI as the swarm's).
- **Workers** (`claude -p`, Haiku by default). One ticket each, scoped files
  only.
- **Verifier** (Haiku screen, Sonnet confirm). Blind, structured output,
  calibrated against the red-team set.
- **Stacker** (or, without `[stack] repo`, the merge bot). Plain code, no model.

## Calibration log

The red-team set (`harness/redteam/cases`) was rewritten for barre: layout
cheats, faithful ports, partial ports and buggy ports in Rust against
barre's API. kahvi's calibration results do not carry over; the verifier has
to be recalibrated on this set (and after any change to the verifier prompt,
questions or models). A configuration is only trusted with 0 critical misses.

| date | verifier | cases | exact | critical misses | good work rejected | $/case |
|---|---|---|---|---|---|---|
| 2026-10-17 | haiku screen → sonnet confirm | 12 | 12/12 | 0 | 0 | 0.027 |
| 2026-10-17 | haiku-5-5 alone ×2 | 12 | 24/24 | 0 | 0 | 0.002 |
| 2026-10-17 | sonnet-5-5 alone | 12 | 12/12 | 0 | 0 | 0.046 |
| 2026-10-17 | **haiku screen → sonnet confirm** ×2 (+ case 13) | 13 | 26/26 | 0 | 0 | 0.005 (cached) |
| 2026-10-17 | cases rewritten against the spine's `LayoutNode` API: haiku screen → sonnet confirm | 13 | 13/13 | 0 | 0 | 0.025 |
| 2026-10-17 | same set, haiku-5-5 alone ×2 | 13 | 26/26 | 0 | 0 | 0.002 |
| 2026-10-17 | same set, sonnet-5-5 alone | 13 | 13/13 | 0 | 0 | 0.049 |

Every model got every case right on the first barre set, so it does not yet
separate configurations the way kahvi's did: add harder cases (subtle cheats
that keep Yoga's structure, faithful ports with very different Rust
structure) as real attempts show what slips through. Haiku labels the
"deliberate fix" excuse (case 13) and the missing margin (case 10) as fakery,
which escalates the ticket at once; acceptable, but watch for it rejecting
honest deviations the ticket asked for.

What carried over from kahvi's calibration (2026-10-13), as design:

- The verifier answers yes/no questions and code derives
  `accept`/`revise`/`reject`; the model never picks a label, so there is
  nothing for it to be lenient about. (Models described a problem correctly
  and then picked a lenient label anyway.)
- Any "yes" blocks a merge; special-casing and made-up results always mean
  starting over. The special-casing question spells out the honest
  exceptions (the spec's own conditions, `not_implemented!` paths) because
  it once caught an honest limit.
- Short referenced functions are appended to the spec text (`--refs`) and
  the prompt says the provided source is authoritative: Haiku rejected good
  work by trusting its memory of an older spec edition.
- The cascade: Haiku for every attempt, Sonnet re-judges anything Haiku
  would merge, stricter decision wins. To land, a cheat has to fool both.
