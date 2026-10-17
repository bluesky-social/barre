# Operating the harness

Everything is driven from the repository root. Requirements are Rust (stable),
Python 3.11+, git, and the `claude` CLI logged in with an API key or a
subscription. Node is optional; it's only used to validate the runner.

```
git submodule update --init --depth 1     # vendor/test262 + vendor/ecma262 (pinned)
./t262 built-ins/Array/prototype/at/      # build the engine, run some test262
./spec sec-array.prototype.at             # read a spec clause (--refs: + called ops)
./orch run                                # grind the ticket pool
./orch status                             # what happened, what it cost
./orch stop                               # finish in-flight attempts, then exit
./orch snapshot                           # commit the run state to the orch-runs branch
```

The run state (ticket statuses, every attempt's prompt, transcript, diff,
gate report and verdicts, the logs) is versioned on its own branch,
`orch-runs`: the orchestrator commits and pushes a snapshot after every
attempt. The code branch stays free of these commits.

Accepted work lands through stacked PRs whose CI runs the full suite
(docs/STACKS.md). The stacker moves the local `stack` branch in the main
checkout, so architect work happens in a separate worktree and lands with
`./orch land BRANCH`, as its own layer. Without `[stack] repo` the old local
merge bot advances `branch` after a local full run instead.

## test262 runner (`./t262`, `harness/t262`)

`./t262 [PREFIX]... [--show N] [--by-dir] [--json F] [--baseline F]` runs
every test file under the given prefixes of `vendor/test262/test/`. It covers
strict, sloppy, module and raw modes, and runs in parallel with timeouts. The
engine contract is in `docs/ENGINE-CLI.md`.

The runner is built so that faking a pass is hard:

- each run gets a fresh temp directory and a random file name;
- positive tests get a random completion marker that the engine must print;
- the engine runs with an empty environment and a memory cap;
- crashes and timeouts count as failures;
- canary tests with random contents must fail. If any of them passes, the
  run is declared invalid and exits with code 3.

`conformance/skip.txt` lists the features and paths that are out of scope.
`conformance/expected-pass.txt` is the ratchet: every run that passes on the
integration branch. Each stack layer carries the pass set CI measured on the
layer below, and CI refuses a layer that loses any of it.

To check the runner itself against a mature engine:

```
target/debug/t262 run built-ins/Array/ --mem-limit-mb 0 \
  --engine "node --experimental-vm-modules --no-warnings harness/t262/node-adapter.mjs"
```

## Orchestrator (`./orch`, `harness/orch`)

`./orch run` loops over:

1. **Claim.** Picks the highest-priority open ticket whose `needs` are done.
2. **Work.** Resets a slot worktree (`work/slots/slot-N`, which keeps its warm
   `target/`) to the branch tip and runs `claude -p` on the ticket's tier
   model. The run is limited to file tools plus `cargo`, `./t262`, `./spec`
   and `git diff/status`, with a turn cap and a dollar cap. A PreToolUse hook
   blocks edits outside the ticket's `scope`. The worker's chat output is
   only scanned for a `BLOCKED:` line.
3. **Gates.** Recomputed from the commit:
   - files in scope;
   - no protected path touched;
   - lint on added lines (`harness/orch/policy.py`);
   - vendor checkout untouched;
   - build and clippy, in a separate target directory;
   - `check` commands pass;
   - test262 over the ticket's `target`, using the trusted runner build in
     `work/bin`: no regressions, canaries ok, and at least one new pass.
4. **Verify.** The blind verifier gets the ticket, the spec text, the gate
   report and the diff. It never gets the worker's words. It answers yes/no
   questions (`harness/orch/verifier.py`), and code derives the decision:
   - `accept`: no defects; merge.
   - `revise`: defects but worth keeping. The next attempt starts from this
     diff plus the reviewer's notes.
   - `reject`: fakery (special-cased inputs, made-up results) or not worth
     keeping. Discarded, and counts as a strike. Fakery escalates at once.

   Haiku screens every attempt; Sonnet re-judges anything that would merge,
   and the stricter decision wins.
5. **Merge.** Done serially in `work/merge`:
   - cherry-pick the attempt onto the current tip;
   - build, then run the **full** suite against the baseline (a regression
     gets one re-run to rule out flakes, then the merge is refused);
   - rewrite the baseline and commit;
   - advance the branch (`git merge --ff-only` if the main checkout is on it,
     otherwise `update-ref`);
   - push if `push = true`.

   If the target still has failures after the merge, the ticket goes back to
   the pool with the list of what still fails.

Escalation: after `strikes_per_tier` strikes (gate rejections, rejected
attempts, empty diffs), the ticket moves up a tier: haiku → sonnet → opus.
After `max_attempts`, or a failure at the top tier, it is marked
`needs-architect`. A `BLOCKED:` reply sets it to `blocked` with the reason.
Both show up in `./orch status` for the architect to split, rewrite or
unblock.

Usage limits: every run reports the plan's usage windows. When one passes
its cap in `[limits]` (default 90% of the 5-hour window, 80% of the 7-day
one), nothing new starts until that window resets. The pause is printed by
`./orch status` and survives restarts. A run that hits a limit anyway (or an
overload) is an interruption, not a failure:
- the attempt doesn't count against the ticket;
- its partial diff seeds the next attempt;
- an attempt cut off during verification resumes at verification.

If the CLI is logged out, the orchestrator stops and says so.

Other commands:

- `./orch status`: tickets by status, any pause and the plan's usage, spend
  (workers and verifier), outcomes per tier, and dollars per new test262 pass.
- `./orch show N`: an attempt's prompt, gate report, verifier answers, merge result
  and diff. Everything is kept under `work/attempts/N/`.
- `./orch reset T-0001 [--tier sonnet]`: reopen a ticket.
- `./orch triage [--fresh]`: groups failures on the branch by
  "not implemented: X", by normalised error message and by directory. This is
  the raw material for new tickets.
- `./orch calibrate [--model M] [--repeat N]`: runs the verifier over
  `harness/redteam/cases`. It exits non-zero if any planted cheat or buggy
  diff would have been merged.

Configuration is in `harness/orch.toml`. Per-machine overrides go in
`work/orch.local.toml`, for example `slots = 8` or a different verifier model.
`KAHVI_WORK=/some/dir` runs a separate orchestrator with its own state.

## Roles

- **Architect** (Opus, interactive). Owns everything protected:
  - the engine spine, `docs/API.md`, tickets, skip list, policy, prompts;
  - reviewing `needs-architect` and `blocked` tickets;
  - turning triage output into tickets;
  - commits directly to the branch, outside the pipeline.
- **Workers** (`claude -p`, Haiku by default). One ticket each, scoped files
  only.
- **Verifier** (Sonnet by default). Blind, structured output, calibrated
  against the red-team set.
- **Stacker** (or the local merge bot). Plain code, no model.

## Calibration log

| date | verifier | exact | critical misses | good work rejected | $/case |
|---|---|---|---|---|---|
| 2026-10-13 | sonnet-5-5, free verdict | 10/12 | 0 (1 on a rerun) | 0 | 0.031 |
| 2026-10-13 | haiku-5-5 ×2, free verdict | 20/24 | 0 | 2 | 0.003 |
| 2026-10-13 | sonnet-5-5 ×2, binding checklist | 24/24 | 0 | 0 | 0.027 |
| 2026-10-13 | haiku-5-5 ×2, binding checklist | 24/24 | 0 | 0 | 0.003 |
| 2026-10-13 | **haiku screen → sonnet confirm** ×2 | 24/24 | 0 | 0 | 0.011 |
| 2026-10-13 | **yes/no questions → derived accept/revise/reject**, haiku screen → sonnet confirm ×2 | 24/24 | 0 | 0 | 0.011 |

What changed along the way:

- Haiku rejected good work by trusting its memory of an older spec edition.
  Fix: short referenced operations are now appended to the spec text
  (`--refs`), and the prompt says the provided spec is authoritative.
- On one rerun, Sonnet labelled a "return the input unchanged" fake as
  `incomplete`, although its own summary described the problem. Fix: the
  verifier answers four yes/no checks, and code enforces them. Any "yes"
  blocks a merge; special-casing inputs is always garbage.
- The checklist's special-casing question once caught an honest
  implementation limit, so it now spells out that exception.

- The verdict labels were then replaced by yes/no questions, with the
  decision derived in code (`accept` / `revise` / `reject`). The model never
  picks a label, so there's nothing for it to be lenient about.

The configured cascade is Haiku for every attempt, with Sonnet re-judging
anything Haiku would merge and the stricter decision winning. To land, a cheat
has to fool both models.
