# Landing through stacked PRs

This is how barre lands work on `bluesky-social/barre` (from kahvi). Without
`[stack] repo` the orchestrator falls back to the local merge bot, which
advances a local branch after a full `./conf` run (docs/HARNESS.md).

In this mode the swarm does not advance an integration branch after a local
full conformance run. Accepted work goes into a stack of pull requests on
GitHub, the full suite runs as PR CI, and Samuel merges the stack from the
bottom whenever he likes.

## Flow

```
worker ─▶ gates (scope · lint · clippy · target ./conf before/after) ─▶ blind verifier
                                                        │ accept
                                                        ▼
                         stacker: cherry-pick onto the stack top
                                                        │
                    layer = branch stack/NNNN + PR (base: the layer below)
                                                        │
                       PR CI: full ./conf (+ stress shards) vs the layer's baseline
              green ─▶ tickets done/partial; layer waits for Samuel's merge
              red   ─▶ split the layer / drop the bad commit, restack, ticket back to the pool
```

- **Layers.** A layer is a branch `stack/NNNN` and a PR whose base is the
  layer below (the bottom one's base is `main`), linked with GitHub's stacks
  REST API (`/repos/{o}/{r}/stacks`, as `gh stack` uses). Each accepted
  attempt is one commit `TICKET: title`.
- **Adaptive batching.** While the top layer's CI is still running, newly
  accepted attempts gather in an open layer (up to `stack.max_layer`, default
  8). When CI on the top finishes (or the open layer is full), the open layer
  is pushed and its PR opened. Fast CI gives one ticket per layer; slow CI
  gives fewer, bigger layers, so the number of CI runs follows CI capacity,
  not swarm throughput.
- **Baseline.** Every layer's commit sets `conformance/expected-pass.txt` to
  the pass set from the CI of its nearest green ancestor layer (main's file
  for the first). CI fails the layer on any test in that file that does not
  pass, so the ratchet holds across the whole stack, not just against main.
- **Optimistic base.** Workers start from the stack top, including layers
  whose CI is still running, so they see each other's work. If a layer is
  later dropped, attempts based on it are replayed onto the new top like any
  other cherry-pick; a conflict sends the ticket back with its diff as
  salvage (as the merge bot does now).

## CI outcomes

The stacker polls each pending layer's check runs (REST, every minute) and
reads the results artifact.

| CI says | Stacker does |
|---|---|
| green | Ticket `done` when its target passes in the CI results, else the partial/open logic of today. Records the pass set as the baseline for layers above. |
| regressions, several tickets | Splits the layer in halves and pushes them as two layers (bisect, as the merge bot does with batches). |
| regressions, one ticket | Drops the commit, restacks the layers above onto the one below (bot-owned branches, `--force-with-lease`), reopens the ticket with the regressions in its notes. |
| does not build | Same as regressions. |
| infra failure (no test ran) | Pushes the layer again once; a second failure is reported in `orch.log` and the stack waits. |

A ticket in a layer whose CI is pending is `stacked`. For `needs`, `stacked`
counts as done (optimistic, like the base).

## Merging

Samuel merges from the bottom at any time (GitHub UI or `gh stack merge`).
A merge commit keeps the layers above valid. After a squash, the stacker's
sync notices the merged layer and rebases the rest onto `main`. The stacker
never merges anything.

## Architect work

`./orch land BRANCH` puts an architect branch on top of the stack as its own
layer (no swarm stop). A layer that touches a path matching `[stack]
stress_paths` gets the `stress_label` label, which a workflow can use to add
extra shards (kahvi used this for GC stress; barre could use it for
differential fuzzing against Yoga; nothing reads it yet). The 0-regression
rule is unchanged; CI enforces it.

This is the only way anything but a GitHub merge reaches `main`: architect
agents (perf work, the RN shim, docs) commit on their own branch, then

```
git merge stack                     # in the branch's worktree, only if it conflicts with the stack top
./orch land my-branch               # from the main checkout; needs a running `./orch run`
```

The branch becomes the next layer (alone, never batched with tickets), gets
the same CI, and Samuel merges it with the rest. If its CI is red the
stacker drops it and logs `architect branch ... is red in CI`; fix the
branch and land it again. With no orchestrator running, `./orch land` only
queues it; the next `./orch run` stacks it and stays up until its CI is
judged. Never commit to `main` or `stack` directly: `stack` is rebuilt by
the stacker (`git reset --keep`), and `main` only moves by merging PRs (and
by CI's README commits).

## What CI provides

`.github/workflows/ci.yml`:

1. Runs on `pull_request` for every base branch (stacked PRs' bases are other
   layers, not `main`), and on pushes to `main`. It tests the PR head itself,
   not GitHub's merge with the base.
2. `fmt, clippy, test` (ubuntu): `cargo fmt --check`, the gate's clippy,
   `cargo test`. A failure here makes the layer red without waiting for the
   suite.
3. `conformance` (macOS arm64, a few minutes, no shards: the suite takes
   seconds): the full `./conf` against the PR head's own
   `conformance/expected-pass.txt`, with `--update-expected` into the
   artifact's copy. Artifact `conf-results`
   (`.github/scripts/conf_results.py`): `results.jsonl` (`id`, `status`,
   `reason`, `not_implemented`, as `harness/orch/conformance.py` normalises
   `./conf --json`), `summary.json` (`./conf --summary-json` plus `runs`,
   `build_failed`: `valid`, `regressions`, `new_passes` decide) and
   `expected-pass.txt` (this run's passes, the baseline for the layer above).
   A compile error in barre is a valid, failed run (red); `./conf` failing
   before it ran anything leaves no summary (CI's problem: re-run once, then
   `stuck`).
4. `runner self-check (real Yoga)` (macOS): `./conf --yoga`, which must pass
   everything in scope with the documented Yoga bugs as `xfail`. Not in
   `required_checks`: workers cannot change the runner, the suite or the pin.
5. On pushes to `main` only, `update README results` rewrites the README's
   results rows from both artifacts and commits `[skip ci]` with
   `GITHUB_TOKEN` (which triggers no further runs). `bench.yml` does the same
   for the benchmark table and keeps history on the `bench-results` branch.
   Pull requests, including ones from forks, only ever get a read-only token.

## Local work

No local full-suite runs remain, so all cores go to worker slots. The
stacker's state (layers, PR numbers, pending CI) lives in `work/state.db`
and is snapshotted to `orch-runs` like everything else.

## Running it

`work/orch.local.toml` on the machine that runs the swarm (the rest of
`[stack]` is in `harness/orch.toml`):

```toml
branch = "stack"                 # local only: the stack top, which workers start from
[stack]
repo = "bluesky-social/barre"
# trunk = "some-branch"          # default main; or the branch of an open PR the stack sits on
# trunk_pr = 3                   # that PR, put at the bottom of the GitHub stack
```

`git branch stack origin/main` once before the first run. The main checkout
should stay on `branch`, so the trusted runner (`./conf`, `skip.txt`) the
gates use is the stack top's; the stacker moves it (`reset --keep`) as the
stack changes. If the checkout is on another branch the stacker only moves
the `stack` ref, and the gates run whatever that checkout has. `./orch stack` shows the layers, `./orch stack retry` re-stacks a
layer stuck on broken CI once CI is fixed, `./orch land BRANCH` queues an
architect branch (merge the stack top into it first if it conflicts).

The stacker uses `gh api` (REST only) for PRs, labels, comments, the stacks
API, check runs, artifact downloads and workflow re-runs, so the machine
needs `gh` logged in with write access. It never deletes branches: closed
layers' branches stay until someone deletes them ("Automatically delete
head branches" is on for merged ones). A PR in a GitHub stack cannot be
retargeted by hand; GitHub retargets it when the PR below merges.
