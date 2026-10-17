"""Staying inside the account's usage limits.

Every headless Claude run reports the plan's usage windows in a
`rate_limit_event` (utilization and reset time for the 5-hour and 7-day
windows on Pro/Max plans). Two kinds of pause come out of that:

- **Cap pause** (proactive). A window passes its swarm cap in `[limits]`
  (or the plan reports it exhausted). Only the *enforced* work waits:
  worker attempts at tiers in `enforce_tiers`, the screening verifier if its
  model is one of those tiers, and the confirming verifier if
  `enforce_verifier_confirm`. Everything else keeps going: requests past the
  plan's windows spill into API billing (through agentgateway), which is
  fine for cheap work. The claim loop skips tickets at paused tiers and keeps
  handing out the rest.
- **Hard pause** (an actual rate-limit or overload error, or a limit hit with
  no reset time). Nothing starts until it ends, with backoff, as in kahvi.

Both are persisted (work/state.db), so a restarted orchestrator keeps
honouring them. With `enforce_tiers` unset every tier is enforced, which is
kahvi's behaviour.
"""

import json
import threading
import time

from common import log

MARGIN_SECONDS = 60
ALL_TIERS = ("haiku", "sonnet", "opus")


def enforce_tiers(cfg):
    limits = cfg.limits if hasattr(cfg, "limits") else {}
    return set(limits.get("enforce_tiers", ALL_TIERS))


def tier_of_model(cfg, model):
    """The tier name (haiku/sonnet/opus) whose configured model is `model`."""
    for tier, m in cfg.models.items():
        if m == model and tier != "verifier":
            return tier
    return None


def enforced_for_worker(cfg, tier):
    return tier in enforce_tiers(cfg)


def enforced_for_verifier(cfg, model, confirm):
    """The confirming verifier follows `enforce_verifier_confirm`; the screen
    follows its model's tier (an unknown model is enforced)."""
    if confirm:
        return bool((cfg.limits if hasattr(cfg, "limits") else {}).get("enforce_verifier_confirm", True))
    tier = tier_of_model(cfg, model)
    return tier is None or tier in enforce_tiers(cfg)


class Limits:
    def __init__(self, cfg, db, caps=True):
        """`caps=False`: never wait out a cap pause (one-off commands a human
        starts, such as calibration); hard pauses still apply."""
        self.caps = caps
        self.cfg = cfg.limits if hasattr(cfg, "limits") else {}
        self.tiers = enforce_tiers(cfg)
        self.enforce_confirm = bool(self.cfg.get("enforce_verifier_confirm", True))
        self.db = db
        self.lock = threading.Lock()
        self.backoff = 0
        self.fatal = None

    # -- state -----------------------------------------------------------

    def _until(self, key):
        return float(self.db.kv_get(key) or 0)

    def paused_until(self):
        """End of the hard pause (nothing runs)."""
        return self._until("paused_until")

    def capped_until(self):
        """End of the cap pause (enforced work waits)."""
        return self._until("cap_paused_until")

    def reason(self):
        return self.db.kv_get("pause_reason") or ""

    def cap_reason(self):
        return self.db.kv_get("cap_pause_reason") or ""

    def blocked(self):
        """A hard pause: nothing starts."""
        return time.time() < self.paused_until()

    def capped(self):
        """A cap pause: enforced tiers wait."""
        return time.time() < self.capped_until()

    def paused(self):
        """Kept for callers that only care whether anything at all is held back."""
        return self.blocked()

    def paused_tiers(self):
        """Tiers whose worker attempts must not start now."""
        if self.blocked():
            return set(ALL_TIERS)
        return set(self.tiers) if self.capped() else set()

    def _set(self, prefix, until, reason):
        with self.lock:
            until = max(until, time.time() + 60)
            if until > self._until(f"{prefix}paused_until"):
                self.db.kv_set(f"{prefix}paused_until", str(until))
                self.db.kv_set(f"{prefix}pause_reason", reason)
                who = "everything" if not prefix else ", ".join(sorted(self.tiers)) + \
                    (" and the verifier confirm" if self.enforce_confirm else "")
                log(f"pausing {who} until {time.strftime('%Y-%m-%d %H:%M', time.localtime(until))}: {reason}")

    def pause(self, until, reason):
        """A hard pause."""
        self._set("", until, reason)

    def cap(self, until, reason):
        """A cap pause: only enforced work waits."""
        if not self.tiers and not self.enforce_confirm:
            return
        self._set("cap_", until, reason)

    # -- inputs ----------------------------------------------------------

    def observe(self, info):
        """Called with each rate_limit_info a run reports."""
        if not info:
            return
        self.db.kv_set("limits", json.dumps(info))
        status = str(info.get("status") or "allowed")
        if not status.startswith("allowed"):
            # The plan's window is exhausted. Work that spills into API
            # billing goes on; enforced work waits for the reset.
            resets = info.get("resetsAt") or time.time() + self._next_backoff()
            self.cap(float(resets) + MARGIN_SECONDS,
                     f"usage limit ({info.get('rateLimitType', 'unknown window')}: {status})")
            return
        for name, window in (info.get("unifiedWindows") or {}).items():
            used = window.get("utilization")
            cap = float(self.cfg.get(f"max_{name}", self.cfg.get("max_default", 0.9)))
            if used is not None and used >= cap:
                resets = window.get("resetsAt") or time.time() + self._next_backoff()
                self.cap(float(resets) + MARGIN_SECONDS, f"{name} window at {used:.0%} (swarm cap {cap:.0%})")

    def hit(self, resets_at=None, reason="usage limit"):
        """A run was refused or cut off by a limit: a hard pause."""
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

    def remaining(self, enforce=True):
        """Seconds until this kind of work may start (0 = now)."""
        ends = [self.paused_until()] + ([self.capped_until()] if enforce and self.caps else [])
        return max(0.0, max(ends) - time.time())

    def wait(self, enforce=True):
        """Blocks while paused for this kind of work: always during a hard
        pause, and during a cap pause if `enforce`. Returns False if the
        orchestrator must stop."""
        announced = 0.0
        while not self.fatal:
            remaining = self.remaining(enforce)
            if remaining <= 0:
                return True
            if time.time() - announced > 900:
                reason = self.reason() if self.blocked() else self.cap_reason()
                log(f"paused for {remaining / 60:.0f} more minutes: {reason}")
                announced = time.time()
            time.sleep(min(remaining, 30))
        return False
