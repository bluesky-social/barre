#!/usr/bin/env python3
"""PreToolUse hook: blocks file edits outside the ticket's scope.

The orchestrator sets BARRE_SCOPE (JSON list of globs, relative to the
worktree) and BARRE_WORKTREE. This is the live guard; the gate re-checks the
final diff, so anything that slips past here is still rejected.
"""

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from policy import PROTECTED, matches  # noqa: E402


def main():
    event = json.load(sys.stdin)
    tool_input = event.get("tool_input") or {}
    path = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not path:
        return 0
    worktree = Path(os.environ["BARRE_WORKTREE"]).resolve()
    scope = json.loads(os.environ.get("BARRE_SCOPE", "[]"))
    target = (worktree / path).resolve() if not os.path.isabs(path) else Path(path).resolve()
    try:
        rel = target.relative_to(worktree).as_posix()
    except ValueError:
        rel = None
    if rel is None or matches(rel, PROTECTED) or not matches(rel, scope):
        print(
            f"Blocked: {path} is outside this ticket's scope ({', '.join(scope)}). "
            "You may only change files in scope. If the ticket cannot be done without "
            "changing other files, stop and end your reply with a line starting `BLOCKED:` "
            "that says exactly what is needed.",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
