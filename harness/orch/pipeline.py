"""One ticket through the pipeline: worker → gates → blind verifier → merge,
and the bookkeeping that decides what happens to the ticket next.

Interruptions (usage limits, overload, logged out) are not the ticket's
fault: the attempt doesn't count, partial work is kept for the next attempt,
and a finished attempt that only lacks its verification resumes there.
"""

import json
import re
import threading
import time
from pathlib import Path

import gates
import llm
import merge
import verifier
import worker
from common import ROOT, WORK, log, read_json, write_json

MERGE_LOCK = threading.Lock()
MAX_ERRORS = 3
# The merge queue `orch run` starts; None means merge inline (one at a time).
MERGES = None
STACKER = None  # stack.Stacker when landing through stacked PRs


def next_tier(cfg, tier):
    tiers = cfg.escalation.get("tiers", ["haiku", "sonnet", "opus"])
    i = tiers.index(tier) if tier in tiers else len(tiers) - 1
    return tiers[i + 1] if i + 1 < len(tiers) else None


def strike(cfg, db, ticket, aid, reason, weight=1, salvage_attempt=None):
    """Records a failed attempt; enough strikes move the ticket up a tier.

    `salvage_attempt`, if given, keeps that attempt's diff for the next try
    instead of starting fresh (used for a merge refusal: unlike a gate or
    verifier rejection, the diff passed both and is worth continuing from,
    the way a `revise` verdict is)."""
    row = db.ticket(ticket.id)
    db.note(ticket.id, aid, reason)
    strikes = row["strikes"] + weight
    if row["attempts"] >= cfg.escalation.get("max_attempts", 6):
        db.set_ticket(ticket.id, status="needs-architect", strikes=strikes, salvage_attempt=None,
                      reason=f"{row['attempts']} attempts without finishing; last: {reason[:300]}")
        return
    if strikes >= cfg.escalation.get("strikes_per_tier", 2):
        up = next_tier(cfg, row["tier"])
        if up is None:
            db.set_ticket(ticket.id, status="needs-architect", strikes=strikes, salvage_attempt=None,
                          reason=f"failed at the top tier; last: {reason[:300]}")
            return
        log(f"{ticket.id}: escalating {row['tier']} -> {up}")
        db.note(ticket.id, aid, f"escalated from {row['tier']} to {up}")
        db.set_ticket(ticket.id, status="open", tier=up, strikes=0, salvage_attempt=None)
        return
    db.set_ticket(ticket.id, status="open", strikes=strikes, salvage_attempt=salvage_attempt)


def infra_error(db, ticket, aid, what):
    """A run that failed for reasons other than limits (crash, bad config).
    A few in a row means something needs a human."""
    row = db.ticket(ticket.id)
    errors = row["errors"] + 1
    db.note(ticket.id, aid, f"{what[:400]}")
    if errors >= MAX_ERRORS:
        db.set_ticket(ticket.id, status="needs-architect", errors=errors, reason=f"{errors} errors in a row: {what[:300]}")
    else:
        db.set_ticket(ticket.id, status="open", errors=errors)


def process(cfg, db, ticket, slot):
    row = db.ticket(ticket.id)
    if row["resume_attempt"]:
        aid = row["resume_attempt"]
        a = db.one("SELECT * FROM attempts WHERE id = ?", aid)
        adir = worker.ATTEMPTS / str(aid)
        rep = read_json(adir / "gate.json")
        if a and a["head"] and rep and rep.get("decision") == "pass":
            log(f"{ticket.id}: resuming verification of attempt {aid}")
            db.set_ticket(ticket.id, resume_attempt=None)
            att = worker.Attempt(aid, "", a["base"], a["head"], 0.0, 0, None, None, str(adir))
            return judge(cfg, db, ticket, att, rep)
        db.set_ticket(ticket.id, resume_attempt=None)

    tier = row["tier"]
    log(f"{ticket.id}: attempt on slot {slot} with {tier}")
    att = worker.attempt(cfg, db, ticket, slot, tier, salvage_attempt=row["salvage_attempt"])
    log(f"{ticket.id}: attempt {att.id} finished (${att.cost:.3f}, {att.turns} turns{', ' + att.kind if att.kind else ''})")

    if att.kind in llm.INTERRUPTED:
        db.set_attempt(att.id, outcome="interrupted", finished=time.time())
        db.uncount_attempt(ticket.id)
        if att.head:
            db.note(ticket.id, att.id, f"interrupted ({att.kind}); the next attempt continues from its partial work")
            db.set_ticket(ticket.id, status="open", salvage_attempt=att.id)
        else:
            db.set_ticket(ticket.id, status="open")
        return
    if att.error and not att.head:
        db.set_attempt(att.id, outcome="error", finished=time.time())
        infra_error(db, ticket, att.id, f"worker run failed: {att.error}")
        return
    if att.blocked and not att.head:
        db.set_attempt(att.id, outcome="blocked", finished=time.time())
        db.note(ticket.id, att.id, f"BLOCKED: {att.blocked}")
        db.set_ticket(ticket.id, status="blocked", reason=att.blocked, errors=0)
        log(f"{ticket.id}: blocked: {att.blocked}")
        return
    if att.blocked:
        # A diff still gets judged on its merits; the reason is kept for context.
        db.note(ticket.id, att.id, f"worker reported BLOCKED but changed files: {att.blocked}")
    if not att.head:
        db.set_attempt(att.id, outcome="no-diff", finished=time.time())
        strike(cfg, db, ticket, att.id, "produced no changes")
        return
    db.set_ticket(ticket.id, errors=0)

    rep = gates.check_attempt(cfg, ticket, slot, att.slot_dir, att.base, att.head, att.dir)
    if rep["decision"] == "reject":
        db.set_attempt(att.id, outcome="gate-reject", finished=time.time())
        log(f"{ticket.id}: gate rejected attempt {att.id}: {'; '.join(rep['reject'])[:200]}")
        strike(cfg, db, ticket, att.id, "rejected by gates: " + "; ".join(rep["reject"])[:600])
        return
    judge(cfg, db, ticket, att, rep)


def judge(cfg, db, ticket, att, rep):
    diff = Path(att.dir, "diff.patch").read_text()
    v, vcost = verifier.verify_pipeline(cfg, ticket, worker.spec_text(ticket.spec), gates.render(rep), diff,
                                        record_dir=Path(att.dir))
    write_json(Path(att.dir) / "verdict.json", v)
    decision = v["decision"]
    prev = db.one("SELECT verifier_cost FROM attempts WHERE id = ?", att.id)
    db.set_attempt(att.id, outcome=decision, verifier_cost=(prev["verifier_cost"] if prev else 0) + vcost)
    log(f"{ticket.id}: verifier: {decision}{' (fakery)' if v.get('fakery') else ''} {v.get('summary', '')[:150]}")
    notes = v.get("notes_for_next_attempt", "")

    if decision == "interrupted":
        db.set_ticket(ticket.id, status="open", resume_attempt=att.id)
        return
    if decision == "error":
        infra_error(db, ticket, att.id, f"verifier produced no decision: {v.get('summary', '')}")
        db.set_ticket(ticket.id, resume_attempt=att.id)
        return
    if decision == "accept":
        if STACKER is not None:
            STACKER.accept(ticket, att, decision)
            db.set_attempt(att.id, finished=time.time())
            return
        db.set_ticket(ticket.id, status="merging")
        if MERGES is not None:
            MERGES.put(ticket, att, decision)
            return
        with MERGE_LOCK:
            m = merge.merge(cfg, ticket, att.id, att.head, decision, att.dir)
        after_merge(cfg, db, ticket, att, m)
    elif decision == "revise":
        db.note(ticket.id, att.id, f"not merged ({', '.join(v['defects'])}); continue from this diff. {notes}")
        row = db.ticket(ticket.id)
        if row["attempts"] >= cfg.escalation.get("max_attempts", 6):
            db.set_ticket(ticket.id, status="needs-architect", reason="ran out of attempts while revising")
        else:
            db.set_ticket(ticket.id, status="open", salvage_attempt=att.id)
    else:
        # Faking is evidence the ticket is beyond this tier: escalate at once.
        weight = cfg.escalation.get("strikes_per_tier", 2) if v.get("fakery") and \
            cfg.escalation.get("fakery_escalates", True) else 1
        strike(cfg, db, ticket, att.id,
               f"rejected ({', '.join(v['defects'])}): {v.get('summary', '')[:300]} {notes}", weight=weight)
    db.set_attempt(att.id, finished=time.time())


def merge_regressions_key(ticket_id):
    return f"merge-regressions:{ticket_id}"


def after_merge(cfg, db, ticket, att, m):
    """Bookkeeping once the merge bot has tried an accepted attempt."""
    write_json(Path(att.dir) / "merge.json", m)
    if not m["merged"]:
        log(f"{ticket.id}: merge refused: {m['reason'][:200]}")
        regressions = m.get("regressions") or []
        detail = m["reason"][:400] + (": " + ", ".join(regressions[:10]) if regressions else "")
        key = merge_regressions_key(ticket.id)
        if regressions and sorted(regressions) == sorted(json.loads(db.kv_get(key) or "[]")):
            # The same diff (or an equivalent one) hit the same regressions
            # on consecutive merge attempts: retrying blind won't fix it, and
            # without this check nothing ever stops it (T-0004 burned 83
            # attempts, all "accept" then "merge refused: 2 regressions",
            # before an architect noticed and fixed the baseline by hand).
            db.note(ticket.id, att.id,
                    f"accepted by the verifier but not merged: {detail}")
            db.set_ticket(ticket.id, status="needs-architect", salvage_attempt=None,
                          reason=f"the same regressions recurred on consecutive merge attempts: "
                                 + ", ".join(regressions[:10]))
            db.kv_set(key, "[]")
            return
        if regressions:
            db.kv_set(key, json.dumps(sorted(regressions)))
        # A merge refusal is a real strike, not a free retry (same bug as
        # above, one attempt earlier): the diff passed gates and the
        # verifier, so it's worth continuing from like a `revise` note, but
        # it still counts toward escalation and max_attempts.
        strike(cfg, db, ticket, att.id, f"accepted by the verifier but not merged: {detail}",
               salvage_attempt=att.id)
        return
    db.kv_set(merge_regressions_key(ticket.id), "[]")
    db.set_attempt(att.id, merged=1, new_passes=m["new_passes"])
    log(f"{ticket.id}: merged (+{m['new_passes']}, {m['total_pass']} total)")
    if m["target_complete"]:
        db.set_ticket(ticket.id, status="done", strikes=0, salvage_attempt=None, reason="")
        return
    left = "\n".join(m["remaining"][:10])
    db.note(ticket.id, att.id, f"merged partial progress. Still failing:\n{left}")
    # Reopen only if the ticket's own functions still report not
    # implemented. Other leftovers are usually blocked on other
    # tickets or engine work: retrying would only escalate and burn
    # budget, so they wait for triage (`orch reset` reopens).
    own = own_not_implemented(ticket)
    if own and any(own.search(r) for r in m["remaining"]):
        db.set_ticket(ticket.id, status="open", strikes=0, salvage_attempt=None)
    else:
        db.set_ticket(ticket.id, status="partial", strikes=0, salvage_attempt=None,
                      reason=f"{len(m['remaining'])} target runs still failing, none in its own code path")


def snake(name):
    """computeFlexBasisForChild -> compute_flex_basis_for_child."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).lower()


def own_not_implemented(ticket):
    """Matches a failure whose not_implemented!("name") names one of the
    ticket's own functions: its `spec` names (Yoga's camelCase, or barre's
    snake_case, last path component) or its title. The exact name:
    `baseline` must not match `calculate_baseline`."""
    names = {ticket.title.strip()}
    for s in ticket.spec:
        last = s.rsplit("::", 1)[-1]
        names |= {s, last, snake(last)}
    names = sorted((n for n in names if n), key=len, reverse=True)
    if not names:
        return None
    alt = "|".join(re.escape(n) for n in names)
    return re.compile(rf"not implemented: ?(?:[\w:]*::)?(?:{alt})(?![\w.\[])")


def refresh_partials(db, pool, results):
    """Re-judges every `partial` ticket against a full run's results: done
    when its whole target passes now, back in the pool when its own function
    reports not implemented again, otherwise still partial with a fresh
    reason naming what blocks it. Returns {status: count} for what changed."""
    from collections import Counter
    by_id = {}
    for r in results:
        by_id[r["id"]] = r
    changed = Counter()
    for row in db.q("SELECT id FROM tickets WHERE status = 'partial'"):
        t = pool.get(row["id"])
        if t is None or not t.target:
            continue
        failing = [r for rid, r in by_id.items()
                   if r["status"] not in ("pass", "skip") and any(rid.startswith(p) for p in t.target)]
        if not failing:
            db.set_ticket(t.id, status="done", reason="")
            db.note(t.id, None, "every target run passes after later merges")
            changed["done"] += 1
            continue
        own = own_not_implemented(t)
        if own and any(own.search(r.get("reason", "")) or own.search("not implemented: " + (r.get("not_implemented") or ""))
                       for r in failing):
            db.set_ticket(t.id, status="open", strikes=0, salvage_attempt=None, reason="")
            db.note(t.id, None, "its own function reports not implemented again")
            changed["open"] += 1
            continue
        blockers = Counter(r["not_implemented"] for r in failing if r.get("not_implemented"))
        other = sum(1 for r in failing if not r.get("not_implemented"))
        parts = [f"{name} ({n})" for name, n in blockers.most_common(4)]
        if other:
            parts.append(f"{other} other failures")
        reason = f"{len(failing)} target runs still failing: " + ", ".join(parts)
        if reason != db.ticket(t.id)["reason"]:
            db.set_ticket(t.id, reason=reason)
    return changed


class MergeQueue:
    """Accepted attempts wait here for the merge bot, so slots go back to work
    at once. One thread merges whatever has queued up as one batch (one full
    conformance run per batch instead of one per attempt)."""

    def __init__(self, cfg, db):
        self.cfg, self.db = cfg, db
        self.max_batch = int(cfg.conf.get("max_merge_batch", 8))
        self.items = []
        self.busy = False
        self.cv = threading.Condition()
        threading.Thread(target=self._loop, daemon=True, name="merge-bot").start()

    def put(self, ticket, att, decision):
        with self.cv:
            self.items.append((ticket, att, decision))
            self.cv.notify()

    def pending(self):
        with self.cv:
            return len(self.items) + (1 if self.busy else 0)

    def _loop(self):
        while True:
            with self.cv:
                while not self.items:
                    self.cv.wait()
                batch, self.items = self.items[:self.max_batch], self.items[self.max_batch:]
                self.busy = True
            try:
                items = [{"ticket": t, "attempt_id": a.id, "head": a.head, "verdict": d, "adir": a.dir}
                         for t, a, d in batch]
                with MERGE_LOCK:
                    results = merge.merge_batch(self.cfg, items)
                for t, a, _ in batch:
                    after_merge(self.cfg, self.db, t, a, results.get(a.id) or
                                {"merged": False, "reason": "the merge bot returned no result"})
                if any(r.get("merged") for r in results.values()):
                    import tickets as ticket_files
                    from gates import read_results
                    changed = refresh_partials(self.db, ticket_files.load_all(ticket_files.TICKETS),
                                               read_results(WORK / "last-full.jsonl"))
                    if changed:
                        log(f"partial tickets after the merge: {dict(changed)}")
            except Exception as e:  # keep the merge bot alive; the attempts stay salvageable
                log(f"merge bot error: {e!r}")
                for t, a, _ in batch:
                    self.db.note(t.id, a.id, f"merge bot error: {e!r}"[:500])
                    self.db.set_ticket(t.id, status="open", salvage_attempt=a.id)
            finally:
                for _, a, _ in batch:
                    self.db.set_attempt(a.id, finished=time.time())
                with self.cv:
                    self.busy = False


def build_tools(cfg):
    """kahvi built a trusted runner binary here. barre's runner is a script
    that always runs from the main checkout (conformance.py), so there is
    nothing to build; this only checks it is there before workers start."""
    from conformance import RUNNER
    if not RUNNER.exists():
        raise SystemExit(f"no conformance runner at {RUNNER} (docs/CONFORMANCE.md)")
    for sub in ("vendor/yoga/yoga", "vendor/googletest"):
        if not (ROOT / sub).exists():
            raise SystemExit(f"{sub} missing: git submodule update --init --depth 1")
