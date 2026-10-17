"""The merge bot: replays an accepted attempt onto the tip of the integration
branch, runs the full suite, and advances the branch only if nothing that
passed before fails now. The baseline (conformance/expected-pass.txt) is
rewritten in the same commit, so the pass count can only ratchet up."""

from pathlib import Path

from common import ROOT, WORK, git, log, run, write_json
from gates import cargo, read_results, run_t262

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
    run(["git", "-c", "user.name=kahvi-merge-bot", "-c", "user.email=merge-bot@kahvi.invalid",
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
    """Merges several accepted attempts with one full test262 run.

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
    build = cargo(["build", "-q", "-p", "kahvi-cli"], MERGE_DIR, target_dir)
    if build.returncode != 0:
        return split({"merged": False, "reason": "does not build on top of the current branch:\n" + build.stderr[-2000:]})
    engine = target_dir / "debug" / "kahvi"
    baseline = MERGE_DIR / "conformance" / "expected-pass.txt"
    jobs = cfg.t262.get("jobs") or 0
    extra = ["--timeout", str(cfg.t262.get("timeout", 10))] + (["--jobs", str(jobs)] if jobs else [])

    adir = Path(picked[0]["adir"])
    # The full run rewrites the baseline, dropping whatever regressed, so the
    # flake re-run must compare against the baseline from before it.
    before = adir / "baseline-before.txt"
    before.write_text(baseline.read_text())
    log(f"full test262 run for merge: {names}")
    _, s = run_t262([], engine, adir, "full", baseline=baseline, write_baseline=baseline, extra=extra)
    if s is None or not s["canaries_ok"]:
        git("checkout", "--", "conformance", cwd=MERGE_DIR, check=False)
        for it in picked:
            results[it["attempt_id"]] = {"merged": False, "reason": "full run invalid (canaries or runner failure)"}
        return results
    regressions = s["regressions"]
    if regressions:
        # One re-run of just the regressed files to rule out flakes.
        files = sorted({r.rsplit(":", 1)[0] for r in regressions})
        _, s2 = run_t262(files, engine, adir, "recheck", baseline=before, write_baseline=baseline, extra=extra)
        still = regressions if s2 is None else s2["regressions"]
        if still:
            git("checkout", "--", "conformance", cwd=MERGE_DIR, check=False)
            return split({"merged": False, "reason": f"{len(still)} regressions in the full suite",
                          "regressions": still[:50]})

    # Passed. The per-attempt commits are kept; the new baseline is committed
    # on top, with the new passes attributed to the tickets whose target they
    # fall in.
    new = s["new_passes"]
    counts = {it["attempt_id"]: (len(new) if len(picked) == 1 else
                                 sum(1 for r in new if _in_target(it["ticket"], r))) for it in picked}
    git("add", "-A", cwd=MERGE_DIR)
    lines = [f"- {it['ticket'].id} (attempt {it['attempt_id']}, verdict {it['verdict']}): "
             f"+{counts[it['attempt_id']]} in its target" for it in picked]
    _commit(f"test262 baseline after {names} (+{len(new)})\n\n" + "\n".join(lines) +
            f"\n\ntest262: {s['pass']}/{s['runs']} runs passing.")
    head = git("rev-parse", "HEAD", cwd=MERGE_DIR)
    advance_branch(cfg, tip, head)
    (WORK / "last-full.jsonl").write_text((adir / "full.jsonl").read_text())
    write_json(WORK / "last-full-summary.json", s)

    full = read_results(adir / "full.jsonl")
    for it in picked:
        remaining = [f"{r['id']}  {r['reason'][:140]}" for r in full
                     if _in_target(it["ticket"], r["id"]) and r["status"] != "pass"]
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
