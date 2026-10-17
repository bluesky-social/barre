"""Staying inside the account's usage limits.

Every headless Claude run reports the plan's usage windows in a
`rate_limit_event` (utilization and reset time for the 5-hour and 7-day
windows on Pro/Max plans). The orchestrator pauses *before* a window is
exhausted, leaving the rest for the human sharing the plan, and resumes when
it resets. If a run hits a limit anyway, the pause comes from the error.
Pauses are persisted, so a restarted orchestrator keeps honouring them.
"""

import json
import threading
import time

from common import log

MARGIN_SECONDS = 60


class Limits:
    def __init__(self, cfg, db):
        self.cfg = cfg.limits if hasattr(cfg, "limits") else {}
        self.db = db
        self.lock = threading.Lock()
        self.backoff = 0
        self.fatal = None

    # -- state -----------------------------------------------------------

    def paused_until(self):
        return float(self.db.kv_get("paused_until") or 0)

    def reason(self):
        return self.db.kv_get("pause_reason") or ""

    def paused(self):
        return time.time() < self.paused_until()

    def pause(self, until, reason):
        with self.lock:
            until = max(until, time.time() + 60)
            if until > self.paused_until():
                self.db.kv_set("paused_until", str(until))
                self.db.kv_set("pause_reason", reason)
                log(f"pausing until {time.strftime('%Y-%m-%d %H:%M', time.localtime(until))}: {reason}")

    # -- inputs ----------------------------------------------------------

    def observe(self, info):
        """Called with each rate_limit_info a run reports."""
        if not info:
            return
        self.db.kv_set("limits", json.dumps(info))
        status = str(info.get("status") or "allowed")
        if not status.startswith("allowed"):
            self.hit(info.get("resetsAt"), f"usage limit ({info.get('rateLimitType', 'unknown window')}: {status})")
            return
        for name, window in (info.get("unifiedWindows") or {}).items():
            used = window.get("utilization")
            cap = float(self.cfg.get(f"max_{name}", self.cfg.get("max_default", 0.9)))
            if used is not None and used >= cap:
                resets = window.get("resetsAt") or time.time() + self._next_backoff()
                self.pause(float(resets) + MARGIN_SECONDS, f"{name} window at {used:.0%} (swarm cap {cap:.0%})")

    def hit(self, resets_at=None, reason="usage limit"):
        """A run was refused or cut off by a limit."""
        if resets_at:
            self.pause(float(resets_at) + MARGIN_SECONDS, reason)
        else:
            self.pause(time.time() + self._next_backoff(), reason + " (no reset time given; backing off)")

    def overloaded(self):
        self.pause(time.time() + 120, "API overloaded")

    def stop(self, reason):
        """Unrecoverable without a human (e.g. logged out)."""
        self.fatal = reason
        log(f"stopping: {reason}")

    def _next_backoff(self):
        first = float(self.cfg.get("backoff_minutes", 5)) * 60
        self.backoff = min(max(self.backoff * 2, first), 2 * 3600)
        return self.backoff

    def ok(self):
        """A run completed normally: reset the blind backoff."""
        self.backoff = 0

    # -- waiting ---------------------------------------------------------

    def wait(self):
        """Blocks while paused. Returns False if the orchestrator must stop."""
        announced = 0.0
        while not self.fatal:
            remaining = self.paused_until() - time.time()
            if remaining <= 0:
                return True
            if time.time() - announced > 900:
                log(f"paused for {remaining / 60:.0f} more minutes: {self.reason()}")
                announced = time.time()
            time.sleep(min(remaining, 30))
        return False
