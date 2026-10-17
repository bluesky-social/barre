"""Ticket files: `tickets/T-0001.md` with TOML frontmatter between `+++` lines
(tickets/README.md has the full format and an example).

    +++
    id = "T-0001"
    title = "calculate_baseline"
    kind = "algorithm"          # algorithm | style | api | node | infra
    tier = "haiku"              # starting tier: haiku | sonnet | opus
    scope = ["crates/barre/src/algorithm/baseline.rs"]   # globs the worker may change
    read = ["docs/API.md"]      # files worth reading first
    spec = ["calculateBaseline", "isBaselineLayout"]     # Yoga names, printed with ./spec --refs
    target = ["generated/YGAlignItemsTest/"]  # conformance id prefixes that define "done"
    check = []                  # extra shell commands that must exit 0 (infra tickets)
    needs = []                  # ticket ids that must be done first
    allow = []                  # policy.py REJECT rule ids this ticket waives (e.g. "unsafe")
    priority = 0                # higher runs first
    +++
    Notes from the architect, shown to the worker.

Ticket files are architect-owned; their run state lives in the database.
Only `tickets/*.md` are loaded (not README.md, not subdirectories).
"""

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from common import ROOT

TICKETS = ROOT / "tickets"


@dataclass
class Ticket:
    id: str
    title: str
    kind: str = "algorithm"
    tier: str = "haiku"
    scope: list = field(default_factory=list)
    read: list = field(default_factory=list)
    spec: list = field(default_factory=list)
    target: list = field(default_factory=list)
    check: list = field(default_factory=list)
    needs: list = field(default_factory=list)
    allow: list = field(default_factory=list)
    priority: int = 0
    body: str = ""
    path: str = ""


def parse(text, path=""):
    if not text.startswith("+++"):
        raise ValueError(f"{path}: ticket must start with +++ frontmatter")
    end = text.find("\n+++", 3)
    if end < 0:
        raise ValueError(f"{path}: unterminated frontmatter")
    meta = tomllib.loads(text[3:end])
    body = text[end + 4 :].strip()
    known = set(Ticket.__dataclass_fields__) - {"body", "path"}
    unknown = set(meta) - known
    if unknown:
        raise ValueError(f"{path}: unknown keys {sorted(unknown)}")
    t = Ticket(**meta, body=body, path=str(path))
    if not t.scope:
        raise ValueError(f"{path}: scope must list the files the worker may change")
    if not t.target and not t.check:
        raise ValueError(f"{path}: needs a conformance target or a check command")
    from policy import RULES
    bad = [a for a in t.allow if a not in RULES]
    if bad:
        raise ValueError(f"{path}: allow lists unknown lint rules {bad} (known: {', '.join(RULES)})")
    return t


def load_all(directory=TICKETS):
    out = {}
    for p in sorted(Path(directory).glob("*.md")):
        if p.name.upper() == "README.MD":
            continue
        t = parse(p.read_text(), p)
        if t.id in out:
            raise ValueError(f"duplicate ticket id {t.id}")
        out[t.id] = t
    return out
