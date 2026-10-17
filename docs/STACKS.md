# Landing through stacked PRs

> **barre runs locally for now** (no `[stack] repo`; the local merge bot
> advances `main` after a full `./conf` run, docs/HARNESS.md). This mode is
> kahvi's, kept working with names changed, for when barre has CI. Turning it
> on needs the CI workflow below, which does not exist yet.

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
differential fuzzing against Yoga). The 0-regression rule is unchanged; CI
enforces it.

## What CI provides

Owned by the CI workflow (`.github/workflows/`):

1. Runs on `pull_request` for every base branch (stacked PRs' bases are other
   layers, not `main`), and on pushes to `main`.
2. Measures regressions against the PR head's own
   `conformance/expected-pass.txt`.
3. One aggregate check run named `conformance`, plus an artifact
   `conf-results` holding the merged results of all shards: per-test JSONL
   (`id`, `status`, `reason`, `not_implemented`, as `harness/orch/conformance.py`
   normalises `./conf --json`), the passing ids in `expected-pass.txt`
   format, and a summary with the regressions and whether the run was valid
   (canaries, no runner loss).
4. Extra shards when the PR has the `stress_label` label.

## Local work

No local full-suite runs remain, so all cores go to worker slots. The
stacker's state (layers, PR numbers, pending CI) lives in `work/state.db`
and is snapshotted to `orch-runs` like everything else.

## Running it

`work/orch.local.toml`:

```toml
branch = "stack"                 # local only: the stack top, which workers start from
[stack]
repo = "mozzius/barre"
trunk = "main"                   # or the branch of an open PR the stack sits on
# trunk_pr = 3                   # that PR, put at the bottom of the GitHub stack
check = "conformance"            # the aggregate check whose artifact decides
required_checks = ["conformance", "fmt, clippy, test"]
artifact = "conf-results"        # results.jsonl, summary.json, expected-pass.txt
max_layer = 8
```

The main checkout stays on `branch`; the stacker moves it as the stack
changes. `./orch stack` shows the layers, `./orch stack retry` re-stacks a
layer stuck on broken CI once CI is fixed, `./orch land BRANCH` queues an
architect branch (merge the stack top into it first if it conflicts).

From this container: PRs, labels, comments, the stacks API, check runs,
artifact downloads and workflow re-runs work over REST. Deleting branches
does not, so closed layers' branches stay until someone deletes them (turn on
"Automatically delete head branches" for merged ones). A PR in a GitHub stack
cannot be retargeted by hand; GitHub retargets it when the PR below merges.
