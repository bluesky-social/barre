"""One worker attempt: reset a slot worktree, run a headless Claude on the
ticket, and commit whatever it changed. Nothing here trusts the worker; the
gates and the verifier decide what the attempt was worth."""

import json
import os
import re
import sys
from dataclasses import dataclass

import llm
from common import PROMPTS, ROOT, WORK, git, run
from conformance import BASELINE, read_ids
from policy import matches

SLOTS = WORK / "slots"
ATTEMPTS = WORK / "attempts"

WORKER_TOOLS = ["Read", "Edit", "Write", "Glob", "Grep", "Bash"]
WORKER_COMMANDS = [
    "cargo build", "cargo check", "cargo clippy", "cargo fmt", "./conf", "./spec", "git diff", "git status",
    "grep", "head", "tail", "wc", "echo",
]
WORKER_ALLOWED = ["Read", "Edit", "Write", "Glob", "Grep"] + [
    f"Bash({c}{suffix})" for c in WORKER_COMMANDS for suffix in ("", " *")
]


@dataclass
class Attempt:
    id: int
    slot_dir: str
    base: str
    head: str | None
    cost: float
    turns: int
    blocked: str | None
    error: str | None
    dir: str
    kind: str | None = None  # llm result kind: None, "exhausted", an interruption, or "error"


def slot_dir(i):
    return SLOTS / f"slot-{i}"


def prepare_slot(i, base):
    path = slot_dir(i)
    if not (path / ".git").exists():
        SLOTS.mkdir(parents=True, exist_ok=True)
        git("worktree", "add", "--detach", str(path), base)
    git("checkout", "--detach", "--force", base, cwd=path)
    git("clean", "-fdx", "-e", "target", cwd=path)
    return path


def spec_text(names, limit=14000):
    """The Yoga source for the ticket's `spec` names (./spec --refs: each
    definition plus the short functions it calls)."""
    parts = []
    for name in names:
        out = run([sys.executable, str(ROOT / "harness" / "spec.py"), "--refs", name], check=False).stdout
        parts.append(out[:limit] + ("\n// ... truncated ..." if len(out) > limit else "")
                     if out.strip() else f"(no Yoga definition named {name})")
    return "\n\n".join(parts) or "(none given)"


def baseline_ids():
    return read_ids(ROOT / BASELINE)


def target_pass_count(targets, ids=None):
    ids = baseline_ids() if ids is None else ids
    return sum(1 for i in ids if any(i.startswith(t) for t in targets))


def bullet(items, empty="(none)"):
    return "\n".join(f"- `{x}`" for x in items) if items else empty


def render_prompt(ticket, notes, salvaged):
    done = []
    if ticket.target:
        done.append(
            f"Every conformance test under {', '.join(ticket.target)} passes "
            f"(check with `./conf {' '.join(ticket.target)}`). "
            f"{target_pass_count(ticket.target)} pass on the main branch right now."
        )
    for c in ticket.check:
        done.append(f"This command exits 0 when run from the repository root: `{c}`")
    history = ""
    if notes or salvaged:
        history = "\n## Previous attempts\n\n"
        if salvaged:
            history += (
                "Your worktree already contains a previous attempt's changes (not merged). "
                "Start from them and fix what the reviewer pointed out.\n\n"
            )
        for n in notes:
            history += f"- attempt {n['attempt']}: {n['text']}\n"
    template = (PROMPTS / "worker.md").read_text()
    return template.format(
        id=ticket.id,
        title=ticket.title,
        body=ticket.body or "(no extra notes)",
        scope=bullet(ticket.scope),
        read=bullet(ticket.read),
        spec=spec_text(ticket.spec),
        done="\n".join(f"- {d}" for d in done),
        history=history,
    )


def blocked_reason(text):
    m = re.search(r"^\s*BLOCKED:\s*(.+)$", text or "", re.M)
    return m.group(1).strip() if m else None


def attempt(cfg, db, ticket, slot, tier, salvage_attempt=None):
    base = git("rev-parse", cfg.branch)
    path = prepare_slot(slot, base)
    model = cfg.model(tier)
    aid = db.new_attempt(ticket.id, slot, tier, model, base)
    adir = ATTEMPTS / str(aid)
    adir.mkdir(parents=True, exist_ok=True)

    salvaged = False
    if salvage_attempt:
        patch = ATTEMPTS / str(salvage_attempt) / "diff.patch"
        if patch.exists() and run(["git", "apply", "--3way", str(patch)], cwd=path, check=False).returncode == 0:
            salvaged = True
        else:
            git("checkout", "--force", base, cwd=path)
            git("clean", "-fdx", "-e", "target", cwd=path)

    prompt = render_prompt(ticket, db.notes(ticket.id), salvaged)
    (adir / "prompt.md").write_text(prompt)
    env = dict(os.environ)
    env.update(
        BARRE_SCOPE=json.dumps(ticket.scope),
        BARRE_WORKTREE=str(path),
        BARRE_VENDOR=str(ROOT / "vendor"),
    )
    hook = f"{sys.executable} {ROOT / 'harness' / 'orch' / 'hooks' / 'scope_guard.py'}"
    settings = {
        "hooks": {
            "PreToolUse": [
                {"matcher": "Edit|Write|MultiEdit|NotebookEdit", "hooks": [{"type": "command", "command": hook}]}
            ]
        }
    }
    wcfg = cfg.worker
    result = llm.claude(
        prompt,
        model=model,
        cwd=path,
        tools=WORKER_TOOLS,
        allowed=WORKER_ALLOWED,
        max_turns=wcfg.get("max_turns", 60),
        budget_usd=wcfg.get("max_budget_usd"),
        effort=wcfg.get("effort"),
        settings=settings,
        system_append=(PROMPTS / "worker-system.md").read_text(),
        add_dirs=[ROOT / "vendor"],
        env=env,
        timeout=int(wcfg.get("timeout_minutes", 40)) * 60,
        record=adir / "worker.json",
    )
    cost = float(result.get("total_cost_usd") or 0)
    turns = int(result.get("num_turns") or 0)
    kind = result.get("kind")
    error = str(result.get("error") or result.get("result") or "unknown error") if kind == "error" else None
    blocked = blocked_reason(result.get("result", "")) if kind in (None, "exhausted") else None

    # Format only what the worker touched (and may touch): formatting anything
    # else would put files outside the scope into the diff.
    touched = run(["git", "status", "--porcelain", "--untracked-files=all"], cwd=path).stdout.splitlines()
    rust_files = [l[3:] for l in touched if l[3:].endswith(".rs") and matches(l[3:], ticket.scope)]
    if rust_files:
        run(["rustfmt", "--edition", "2024", *rust_files], cwd=path, check=False)
    git("add", "-A", cwd=path)
    head = None
    if run(["git", "diff", "--cached", "--quiet"], cwd=path, check=False).returncode != 0:
        run(
            ["git", "-c", "user.name=barre-worker", "-c", "user.email=worker@barre.invalid",
             "commit", "-q", "--no-verify", "-m", f"{ticket.id} attempt {aid}"],
            cwd=path,
        )
        head = git("rev-parse", "HEAD", cwd=path)
        # Keep the commit reachable: later stages (and salvage) may need it after the slot moves on.
        git("update-ref", f"refs/barre/{db.instance()}/attempts/{aid}", head, cwd=path)
        (adir / "diff.patch").write_text(run(["git", "diff", base, head], cwd=path).stdout)
    db.set_attempt(aid, head=head, worker_cost=cost, turns=turns)
    return Attempt(aid, str(path), base, head, cost, turns, blocked, error, str(adir), kind)
