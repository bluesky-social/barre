"""Versions the orchestrator's run state on its own branch (`orch-runs`).

After every attempt the run loop calls `snapshot(cfg)`, which copies into a
dedicated worktree (work/runs-repo):

- state.db (a consistent SQLite backup) and a readable export of it
  (tickets.json, attempts.jsonl, notes.jsonl),
- the orchestrator logs and the verifier calibration results,
- every attempt's artifacts (prompt, transcript, diff, gate report, verdicts,
  test summaries), without the raw full-suite logs, which are large and can
  be regenerated,

then commits and pushes the branch. The code branch never sees these
commits, so they cannot race the merge bot.
"""

import json
import shutil
import sqlite3
import time

from common import WORK, git, log, run

BRANCH = "orch-runs"
REPO = WORK / "runs-repo"
# Attempt files not worth versioning (large and reproducible).
SKIP_ATTEMPT_FILES = {"full.jsonl", "recheck.jsonl"}


def _ensure_repo(cfg):
    if (REPO / ".git").exists():
        return
    exists = git("rev-parse", "--verify", "--quiet", BRANCH, check=False)
    if not exists and cfg.push:
        # Pick up the branch from the remote if this machine is new.
        run(["git", "fetch", "-q", cfg.remote, f"{BRANCH}:{BRANCH}"], check=False)
        exists = git("rev-parse", "--verify", "--quiet", BRANCH, check=False)
    if exists:
        git("worktree", "add", "-q", str(REPO), BRANCH)
    else:
        git("worktree", "add", "-q", "--orphan", "-b", BRANCH, str(REPO))


def _export_db(dest):
    src = sqlite3.connect(WORK / "state.db")
    dst = sqlite3.connect(dest / "state.db")
    with dst:
        src.backup(dst)
    src.close()
    dst.row_factory = sqlite3.Row
    tables = [r[0] for r in dst.execute("SELECT name FROM sqlite_master WHERE type = 'table'")]
    if "tickets" in tables:
        rows = [dict(r) for r in dst.execute("SELECT * FROM tickets ORDER BY id")]
        (dest / "tickets.json").write_text(json.dumps(rows, indent=1, default=str) + "\n")
    for table in ("attempts", "notes", "layers", "stack_queue"):
        if table in tables:
            with open(dest / f"{table}.jsonl", "w") as f:
                for r in dst.execute(f"SELECT * FROM {table} ORDER BY rowid"):
                    f.write(json.dumps(dict(r), default=str) + "\n")
    dst.close()


def _copy_tree(src, dest, skip=frozenset()):
    if not src.exists():
        return
    for p in src.rglob("*"):
        if p.is_dir() or p.name in skip:
            continue
        target = dest / p.relative_to(src)
        if target.exists() and target.stat().st_mtime >= p.stat().st_mtime and target.stat().st_size == p.stat().st_size:
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, target)


def snapshot(cfg, reason="attempt finished"):
    """Commits (and pushes) the current run state. Never raises."""
    try:
        _ensure_repo(cfg)
        _export_db(REPO)
        for name in ("orch.log", "pilot.log", "last-full-summary.json"):
            if (WORK / name).exists():
                shutil.copy2(WORK / name, REPO / name)
        _copy_tree(WORK / "calibration", REPO / "calibration")
        _copy_tree(WORK / "attempts", REPO / "attempts", SKIP_ATTEMPT_FILES)
        git("add", "-A", cwd=REPO)
        if not git("status", "--porcelain", cwd=REPO):
            return
        stamp = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime())
        run(["git", "-c", "user.name=kahvi-orchestrator", "-c", "user.email=orchestrator@kahvi.invalid",
             "commit", "-q", "--no-verify", "-m", f"runs: {reason} ({stamp} UTC)"], cwd=REPO)
        if cfg.push:
            for delay in (0, 2, 4, 8):
                if delay:
                    time.sleep(delay)
                if run(["git", "push", "-q", cfg.remote, f"{BRANCH}:{BRANCH}"], cwd=REPO,
                       check=False).returncode == 0:
                    break
            else:
                log(f"push of {BRANCH} failed; the snapshot is committed locally")
    except Exception as e:  # noqa: BLE001 - a snapshot failure must not stop the swarm
        log(f"run-state snapshot failed: {e!r}")
