"""The merge bot: replays an accepted attempt onto the tip of the integration
branch, runs the full conformance suite, and advances the branch only if
nothing that passed before fails now. The baseline
(conformance/expected-pass.txt) is rewritten in the same commit, so the pass
count can only ratchet up."""

from pathlib import Path

from common import WORK, git, log, run, write_json
from conformance import BASELINE, read_ids, run_conf, write_baseline
from gates import build, read_results

MERGE_DIR = WORK / "merge"


def prepare(tip):
    if not (MERGE_DIR / ".git").exists():
        git("worktree", "add", "--detach", str(MERGE_DIR), tip)
    git("cherry-pick", "--abort", cwd=MERGE_DIR, check=False)
    git("checkout", "--detach", "--force", tip, cwd=MERGE_DIR)
    git("clean", "-fdx", "-e", "target", cwd=MERGE_DIR)


def advance_branch(cfg, old, new):
    current = git("rev-parse", "--abbrev-ref", "HEAD")
    if current == cfg.branch:
        # Keep the main checkout's working tree in sync with the branch.
        git("merge", "--ff-only", "-q", new)
    else:
        git("update-ref", f"refs/heads/{cfg.branch}", new, old)
    if cfg.push:
        for delay in (0, 2, 4, 8, 16):
            if delay:
                import time
                time.sleep(delay)
            if run(["git", "push", cfg.remote, f"{new}:refs/heads/{cfg.branch}"], check=False).returncode == 0:
                break
        else:
            log(f"push of {cfg.branch} failed; continuing locally")


def _commit(message, cwd=MERGE_DIR):
    run(["git", "-c", "user.name=barre-merge-bot", "-c", "user.email=merge-bot@barre.invalid",
         "commit", "-q", "--no-verify", "--allow-empty", "-m", message], cwd=cwd)


def _pick_all(tip, items):
    """Cherry-picks each item onto tip, one commit each. Returns (picked, refused)."""
    prepare(tip)
    picked, refused = [], {}
    for it in items:
        pick = run(["git", "cherry-pick", "--no-commit", it["head"]], cwd=MERGE_DIR, check=False)
        if pick.returncode != 0:
            git("cherry-pick", "--abort", cwd=MERGE_DIR, check=False)
            git("reset", "--hard", "-q", "HEAD", cwd=MERGE_DIR, check=False)
            refused[it["attempt_id"]] = {"merged": False, "reason": "conflicts with newer work on the branch"}
            continue
        git("add", "-A", cwd=MERGE_DIR)
        _commit(f"{it['ticket'].id}: {it['ticket'].title}")
        picked.append(it)
    return picked, refused


def _in_target(ticket, test_id):
    return any(test_id.startswith(t) for t in ticket.target)


def merge(cfg, ticket, attempt_id, head, verdict, adir):
    """One attempt; see merge_batch."""
    item = {"ticket": ticket, "attempt_id": attempt_id, "head": head, "verdict": verdict, "adir": adir}
    return merge_batch(cfg, [item])[attempt_id]


def merge_batch(cfg, items):
    """Merges several accepted attempts with one full conformance run.

    Each item is a dict with ticket, attempt_id, head, verdict and adir. The
    attempts are cherry-picked in order onto the branch tip; one that
    conflicts is refused on its own. The rest are built and run together
    against the baseline. On a build failure or a regression the batch is
    split in half and each half is tried on its own, down to single attempts,
    so one bad attempt cannot hold back the others. Returns a dict from
    attempt id to the result (merged, reason, new_passes, target_complete,
    remaining)."""
    tip = git("rev-parse", cfg.branch)
    picked, results = _pick_all(tip, items)
    if not picked:
        return results

    def split(reason):
        if len(picked) == 1:
            results[picked[0]["attempt_id"]] = reason
            return results
        half = len(picked) // 2
        log(f"merge batch of {len(picked)} failed ({reason['reason'][:80]}); splitting")
        results.update(merge_batch(cfg, picked[:half]))
        results.update(merge_batch(cfg, picked[half:]))
        return results

    names = ", ".join(it["ticket"].id for it in picked)
    target_dir = WORK / "merge-target"
    b = build(cfg, MERGE_DIR, target_dir)
    if b.returncode != 0:
        return split({"merged": False, "reason": "does not build on top of the current branch:\n" + b.stderr[-2000:]})
    baseline = MERGE_DIR / BASELINE
    old = read_ids(baseline)

    adir = Path(picked[0]["adir"])
    log(f"full conformance run for merge: {names}")
    _, s = run_conf([], MERGE_DIR, adir, "full", target_dir=target_dir, baseline=baseline, cfg=cfg.conf)
    if s is None or not s["canaries_ok"]:
        for it in picked:
            results[it["attempt_id"]] = {"merged": False,
                                         "reason": "full run invalid (canaries or runner failure)"}
        return results
    regressions = s["regressions"]
    if regressions:
        # One re-run of just the regressed tests to rule out flakes.
        _, s2 = run_conf(regressions, MERGE_DIR, adir, "recheck", target_dir=target_dir, baseline=baseline,
                         cfg=cfg.conf)
        still = regressions if s2 is None or not s2["canaries_ok"] else \
            [r for r in s2["regressions"] if r in set(regressions)]
        if still:
            return split({"merged": False, "reason": f"{len(still)} regressions in the full suite",
                          "regressions": still[:50]})

    # The new baseline: everything passing now. A flaky regression that passed
    # on the re-run stays in; a test that vanished (skipped now) drops out.
    full = read_results(adir / "full.jsonl")
    passing = {r["id"] for r in full if r["status"] == "pass"} | (old & set(regressions))
    write_baseline(baseline, passing)
    s["pass"] = len(passing)

    # Passed. The per-attempt commits are kept; the new baseline is committed
    # on top, with the new passes attributed to the tickets whose target they
    # fall in.
    new = s["new_passes"]
    counts = {it["attempt_id"]: (len(new) if len(picked) == 1 else
                                 sum(1 for r in new if _in_target(it["ticket"], r))) for it in picked}
    git("add", "--", BASELINE, cwd=MERGE_DIR)
    lines = [f"- {it['ticket'].id} (attempt {it['attempt_id']}, verdict {it['verdict']}): "
             f"+{counts[it['attempt_id']]} in its target" for it in picked]
    _commit(f"conformance baseline after {names} (+{len(new)})\n\n" + "\n".join(lines) +
            f"\n\nconformance: {s['pass']}/{s['runs']} tests passing.")
    head = git("rev-parse", "HEAD", cwd=MERGE_DIR)
    advance_branch(cfg, tip, head)
    (WORK / "last-full.jsonl").write_text((adir / "full.jsonl").read_text())
    write_json(WORK / "last-full-summary.json", s)

    for it in picked:
        remaining = [f"{r['id']}  {r['reason'][:140]}" for r in full
                     if _in_target(it["ticket"], r["id"]) and r["status"] not in ("pass", "skip")]
        results[it["attempt_id"]] = {
            "merged": True,
            "commit": head,
            "batch": [i["ticket"].id for i in picked],
            "new_passes": counts[it["attempt_id"]],
            "total_pass": s["pass"],
            "target_complete": not remaining,
            "remaining": remaining,
        }
    return results
