"""The stacker: lands accepted work as a stack of pull requests whose CI runs
the full conformance suite (docs/STACKS.md).

Accepted attempts join a queue and are cherry-picked, in order, onto the
local integration branch (cfg.branch), which workers start from. A layer is
the front of that queue pushed as branch stack/NNNN with a PR on the layer
below. Only one layer waits for CI at a time; whatever is accepted meanwhile
becomes the next layer, so CI runs follow CI capacity. A green layer stays
for Samuel to merge; a red one is closed and its attempts go back to the
front of the queue in halves until the one that breaks something is found.
"""

import json
import re
import threading
import time
from pathlib import Path
from types import SimpleNamespace

import github
from common import ROOT, WORK, git, log, run, write_json
from gates import read_results

TREE = WORK / "stack-tree"
LAYERS = WORK / "layers"
BASELINE = "conformance/expected-pass.txt"

SCHEMA = """
CREATE TABLE IF NOT EXISTS layers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    branch TEXT NOT NULL,
    pr INTEGER,
    base TEXT NOT NULL,          -- the branch the PR is based on
    head TEXT NOT NULL,          -- the pushed commit
    status TEXT NOT NULL,        -- pending | green | red | stuck | merged | closed
    items TEXT NOT NULL,         -- JSON list of queue items
    reruns INTEGER NOT NULL DEFAULT 0,
    rerun_of INTEGER,            -- the check run a re-run replaces
    note TEXT NOT NULL DEFAULT '',
    created REAL NOT NULL,
    updated REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS stack_queue (
    pos REAL PRIMARY KEY,        -- order in the stack
    kind TEXT NOT NULL,          -- ticket | land
    ticket TEXT NOT NULL,        -- ticket id, or the architect branch for land
    attempt INTEGER,
    verdict TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,        -- commit to cherry-pick (ticket) or merge (land)
    cap INTEGER NOT NULL DEFAULT 0,  -- largest layer this item may share; 0 = no cap beyond max_layer
    composed TEXT                -- its commit on the integration branch
);
"""

LIVE = ("pending", "green", "stuck")


def _commit(message, cwd=TREE):
    run(["git", "-c", "user.name=barre-stacker", "-c", "user.email=stacker@barre.invalid",
         "commit", "-q", "--no-verify", "-m", message], cwd=cwd)


class Stacker:
    def __init__(self, cfg, db, pool_loader):
        self.cfg, self.db = cfg, db
        s = cfg.stack
        self.gh = github.GitHub(s["repo"])
        self.trunk = s.get("trunk", "main")
        self.trunk_pr = s.get("trunk_pr")  # an open PR the stack sits on (m1-spine's PR into main)
        self.max_layer = int(s.get("max_layer", 8))
        self.check = s.get("check", "conformance")
        self.required = list(s.get("required_checks", [self.check]))
        self.artifact = s.get("artifact", "conf-results")
        # Layers that change these paths get `stress_label` (extra CI shards).
        self.stress_paths = re.compile(s["stress_paths"]) if s.get("stress_paths") else None
        self.stress_label = s.get("stress_label", "stress")
        self.poll_seconds = int(s.get("poll_seconds", 60))
        # Layer branches are PREFIX + the layer id; a scratch orchestrator
        # (its own BARRE_WORK, so its own ids) needs its own prefix.
        self.branch_prefix = s.get("branch_prefix", "stack/")
        self.pool_loader = pool_loader
        self.lock = threading.RLock()
        self.wake = threading.Event()
        with db.lock:
            db.conn.executescript(SCHEMA)
        LAYERS.mkdir(parents=True, exist_ok=True)

    # The queue

    def queue(self):
        return self.db.q("SELECT * FROM stack_queue ORDER BY pos")

    def _push_queue(self, kind, ticket, attempt, verdict, source, cap=0, front=False):
        row = self.db.one("SELECT MIN(pos) AS lo, MAX(pos) AS hi FROM stack_queue")
        if front:
            pos = (row["lo"] if row["lo"] is not None else 0) - 1
        else:
            pos = (row["hi"] if row["hi"] is not None else 0) + 1
        self.db.q("INSERT INTO stack_queue (pos, kind, ticket, attempt, verdict, source, cap) VALUES (?, ?, ?, ?, ?, ?, ?)",
                  pos, kind, ticket, attempt, verdict, source, cap)

    def accept(self, ticket, att, decision):
        """An accepted attempt: onto the integration branch now, into a layer later."""
        with self.lock:
            self.db.set_ticket(ticket.id, status="stacked")
            self._push_queue("ticket", ticket.id, att.id, decision, att.head)
            self.compose(incremental=True)
        self.wake.set()

    def land(self, branch):
        """An architect branch, merged on top as its own layer."""
        sha = git("rev-parse", "--verify", branch)
        # Only queued here: the running stacker composes it (this may be
        # another process, `./orch land`).
        self._push_queue("land", branch, None, "architect", sha, cap=1)
        self.wake.set()
        return sha

    # The integration branch

    def floor(self):
        """The newest live layer, or None when the stack is empty."""
        return self.db.one(f"SELECT * FROM layers WHERE status IN {LIVE} ORDER BY id DESC LIMIT 1")

    def trunk_branch(self):
        """The branch under the stack: the trunk PR's branch until that PR
        merges, then main."""
        if self.trunk_pr and self.trunk != "main" and self.gh.pr(self.trunk_pr)["merged"]:
            log(f"stack: #{self.trunk_pr} merged; the stack now sits on main")
            self.trunk, self.trunk_pr = "main", None
        return self.trunk

    def floor_commit(self):
        f = self.floor()
        if f:
            return f["head"]
        trunk = self.trunk_branch()
        run(["git", "fetch", "-q", self.cfg.remote, trunk], check=False)
        return git("rev-parse", f"{self.cfg.remote}/{trunk}")

    def _prepare(self, commit):
        if not (TREE / ".git").exists():
            git("worktree", "add", "--detach", str(TREE), commit)
        for abort in (["cherry-pick", "--abort"], ["merge", "--abort"]):
            git(*abort, cwd=TREE, check=False)
        git("checkout", "--detach", "--force", commit, cwd=TREE)
        git("clean", "-fdx", "-e", "target", cwd=TREE)

    def _apply(self, item):
        """Applies one queue item on TREE's HEAD; returns the new commit or None on a conflict."""
        if item["kind"] == "land":
            r = run(["git", "-c", "user.name=barre-stacker", "-c", "user.email=stacker@barre.invalid",
                     "merge", "--no-ff", "-q", "-m", f"Merge {item['ticket']} (architect)", item["source"]],
                    cwd=TREE, check=False)
            if r.returncode != 0:
                git("merge", "--abort", cwd=TREE, check=False)
                return None
            return git("rev-parse", "HEAD", cwd=TREE)
        r = run(["git", "cherry-pick", "--no-commit", item["source"]], cwd=TREE, check=False)
        if r.returncode != 0:
            git("cherry-pick", "--abort", cwd=TREE, check=False)
            git("reset", "--hard", "-q", "HEAD", cwd=TREE, check=False)
            return None
        git("add", "-A", cwd=TREE)
        if not run(["git", "diff", "--cached", "--quiet"], cwd=TREE, check=False).returncode:
            return git("rev-parse", "HEAD", cwd=TREE)  # already on the stack
        title = self.pool_loader().get(item["ticket"])
        _commit(f"{item['ticket']}: {title.title if title else item['ticket']}\n\nattempt {item['attempt']}")
        return git("rev-parse", "HEAD", cwd=TREE)

    def _conflict(self, item):
        self.db.q("DELETE FROM stack_queue WHERE pos = ?", item["pos"])
        if item["kind"] == "land":
            log(f"stack: {item['ticket']} conflicts with the stack; land it again after merging the stack top into it")
            return
        log(f"stack: {item['ticket']} (attempt {item['attempt']}) conflicts with newer work; back to the pool")
        self.db.note(item["ticket"], item["attempt"], "accepted, but conflicts with newer work on the stack; "
                     "continue from this diff on the new base")
        self.db.set_ticket(item["ticket"], status="open", salvage_attempt=item["attempt"])

    def compose(self, incremental=False):
        """Rebuilds the integration branch: the floor plus every queued item.
        Incremental: only the items not yet composed, when the branch still
        ends at the last composed one."""
        items = self.queue()
        n = sum(1 for i in items if i["composed"])
        start, todo = self.floor_commit(), items
        if incremental and all(i["composed"] for i in items[:n]):
            expect = items[n - 1]["composed"] if n else start
            if git("rev-parse", self.cfg.branch, check=False) == expect:
                start, todo = expect, items[n:]
        if not todo and not incremental:
            self._set_branch(start)
            return
        if not todo:
            return
        self._prepare(start)
        for item in todo:
            before = git("rev-parse", "HEAD", cwd=TREE)
            commit = self._apply(item)
            if commit is None:
                self._conflict(item)
                continue
            if item["kind"] == "land" and commit == before:
                # Landed twice, or already merged: a layer of it would be empty.
                self.db.q("DELETE FROM stack_queue WHERE pos = ?", item["pos"])
                log(f"stack: {item['ticket']} ({item['source'][:10]}) is already on the stack; dropped")
                continue
            self.db.q("UPDATE stack_queue SET composed = ? WHERE pos = ?", commit, item["pos"])
        self._set_branch(git("rev-parse", "HEAD", cwd=TREE))

    def _set_branch(self, commit):
        current = git("rev-parse", "--abbrev-ref", "HEAD")
        if current == self.cfg.branch:
            # The main checkout runs the harness from this branch: move it too.
            r = run(["git", "reset", "-q", "--keep", commit], check=False)
            if r.returncode != 0:
                log(f"stack: could not move the main checkout to {commit[:10]}: {r.stderr.strip()[:200]}")
        else:
            git("update-ref", f"refs/heads/{self.cfg.branch}", commit)

    # Layers

    def _baseline_for(self, floor):
        """The pass set a new layer must keep: its floor's CI result."""
        if floor is None:
            return None  # the trunk's own file
        p = LAYERS / str(floor["id"]) / "expected-pass.txt"
        return p.read_text() if p.exists() else None

    def push_next(self):
        # A layer whose PR was never opened (the process stopped, or GitHub
        # refused it as empty): its items are still queued, so drop it and
        # recompose. One whose head the trunk already has counts as merged,
        # so the PR above it retargets past it.
        orphans = self.db.q(f"SELECT * FROM layers WHERE status IN {LIVE} AND pr IS NULL")
        if orphans:
            trunk = self.trunk_branch()
            run(["git", "fetch", "-q", self.cfg.remote, trunk], check=False)
        for layer in orphans:
            merged = run(["git", "merge-base", "--is-ancestor", layer["head"], f"{self.cfg.remote}/{trunk}"],
                         check=False).returncode == 0
            status = "merged" if merged else "closed"
            self.db.q("UPDATE layers SET status = ?, updated = ? WHERE id = ?", status, time.time(), layer["id"])
            log(f"stack: {layer['branch']} never got a PR; {'already in ' + trunk if merged else 'dropped'}")
        if orphans:
            self.compose()
            return False
        if self.db.one("SELECT 1 FROM layers WHERE status IN ('pending', 'stuck')"):
            return False
        items = self.queue()
        if not items or not all(i["composed"] for i in items):
            return False
        group = []
        for it in items:
            limit = min([self.max_layer] + [g["cap"] for g in group + [it] if g["cap"]])
            if len(group) >= limit:
                break
            if group and it["kind"] == "land":
                break
            group.append(it)
            if it["kind"] == "land":
                break
        floor = self.floor()
        last = group[-1]["composed"]
        self._prepare(last)
        baseline = self._baseline_for(floor)
        if baseline is not None and (TREE / BASELINE).read_text() != baseline:
            (TREE / BASELINE).write_text(baseline)
            git("add", BASELINE, cwd=TREE)
            _commit(f"conformance baseline: what the layer below passes in CI\n\n{baseline.count(chr(10))} expected passing runs.")
        head = git("rev-parse", "HEAD", cwd=TREE)
        base_branch = floor["branch"] if floor else self.trunk_branch()
        now = time.time()
        with self.db.lock:
            cur = self.db.conn.execute(
                "INSERT INTO layers (branch, base, head, status, items, created, updated) VALUES ('', ?, ?, 'pending', ?, ?, ?)",
                (base_branch, head, json.dumps([dict(i) for i in group]), now, now))
            lid = cur.lastrowid
        branch = f"{self.branch_prefix}{lid:04d}"
        self.db.q("UPDATE layers SET branch = ? WHERE id = ?", branch, lid)
        for delay in (0, 2, 4, 8, 16):
            time.sleep(delay)
            if run(["git", "push", "-q", "--force", self.cfg.remote, f"{head}:refs/heads/{branch}"],
                   check=False).returncode == 0:
                break
        else:
            raise RuntimeError(f"could not push {branch}")
        pr = self.gh.create_pr(base_branch, branch, self._title(group), self._body(group, floor))
        self.db.q("UPDATE layers SET pr = ? WHERE id = ?", pr, lid)
        changed = run(["git", "diff", "--name-only", self.floor_commit() if not floor else floor["head"], head],
                      check=False).stdout.split()
        if self.stress_paths and any(self.stress_paths.match(p) for p in changed):
            self.gh.add_labels(pr, [self.stress_label])
        self._link(floor, pr)
        self.db.q(f"DELETE FROM stack_queue WHERE pos IN ({','.join('?' * len(group))})", *[g["pos"] for g in group])
        log(f"stack: pushed {branch} as #{pr} with {', '.join(g['ticket'] for g in group)}")
        # The rest of the queue moves onto the new layer (its baseline commit).
        self.compose()
        return True

    def _title(self, group):
        if len(group) == 1:
            g = group[0]
            t = self.pool_loader().get(g["ticket"])
            return f"{g['ticket']}: {t.title}" if t else f"Land {g['ticket']}"
        return f"{len(group)} tickets: " + ", ".join(g["ticket"] for g in group)[:200]

    def _body(self, group, floor):
        pool = self.pool_loader()
        lines = []
        for g in group:
            if g["kind"] == "land":
                lines.append(f"- architect branch `{g['ticket']}` at {g['source'][:10]}")
            else:
                t = pool.get(g["ticket"])
                lines.append(f"- {g['ticket']}: {t.title if t else ''} (attempt {g['attempt']}, verdict {g['verdict']})")
        below = f"#{floor['pr']}" if floor and floor["pr"] else f"`{self.trunk_branch()}`"
        return ("Swarm work, stacked on " + below + ". Accepted by the gates and the blind verifier; "
                "CI runs the full conformance suite against the layer below's results (0 regressions).\n\n"
                + "\n".join(lines) + "\n\nOpened by the barre stacker (docs/STACKS.md).")

    def _link(self, floor, pr):
        """Adds the PR to the GitHub stack. Linking is GitHub's view of the
        stack (and lets GitHub retarget PRs as the bottom merges); the base
        branches already make the stack."""
        below = floor["pr"] if floor and floor["status"] in LIVE else None
        if below is None and self.trunk_pr and self.gh.pr(self.trunk_pr)["state"] == "open":
            below = self.trunk_pr
        if below is None:
            return
        try:
            s = self.gh.stack_for_pr(below)
            if s:
                self.gh.add_to_stack(s["number"], [pr])
            else:
                self.gh.create_stack([below, pr])
            return
        except github.GitHubError as e:
            log(f"stack: adding #{pr} to the GitHub stack failed ({e}); relinking")
        self.relink()

    def relink(self):
        """Recreates the GitHub stack from the open layers (after a layer in
        the middle was closed)."""
        prs = [r["pr"] for r in self.db.q(f"SELECT pr FROM layers WHERE status IN {LIVE} AND pr IS NOT NULL ORDER BY id")]
        if self.trunk_pr and self.gh.pr(self.trunk_pr)["state"] == "open":
            prs.insert(0, self.trunk_pr)
        try:
            seen = set()
            for p in prs:
                st = self.gh.stack_for_pr(p)
                if st and st["number"] not in seen:
                    seen.add(st["number"])
                    self.gh.unstack(st["number"])
            if len(prs) > 1:
                self.gh.create_stack(prs)
        except github.GitHubError as e:
            log(f"stack: could not relink the GitHub stack: {e}")

    # CI

    def poll(self):
        layer = self.db.one("SELECT * FROM layers WHERE status = 'pending' ORDER BY id LIMIT 1")
        if not layer:
            return False
        # Merging the bottom of a GitHub stack rebases the PRs above it. The
        # old head's CI is then cancelled or stale: re-running it would
        # cancel the new head's run. Judge the head the PR has now.
        head = self.gh.pr(layer["pr"])["head"]["sha"] if layer["pr"] else layer["head"]
        if head != layer["head"]:
            run(["git", "fetch", "-q", self.cfg.remote, layer["branch"]], check=False)
            self.db.q("UPDATE layers SET head = ?, reruns = 0, rerun_of = NULL, updated = ? WHERE id = ?",
                      head, time.time(), layer["id"])
            log(f"stack: {layer['branch']} (#{layer['pr']}) moved on GitHub to {head[:10]}; judging that")
            # Queued items were composed on the old head.
            self.compose()
            layer = self.db.one("SELECT * FROM layers WHERE id = ?", layer["id"])
        runs = {}
        for c in self.gh.check_runs(layer["head"]):
            # The newest run of each check (re-runs add new ones).
            if c["name"] not in runs or c["id"] > runs[c["name"]]["id"]:
                runs[c["name"]] = c
        # A failed side check (fmt, clippy, unit tests) decides at once; no
        # need to wait for the full suite.
        early = [n for n in self.required if n != self.check and n in runs and runs[n]["status"] == "completed"
                 and runs[n]["conclusion"] not in ("success", "skipped", "neutral")]
        if early and not (layer["rerun_of"] and runs[self.check]["id"] == layer["rerun_of"]):
            self._red(layer, f"CI failed: {', '.join(early)}", [])
            return True
        if any(name not in runs or runs[name]["status"] != "completed" for name in self.required):
            return False
        if layer["rerun_of"] and runs[self.check]["id"] == layer["rerun_of"]:
            return False  # the re-run has not started yet
        main = runs[self.check]
        files = self.gh.artifact(github.GitHub.run_id(main), self.artifact) if main["conclusion"] in ("success", "failure") else None
        summary = json.loads(files["summary.json"]) if files and "summary.json" in files else None
        failed = [n for n in self.required if runs[n]["conclusion"] != "success"]
        ldir = LAYERS / str(layer["id"])
        ldir.mkdir(parents=True, exist_ok=True)
        for name, data in (files or {}).items():
            (ldir / Path(name).name).write_bytes(data)
        if not failed and summary and summary.get("valid", True):
            self._green(layer, ldir)
        elif summary is None or not summary.get("valid", True) and not summary.get("regressions"):
            self._infra(layer, runs, failed)
        else:
            reason = (f"{len(summary['regressions'])} regressions in the full suite"
                      if summary.get("regressions") else f"CI failed: {', '.join(failed)}")
            self._red(layer, reason, summary.get("regressions", [])[:50])
        return True

    def _green(self, layer, ldir):
        from pipeline import after_merge, refresh_partials
        pool = self.pool_loader()
        results = read_results(ldir / "results.jsonl")
        summary = json.loads((ldir / "summary.json").read_text())
        new = set(summary.get("new_passes", []))
        items = json.loads(layer["items"])
        self.db.q("UPDATE layers SET status = 'green', updated = ? WHERE id = ?", time.time(), layer["id"])
        log(f"stack: {layer['branch']} (#{layer['pr']}) green: {summary.get('pass')}/{summary.get('runs')} passing, +{len(new)}")
        for it in items:
            if it["kind"] != "ticket" or it["ticket"] not in pool:
                continue
            t = pool[it["ticket"]]
            mine = [r for r in results if any(r["id"].startswith(p) for p in t.target)]
            remaining = [f"{r['id']}  {r.get('reason', '')[:140]}" for r in mine if r["status"] != "pass"]
            gained = sum(1 for r in mine if r["id"] in new)
            att = SimpleNamespace(id=it["attempt"], dir=str(WORK / "attempts" / str(it["attempt"])))
            after_merge(self.cfg, self.db, t, att, {
                "merged": True, "commit": layer["head"], "batch": [i["ticket"] for i in items],
                "new_passes": gained, "total_pass": summary.get("pass"),
                "target_complete": bool(mine) and not remaining, "remaining": remaining})
        changed = refresh_partials(self.db, pool, results)
        if changed:
            log(f"stack: partial tickets after {layer['branch']}: {dict(changed)}")
        (WORK / "last-full.jsonl").write_text((ldir / "results.jsonl").read_text())
        write_json(WORK / "last-full-summary.json", summary)

    def _infra(self, layer, runs, failed):
        if layer["reruns"] < 1:
            for name in failed or self.required:
                rid = github.GitHub.run_id(runs[name])
                if rid:
                    self.gh.rerun_failed(rid)
            self.db.q("UPDATE layers SET reruns = reruns + 1, rerun_of = ?, updated = ? WHERE id = ?",
                      runs[self.check]["id"], time.time(), layer["id"])
            log(f"stack: {layer['branch']} CI produced no valid result; re-running once")
            return
        note = f"CI failed twice without a valid conformance result ({', '.join(failed)}); needs the architect"
        self.db.q("UPDATE layers SET status = 'stuck', note = ?, updated = ? WHERE id = ?", note, time.time(), layer["id"])
        log(f"stack: needs-architect: {layer['branch']} (#{layer['pr']}): {note}. "
            "Fix CI, then `./orch stack retry`.")

    def _red(self, layer, reason, regressions):
        items = json.loads(layer["items"])
        self.db.q("UPDATE layers SET status = 'red', note = ?, updated = ? WHERE id = ?", reason, time.time(), layer["id"])
        log(f"stack: {layer['branch']} (#{layer['pr']}) red: {reason}")
        detail = "\n".join(f"- `{r}`" for r in regressions[:30])
        try:
            if len(items) > 1:
                msg = f"Red: {reason}. Splitting its {len(items)} tickets into smaller layers to find the cause."
            else:
                msg = f"Red: {reason}. The ticket goes back to the pool."
            self.gh.comment(layer["pr"], msg + ("\n\n" + detail if detail else ""))
            self.gh.update_pr(layer["pr"], state="closed")
        except github.GitHubError as e:
            log(f"stack: could not close #{layer['pr']}: {e}")
        self.relink()
        if len(items) > 1:
            half = (len(items) + 1) // 2
            # Back to the front of the queue, in order, each half its own layer.
            for group in (items[half:], items[:half]):
                for it in reversed(group):
                    self._push_queue(it["kind"], it["ticket"], it["attempt"], it["verdict"], it["source"],
                                     cap=len(group), front=True)
        else:
            it = items[0]
            if it["kind"] == "land":
                log(f"stack: architect branch {it['ticket']} is red in CI ({reason}); fix it and land again")
            else:
                note = f"accepted, but its layer failed CI: {reason}"
                if regressions:
                    note += "\nRegressed:\n" + "\n".join(regressions[:20])
                self.db.note(it["ticket"], it["attempt"], note)
                self.db.set_ticket(it["ticket"], status="open", salvage_attempt=it["attempt"])
        self.compose()

    def retry(self):
        """After a stuck layer's CI is fixed: close it and stack its work again."""
        with self.lock:
            for layer in self.db.q("SELECT * FROM layers WHERE status = 'stuck'"):
                self.db.q("UPDATE layers SET status = 'closed', updated = ? WHERE id = ?", time.time(), layer["id"])
                try:
                    self.gh.update_pr(layer["pr"], state="closed")
                except github.GitHubError:
                    pass
                for it in reversed(json.loads(layer["items"])):
                    self._push_queue(it["kind"], it["ticket"], it["attempt"], it["verdict"], it["source"],
                                     cap=it["cap"], front=True)
            self.compose()
        self.wake.set()

    # Merges on GitHub

    def sync(self):
        """Notices layers Samuel merged (or closed) and keeps PR bases right."""
        changed = False
        for layer in self.db.q(f"SELECT * FROM layers WHERE status IN {LIVE} AND pr IS NOT NULL ORDER BY id"):
            pr = self.gh.pr(layer["pr"])
            if pr["merged"]:
                self.db.q("UPDATE layers SET status = 'merged', updated = ? WHERE id = ?", time.time(), layer["id"])
                log(f"stack: {layer['branch']} (#{layer['pr']}) merged into {pr['base']['ref']}")
                changed = True
            elif pr["state"] == "closed":
                self.db.q("UPDATE layers SET status = 'closed', updated = ? WHERE id = ?", time.time(), layer["id"])
                log(f"stack: {layer['branch']} (#{layer['pr']}) was closed on GitHub; dropped from the stack")
                changed = True
        if self._restack():
            changed = True
        if changed:
            self.compose()
        # The lowest open layer targets whatever its merged layers merged into.
        lowest = self.db.one(f"SELECT * FROM layers WHERE status IN {LIVE} AND pr IS NOT NULL ORDER BY id LIMIT 1")
        if lowest:
            target = lowest["base"]
            while True:
                below = self.db.one("SELECT * FROM layers WHERE branch = ? AND status = 'merged'", target)
                if not below:
                    break
                target = below["base"]
            if target == self.trunk:
                target = self.trunk_branch()
            current = self.gh.pr(lowest["pr"])["base"]["ref"]
            if target != current:
                try:
                    self.gh.update_pr(lowest["pr"], base=target)
                    log(f"stack: #{lowest['pr']} now targets {target}")
                except github.GitHubError as e:
                    # GitHub retargets PRs in a stack itself.
                    if "part of a stack" not in str(e):
                        raise
            if target != lowest["base"]:
                self.db.q("UPDATE layers SET base = ? WHERE id = ?", target, lowest["id"])
        return changed

    def _restack(self):
        """After a squash merge into the trunk, the live layers' history is
        no longer under the trunk (GitHub then reports the lowest PR as
        conflicting). Replays every live layer onto the trunk, bottom first,
        and force-pushes the bot-owned branches; CI runs again on each."""
        live = self.db.q(f"SELECT * FROM layers WHERE status IN {LIVE} AND pr IS NOT NULL ORDER BY id")
        if not live:
            return False
        below = self.db.one("SELECT * FROM layers WHERE status = 'merged' AND id < ? ORDER BY id DESC LIMIT 1",
                            live[0]["id"])
        if not below:
            return False
        trunk = self.trunk_branch()
        run(["git", "fetch", "-q", self.cfg.remote, trunk], check=False)
        onto = git("rev-parse", f"{self.cfg.remote}/{trunk}")

        def contains(ancestor, commit):
            return run(["git", "merge-base", "--is-ancestor", ancestor, commit], check=False).returncode == 0

        if contains(below["head"], onto) or contains(onto, live[0]["head"]):
            return False  # merged with a merge commit, or already replayed
        log(f"stack: {below['branch']} was squashed into {trunk}; replaying {len(live)} live layer(s) onto it")
        upstream = below["head"]
        # From here on the merged layer's head is where it landed on the trunk,
        # so a later move of the trunk is not mistaken for another squash.
        self.db.q("UPDATE layers SET head = ? WHERE id = ?", onto, below["id"])
        for layer in live:
            self._prepare(layer["head"])
            r = run(["git", "-c", "user.name=barre-stacker", "-c", "user.email=stacker@barre.invalid",
                     "rebase", "-q", "--rebase-merges", "--onto", onto, upstream], cwd=TREE, check=False)
            if r.returncode != 0:
                git("rebase", "--abort", cwd=TREE, check=False)
                self.db.q("UPDATE layers SET status = 'stuck', note = ?, updated = ? WHERE id = ?",
                          f"replay onto {trunk} after a squash conflicted; needs-architect", time.time(), layer["id"])
                log(f"stack: needs-architect: replaying {layer['branch']} onto {trunk[:10]} conflicted")
                return True
            head = git("rev-parse", "HEAD", cwd=TREE)
            run(["git", "push", "-q", f"--force-with-lease=refs/heads/{layer['branch']}:{layer['head']}",
                 self.cfg.remote, f"{head}:refs/heads/{layer['branch']}"])
            # A green layer is green again only once CI says so on the new head.
            self.db.q("UPDATE layers SET head = ?, status = 'pending', reruns = 0, updated = ? WHERE id = ?",
                      head, time.time(), layer["id"])
            log(f"stack: replayed {layer['branch']} (#{layer['pr']}) onto {onto[:10]} as {head[:10]}")
            upstream, onto = layer["head"], head
        return True

    # The loop

    def tick(self):
        with self.lock:
            if any(not i["composed"] for i in self.queue()):
                self.compose(incremental=True)
            # CI first: a layer Samuel merges as soon as it turns green must
            # still be judged (tickets done, partials), not just marked merged.
            self.poll()
            self.sync()
            self.push_next()

    def run_forever(self):
        while True:
            try:
                self.tick()
            except Exception as e:  # the stacker must outlive GitHub hiccups
                log(f"stack: error: {e!r}")
            self.wake.wait(self.poll_seconds)
            self.wake.clear()

    def start(self):
        with self.lock:
            self.compose()
        threading.Thread(target=self.run_forever, daemon=True, name="stacker").start()

    def pending(self):
        """Queued items plus the layer waiting for CI (keeps `orch run` alive)."""
        q = self.db.one("SELECT COUNT(*) AS n FROM stack_queue")["n"]
        return q + self.db.one("SELECT COUNT(*) AS n FROM layers WHERE status = 'pending'")["n"]

    def status(self):
        lines = []
        for layer in self.db.q("SELECT * FROM layers ORDER BY id DESC LIMIT 15"):
            items = json.loads(layer["items"])
            lines.append(f"  {layer['branch']}  #{layer['pr']}  {layer['status']:<8} on {layer['base']:<12} "
                         f"{', '.join(i['ticket'] for i in items)[:90]}" + (f"  ({layer['note'][:80]})" if layer["note"] else ""))
        q = self.queue()
        lines.append(f"  queued for the next layer: {len(q)}" + (f" ({', '.join(i['ticket'] for i in q[:8])})" if q else ""))
        return "\n".join(lines)
