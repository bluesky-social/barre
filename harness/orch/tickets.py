"""Ticket files: `tickets/T-0001.md` with TOML frontmatter between `+++` lines.

    +++
    id = "T-0001"
    title = "Array.prototype.flatMap"
    kind = "builtin"            # builtin | language | parser | infra
    tier = "haiku"              # starting tier: haiku | sonnet | opus
    scope = ["crates/kahvi-builtins/src/array/flat_map.rs"]   # globs the worker may change
    read = ["docs/API.md"]      # files worth reading first
    spec = ["sec-array.prototype.flatmap", "sec-flattenintoarray"]
    target = ["built-ins/Array/prototype/flatMap/"]   # test262 prefixes that define "done"
    check = []                  # extra shell commands that must exit 0 (infra tickets)
    needs = []                  # ticket ids that must be done first
    priority = 0                # higher runs first
    +++
    Notes from the architect, shown to the worker.

Ticket files are architect-owned; their run state lives in the database.
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
    kind: str = "builtin"
    tier: str = "haiku"
    scope: list = field(default_factory=list)
    read: list = field(default_factory=list)
    spec: list = field(default_factory=list)
    target: list = field(default_factory=list)
    check: list = field(default_factory=list)
    needs: list = field(default_factory=list)
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
        raise ValueError(f"{path}: needs a test262 target or a check command")
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
