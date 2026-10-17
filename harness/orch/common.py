"""Paths, configuration and small subprocess/git helpers."""

import json
import os
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# Run state, worktrees and build caches. BARRE_WORK lets a second
# orchestrator (or a smoke test) run without touching the main one's state.
WORK = Path(os.environ.get("BARRE_WORK") or ROOT / "work").resolve()
PROMPTS = ROOT / "harness" / "prompts"


@dataclass
class Config:
    branch: str = "main"
    slots: int = 2
    push: bool = False
    remote: str = "origin"
    models: dict = field(default_factory=dict)
    worker: dict = field(default_factory=dict)
    verifier: dict = field(default_factory=dict)
    escalation: dict = field(default_factory=dict)
    limits: dict = field(default_factory=dict)  # usage-window caps (limits.py)
    gate: dict = field(default_factory=dict)    # cargo commands the gates and the merge bot run
    conf: dict = field(default_factory=dict)    # the conformance runner (./conf)
    stack: dict = field(default_factory=dict)  # set repo to land through stacked PRs (docs/STACKS.md)

    def model(self, tier):
        return self.models[tier]


def load_config(path=None):
    path = Path(path) if path else ROOT / "harness" / "orch.toml"
    with open(path, "rb") as f:
        data = tomllib.load(f)
    cfg = Config(**{k: v for k, v in data.items() if k in Config.__dataclass_fields__})
    local = WORK / "orch.local.toml"
    if local.exists():
        with open(local, "rb") as f:
            for k, v in tomllib.load(f).items():
                if isinstance(v, dict) and isinstance(getattr(cfg, k, None), dict):
                    getattr(cfg, k).update(v)
                else:
                    setattr(cfg, k, v)
    return cfg


def log(msg):
    stamp = time.strftime("%H:%M:%S")
    print(f"[{stamp}] {msg}", file=sys.stderr, flush=True)


def run(cmd, cwd=ROOT, check=True, env=None, timeout=None, capture=True):
    """Runs a command; returns CompletedProcess with text stdout/stderr."""
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        timeout=timeout,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(map(str, cmd))}\n{proc.stdout or ''}{proc.stderr or ''}"
        )
    return proc


def git(*args, cwd=ROOT, check=True):
    return run(["git", *args], cwd=cwd, check=check).stdout.strip()


def write_json(path, data):
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_text(json.dumps(data, indent=2) + "\n")


def read_json(path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return default


def tail(text, limit=4000):
    text = text or ""
    return text if len(text) <= limit else "…" + text[-limit:]
