# Landing through stacked PRs

The swarm no longer advances an integration branch after a local full
test262 run. Accepted work goes into a stack of pull requests on GitHub, the
full suite (and GC stress) runs as sharded PR CI, and Samuel merges the stack
from the bottom whenever he likes. `m1-spine`, `land.sh` and the local merge
bot's full run retire.

## Flow

```
worker ─▶ gates (scope · lint · clippy · target test262 before/after) ─▶ blind verifier
                                                        │ accept
                                                        ▼
                         stacker: cherry-pick onto the stack top
                                                        │
                    layer = branch stack/NNNN + PR (base: the layer below)
                                                        │
                       sharded PR CI: full test262 (+ GC stress) vs the layer's baseline
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
layer (no swarm stop). A layer that touches the GC, the heap or object
internals gets the `gc-stress` label, which makes CI add the stress shards.
The 0-regression and GC-stress rules are unchanged; CI enforces them.

## What CI provides

Owned by the CI workflow (`.github/workflows/`):

1. Runs on `pull_request` for every base branch (stacked PRs' bases are other
   layers, not `main`), and on pushes to `main`.
2. Measures regressions against the PR head's own
   `conformance/expected-pass.txt`.
3. One aggregate check run named `test262`, plus an artifact `t262-results`
   holding the merged results of all shards: per-run JSONL (`id`, `status`,
   `reason`, `not_implemented`) and the passing ids in `expected-pass.txt`
   format, and a summary with the regressions and whether the run was valid
   (canaries, no runner loss).
4. GC-stress shards when the PR has the `gc-stress` label.

## Local work

No local full-suite runs remain, so all cores go to worker slots. The
stacker's state (layers, PR numbers, pending CI) lives in `work/state.db`
and is snapshotted to `orch-runs` like everything else.

## Running it

`work/orch.local.toml`:

```toml
branch = "stack"                 # local only: the stack top, which workers start from
[stack]
repo = "mozzius/kahvi"
trunk = "main"                   # or the branch of an open PR the stack sits on
# trunk_pr = 3                   # that PR, put at the bottom of the GitHub stack
check = "test262"                # the aggregate check whose artifact decides
required_checks = ["test262", "fmt, clippy, test"]
artifact = "t262-results"        # results.jsonl, summary.json, expected-pass.txt
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
