"""barre orchestrator CLI. Run via ./orch from the repository root.

  ./orch run [--slots N] [--only T-0001,...] [--max-attempts N]   grind the ticket pool
  ./orch status                                                   tickets, costs, outcomes
  ./orch stack [retry]                                            the stack of PRs; retry stuck layers
  ./orch land BRANCH                                              stack an architect branch
  ./orch usage                                                    the plan's usage windows right now
  ./orch show ATTEMPT                                             one attempt's artifacts
  ./orch reset TICKET [--tier haiku]                              put a ticket back in the pool
  ./orch partials [--results F]                                   re-judge partial tickets
  ./orch triage [--fresh] [--top N]                               cluster conformance failures
  ./orch calibrate [--model M] [--only CASE]                      run the verifier red-team set
"""

import argparse
import concurrent.futures as cf
import json
import sys
import time
from collections import Counter
from pathlib import Path

import llm
import pipeline
import snapshot
import tickets as ticket_files
from common import WORK, git, load_config, log, read_json
from db import DB

STOP_FILE = WORK / "stop"
from limits import Limits


def cmd_run(args):
    cfg = load_config()
    if args.slots:
        cfg.slots = args.slots
    if args.branch:
        cfg.branch = args.branch
    git("rev-parse", "--verify", cfg.branch)
    log(f"checking the conformance runner; integration branch {cfg.branch}, {cfg.slots} slots")
    pipeline.build_tools(cfg)
    db = DB()
    # Accepted attempts that were waiting for the merge bot when the last run
    # stopped go back in the merge queue instead of starting over.
    unmerged = db.q(
        "SELECT t.id AS ticket, MAX(a.id) AS aid FROM tickets t JOIN attempts a ON a.ticket = t.id "
        "WHERE t.status = 'merging' AND a.outcome = 'accept' AND a.merged = 0 GROUP BY t.id")
    db.release_stale_claims()
    limits = Limits(cfg, db)
    llm.LIMITS = limits
    only = set(args.only.split(",")) if args.only else None
    tickets_dir = Path(args.tickets) if args.tickets else ticket_files.TICKETS
    pool = ticket_files.load_all(tickets_dir)
    db.sync_tickets(pool)
    if cfg.stack.get("repo"):
        import stack
        stacker = stack.Stacker(cfg, db, lambda: ticket_files.load_all(tickets_dir))
        pipeline.STACKER = stacker
        stacker.start()
        merges = stacker
        # Accepted attempts the merge bot never got to go on the stack.
        for row in unmerged:
            a = db.one("SELECT * FROM attempts WHERE id = ?", row["aid"])
            if row["ticket"] in pool and a["head"]:
                log(f"{row['ticket']}: onto the stack (attempt {a['id']}, accepted before the switch)")
                stacker.accept(pool[row["ticket"]], pipeline.worker.Attempt(
                    a["id"], "", a["base"], a["head"], 0.0, 0, None, None,
                    str(pipeline.worker.ATTEMPTS / str(a["id"]))), "accept")
        unmerged = []
    else:
        merges = pipeline.MergeQueue(cfg, db)
        pipeline.MERGES = merges
    for row in unmerged:
        a = db.one("SELECT * FROM attempts WHERE id = ?", row["aid"])
        if row["ticket"] in pool and a["head"]:
            log(f"{row['ticket']}: back in the merge queue (attempt {a['id']})")
            db.set_ticket(row["ticket"], status="merging")
            merges.put(pool[row["ticket"]], pipeline.worker.Attempt(
                a["id"], "", a["base"], a["head"], 0.0, 0, None, None, str(pipeline.worker.ATTEMPTS / str(a["id"]))),
                "accept")
    started = 0
    STOP_FILE.unlink(missing_ok=True)
    with cf.ThreadPoolExecutor(cfg.slots) as ex:
        free = list(range(cfg.slots))
        running = {}
        while True:
            try:
                pool = ticket_files.load_all(tickets_dir)
                db.sync_tickets(pool)
            except ValueError as e:
                log(f"ticket files invalid, keeping the previous set: {e}")
            # While paused (usage limits) nothing new starts; running work finishes or
            # waits inside its next model call.
            stopping = STOP_FILE.exists()
            while free and not stopping and not limits.fatal and not limits.paused() and (
                args.max_attempts is None or started < args.max_attempts
            ):
                t = db.claim_next(pool, only)
                if t is None:
                    break
                slot = free.pop()
                running[ex.submit(pipeline.process, cfg, db, t, slot)] = (slot, t.id)
                started += 1
            if not running and merges.pending() and not stopping:
                # Only the merge bot (or the stack's CI) is busy: wait for it
                # (it may free tickets that unblock others).
                time.sleep(10)
                continue
            if not running:
                if limits.fatal or stopping:
                    break
                if limits.paused():
                    if not limits.wait():
                        break
                    continue
                break
            done, _ = cf.wait(running, timeout=60, return_when=cf.FIRST_COMPLETED)
            finished = []
            for f in done:
                slot, tid = running.pop(f)
                free.append(slot)
                finished.append(tid)
                if f.exception():
                    log(f"{tid}: pipeline error: {f.exception()!r}")
                    db.note(tid, None, f"orchestrator error: {f.exception()!r}"[:500])
                    db.set_ticket(tid, status="open")
            if finished:
                # Version the run state (orch-runs branch) after every attempt.
                snapshot.snapshot(cfg, ", ".join(sorted(finished)))
    snapshot.snapshot(cfg, "orchestrator stopped")
    if STOP_FILE.exists():
        STOP_FILE.unlink(missing_ok=True)
        log("stopped on request")
    else:
        log(f"stopped: {limits.fatal}" if limits.fatal else "nothing left to do")
    cmd_status(args)


def _stacker(args):
    import stack
    cfg = load_config()
    if not cfg.stack.get("repo"):
        sys.exit("no [stack] repo configured (docs/STACKS.md)")
    return stack.Stacker(cfg, DB(), lambda: ticket_files.load_all(ticket_files.TICKETS))


def cmd_stack(args):
    """The stack of PRs: layers, CI state and what is queued."""
    s = _stacker(args)
    if args.action == "retry":
        s.retry()
        print("stuck layers closed; their work is queued again")
    else:
        print(s.status())


def cmd_land(args):
    """Queues an architect branch as its own layer on top of the stack."""
    sha = _stacker(args).land(args.branch)
    print(f"{args.branch} ({sha[:10]}) queued; the running orchestrator stacks it as its own layer")


def cmd_snapshot(args):
    """Commits the run state to the orch-runs branch now."""
    snapshot.snapshot(load_config(), "manual snapshot")
    print(f"run state committed to {snapshot.BRANCH} ({snapshot.REPO})")


def cmd_stop(args):
    """Asks a running `orch run` to start nothing new and exit once its
    in-flight attempts (and merges) finish."""
    STOP_FILE.parent.mkdir(parents=True, exist_ok=True)
    STOP_FILE.write_text("stop\n")
    print(f"stop requested; `orch run` exits after its in-flight attempts ({STOP_FILE})")


def cmd_status(args):
    db = DB()
    rows = db.q("SELECT * FROM tickets ORDER BY id")
    by = Counter(r["status"] for r in rows)
    print("tickets: " + ", ".join(f"{k} {v}" for k, v in sorted(by.items())) if rows else "tickets: none")
    for r in rows:
        if r["status"] != "done":
            reason = f"  {r['reason'][:90]}" if r["reason"] else ""
            print(f"  {r['id']:<10} {r['status']:<16} tier={r['tier']:<7} attempts={r['attempts']} strikes={r['strikes']}{reason}")
    att = db.q("SELECT * FROM attempts")
    if att:
        cost = sum(a["worker_cost"] + a["verifier_cost"] for a in att)
        wcost = sum(a["worker_cost"] for a in att)
        new = sum(a["new_passes"] for a in att if a["merged"])
        print(f"attempts: {len(att)}  merged: {sum(a['merged'] for a in att)}  "
              f"spend: ${cost:.2f} (workers ${wcost:.2f}, verifier ${cost - wcost:.2f})  "
              f"new conformance passes: {new}" + (f"  (${cost / new:.3f} per pass)" if new else ""))
        outcomes = Counter((a["tier"], a["outcome"]) for a in att)
        for (tier, outcome), n in sorted(outcomes.items(), key=lambda x: (x[0][0], str(x[0][1]))):
            print(f"  {tier:<7} {str(outcome):<12} {n}")
    paused = float(db.kv_get("paused_until") or 0)
    if paused > time.time():
        print(f"PAUSED until {time.strftime('%Y-%m-%d %H:%M', time.localtime(paused))}: {db.kv_get('pause_reason')}")
    info = db.kv_get("limits")
    if info:
        info = json.loads(info)
        windows = ", ".join(
            f"{name} {w.get('utilization', 0):.0%} (resets {time.strftime('%a %H:%M', time.localtime(w['resetsAt']))})"
            for name, w in (info.get("unifiedWindows") or {}).items() if w.get("resetsAt")
        )
        print(f"plan usage at last run: {info.get('status')}" + (f"; {windows}" if windows else ""))
    summary = read_json(WORK / "last-full-summary.json")
    if summary:
        print(f"conformance at last merge: {summary['pass']}/{summary['runs']} tests passing")


def cmd_usage(args):
    """One tiny Haiku call to read the plan's current usage windows."""
    cfg = load_config()
    db = DB()
    llm.LIMITS = None
    r = llm.claude("Reply with: ok", model=cfg.model("haiku"), cwd=WORK, tools=[], max_turns=1)
    info = r.get("rate_limit_info") or {}
    if info:
        db.kv_set("limits", json.dumps(info))
    print(f"status: {info.get('status', 'unknown')}")
    for name, w in (info.get("unifiedWindows") or {}).items():
        reset = time.strftime("%a %H:%M", time.localtime(w["resetsAt"])) if w.get("resetsAt") else "?"
        print(f"  {name:<10} {w.get('utilization', 0):>4.0%}  resets {reset}")


def cmd_show(args):
    adir = WORK / "attempts" / str(args.attempt)
    for name in ("prompt.md", "gate.json", "verdict.json", "merge.json"):
        p = adir / name
        if p.exists():
            print(f"===== {name}")
            print(p.read_text()[:6000])
    if (adir / "diff.patch").exists():
        print("===== diff.patch")
        print((adir / "diff.patch").read_text()[:8000])


def cmd_reset(args):
    db = DB()
    fields = {"status": "open", "strikes": 0, "reason": "", "salvage_attempt": None}
    if args.tier:
        fields["tier"] = args.tier
    db.set_ticket(args.ticket, **fields)
    print(f"{args.ticket} reopened")


def cmd_partials(args):
    """Re-judges the partial tickets against a full run (default: the merge
    bot's last one)."""
    from gates import read_results
    results = read_results(Path(args.results) if args.results else WORK / "last-full.jsonl")
    if not results:
        sys.exit("no results to judge against")
    db = DB()
    changed = pipeline.refresh_partials(db, ticket_files.load_all(ticket_files.TICKETS), results)
    print(f"partial tickets: {dict(changed) or 'no status changes'}")


def cmd_triage(args):
    import triage
    triage.main(args)


def cmd_calibrate(args):
    import calibrate
    sys.exit(calibrate.main(args))


def main():
    ap = argparse.ArgumentParser(prog="orch", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--slots", type=int)
    r.add_argument("--branch")
    r.add_argument("--only")
    r.add_argument("--tickets", help="ticket directory (default: tickets/)")
    r.add_argument("--max-attempts", type=int, help="stop starting new attempts after N")
    r.set_defaults(fn=cmd_run)
    sub.add_parser("stop", help="finish in-flight attempts, then exit").set_defaults(fn=cmd_stop)
    sub.add_parser("snapshot", help="commit the run state to the orch-runs branch").set_defaults(fn=cmd_snapshot)
    sub.add_parser("status").set_defaults(fn=cmd_status)
    st = sub.add_parser("stack", help="the stack of PRs (docs/STACKS.md)")
    st.add_argument("action", nargs="?", choices=["status", "retry"], default="status")
    st.set_defaults(fn=cmd_stack)
    ld = sub.add_parser("land", help="put an architect branch on the stack as its own layer")
    ld.add_argument("branch")
    ld.set_defaults(fn=cmd_land)
    sub.add_parser("usage", help="probe the plan's usage windows").set_defaults(fn=cmd_usage)
    s = sub.add_parser("show")
    s.add_argument("attempt", type=int)
    s.set_defaults(fn=cmd_show)
    rs = sub.add_parser("reset")
    rs.add_argument("ticket")
    rs.add_argument("--tier")
    rs.set_defaults(fn=cmd_reset)
    pt = sub.add_parser("partials", help="re-judge partial tickets against a full run")
    pt.add_argument("--results", help="a full run's --json output (default: work/last-full.jsonl)")
    pt.set_defaults(fn=cmd_partials)
    t = sub.add_parser("triage")
    t.add_argument("--fresh", action="store_true", help="run the full suite on the branch first")
    t.add_argument("--top", type=int, default=25)
    t.set_defaults(fn=cmd_triage)
    c = sub.add_parser("calibrate")
    c.add_argument("--model")
    c.add_argument("--only")
    c.add_argument("--repeat", type=int, default=1)
    c.set_defaults(fn=cmd_calibrate)
    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
