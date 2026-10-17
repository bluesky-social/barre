"""Running Claude headlessly (`claude -p`). Works with an API key or a
subscription login, whichever the `claude` CLI is configured with.

Every run goes through the shared `LIMITS` (set by the CLI): it waits out
pauses that apply to it before starting (limits.py: per-tier policy), reports the plan's usage windows afterwards, and
classifies failures so callers can tell "the work failed" from "the
infrastructure stopped us" (`result["kind"]`).
"""

import json
import re
import subprocess
import uuid
from pathlib import Path

from common import write_json

LIMITS = None  # limits.Limits, installed by the CLI

# result["kind"]: None (finished normally), "exhausted" (turn/budget cap: a
# normal end for our purposes), or one of these interruptions, or "error".
INTERRUPTED = {"rate-limited", "overloaded", "auth", "stopped"}

LIMIT_RX = re.compile(r"usage limit|rate.?limit|limit reached|hit your .{0,20}limit|limit will reset|out of (extra )?usage", re.I)
AUTH_RX = re.compile(r"invalid api key|/login|not logged in|authentication_error|oauth token|unauthori[sz]ed", re.I)


def classify(result, rate_info, stderr=""):
    """Returns (kind, resets_at) for a finished run. A run that finished is
    not an interruption even if the plan reported its window exhausted: past
    the plan's windows, requests spill into API billing (limits.py)."""
    subtype = str(result.get("subtype") or "")
    if not result.get("is_error") and subtype in ("", "success"):
        return None, None
    if rate_info and not str(rate_info.get("status") or "allowed").startswith("allowed"):
        return "rate-limited", rate_info.get("resetsAt")
    if subtype.startswith("error_max"):
        return "exhausted", None
    text = " ".join(str(result.get(k) or "") for k in ("result", "error", "terminal_reason")) + " " + stderr[-2000:]
    status = result.get("api_error_status")
    if status == 429 or LIMIT_RX.search(text):
        m = re.search(r"\|(\d{10})\b", text)
        return "rate-limited", int(m.group(1)) if m else (rate_info or {}).get("resetsAt")
    if status in (500, 502, 503, 529) or re.search(r"overloaded", text, re.I):
        return "overloaded", None
    if status in (401, 403) or AUTH_RX.search(text):
        return "auth", None
    return "error", None


def claude(
    prompt,
    *,
    model,
    cwd,
    tools,
    allowed=(),
    max_turns=30,
    budget_usd=None,
    json_schema=None,
    settings=None,
    effort=None,
    system_append=None,
    system_prompt=None,
    add_dirs=(),
    env=None,
    timeout=3600,
    record=None,
    enforce_limits=True,
):
    """Runs one headless session and returns its result event, plus `kind`
    (see above) and `rate_limit_info`. `record` is a path to save it to.
    `enforce_limits`: whether this run waits out a cap pause (limits.py); a
    hard pause holds every run."""
    if LIMITS is not None and not LIMITS.wait(enforce=enforce_limits):
        return {"is_error": True, "kind": "stopped", "error": LIMITS.fatal, "total_cost_usd": 0.0, "num_turns": 0}
    cmd = [
        "claude", "-p",
        "--model", model,
        "--output-format", "stream-json", "--verbose",
        "--max-turns", str(max_turns),
        "--session-id", str(uuid.uuid4()),
        "--no-session-persistence",
        "--strict-mcp-config",
        "--setting-sources", "project",
        "--exclude-dynamic-system-prompt-sections",
        "--permission-mode", "dontAsk",
        "--permission-prompts", "none",
        "--tools", ",".join(tools),
    ]
    if allowed:
        cmd += ["--allowedTools", " ".join(allowed)]
    if budget_usd:
        cmd += ["--max-budget-usd", str(budget_usd)]
    if json_schema:
        cmd += ["--json-schema", json.dumps(json_schema)]
    if settings:
        cmd += ["--settings", json.dumps(settings)]
    if effort:
        cmd += ["--effort", effort]
    if system_prompt:
        cmd += ["--system-prompt", system_prompt]
    if system_append:
        cmd += ["--append-system-prompt", system_append]
    for d in add_dirs:
        cmd += ["--add-dir", str(d)]

    stderr = ""
    result, rate_info = None, None
    try:
        proc = subprocess.run(
            cmd, input=prompt, cwd=cwd, env=env, timeout=timeout, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        stderr = proc.stderr or ""
        for line in proc.stdout.splitlines():
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "rate_limit_event":
                rate_info = event.get("rate_limit_info") or rate_info
            elif event.get("type") == "result":
                result = event
        if result is None:
            result = {"is_error": True, "error": f"exit {proc.returncode}: {proc.stdout[-1500:]}{stderr[-1500:]}"}
    except subprocess.TimeoutExpired:
        result = {"is_error": True, "error": f"timed out after {timeout}s"}

    kind, resets_at = classify(result, rate_info, stderr)
    result["kind"] = kind
    result["rate_limit_info"] = rate_info
    result.setdefault("total_cost_usd", 0.0)
    result.setdefault("num_turns", 0)
    if LIMITS is not None:
        LIMITS.observe(rate_info)
        if kind == "rate-limited":
            LIMITS.hit(resets_at, "usage limit reached")
        elif kind == "overloaded":
            LIMITS.overloaded()
        elif kind == "auth":
            LIMITS.stop("Claude CLI is not authenticated; log in again (`claude` then /login) and rerun")
        elif kind in (None, "exhausted"):
            LIMITS.ok()
    if record:
        slim = {k: v for k, v in result.items() if k not in ("modelUsage", "subagent_stats")}
        write_json(Path(record), slim)
    return result
