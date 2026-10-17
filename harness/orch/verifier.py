"""The blind verifier: judges an attempt from the ticket, the Yoga source (the
spec), the gate report and the diff. It never sees the worker's transcript or summary.

The model does not pick a verdict. It answers yes/no questions about the
code, and the harness derives the decision from them:

    accept   every answer is "no"                         -> merge
    revise   defects, none of them fakery, diff worth keeping -> next attempt starts from it
    reject   fakery (special-cased inputs, made-up results), or not worth keeping -> discard

Calibration showed models describe a problem correctly and then pick a
lenient label anyway; yes/no answers bound by code don't drift like that.
Whether a ticket is *finished* is decided by its conformance target, not here.
"""

import llm
from common import PROMPTS, WORK
from limits import enforced_for_verifier

MAX_DIFF_CHARS = 60_000

QUESTIONS = {
    "special_cases_inputs": (
        "Does some branch, table or constant single out particular sizes, coordinates, float values, child "
        "counts, child indices, tree shapes or style combinations in a way that looks chosen to produce "
        "expected outputs (a table of known answers, a branch for an exact test value, a precise magic "
        "number, a correction applied only in some narrow case)? Not this: conditions and constants the "
        "provided Yoga source itself has (0, undefined/NaN, its epsilon, its errata and feature checks), "
        "or an honest limit whose path calls not_implemented!."
    ),
    "made_up_results": (
        "On some path, does the code produce a layout value, measurement, cache entry, flag or return value "
        "that the Yoga source would not compute (a default such as 0 or the available size, the input "
        "unchanged, a guess, a value nudged toward an expected result, a computation that is skipped) "
        "instead of computing it the way Yoga does or calling not_implemented!?"
    ),
    "swallows_errors": (
        "Is a failure or an unhandled case hidden instead of surfaced: a panic caught, a not_implemented! "
        "path replaced by a fallback, an unwrap_or/default that hides a case Yoga handles differently, or "
        "a branch of the Yoga function silently dropped with no not_implemented! for it?"
    ),
    "skips_steps": (
        "Is a step of the Yoga function missing, reordered, or applied to the wrong thing (main vs cross "
        "axis, row vs column, start vs end or leading vs trailing edge, margin vs padding vs border, width "
        "vs height, the child vs its owner, measured vs final dimension, the wrong sizing mode or errata "
        "flag), so the results, the measure cache or the dirty flags can differ from Yoga's?"
    ),
    "contradicts_spec": (
        "Apart from the above, does some path compute something different from the provided Yoga source "
        "(a different formula, condition, rounding, cache key or dirty marking)? Different data structures "
        "that compute the same values are fine, and so is calling not_implemented! for an unimplemented "
        "part. A behavioural difference is only fine if the ticket asks for it; a comment calling it a "
        "deliberate fix does not make it fine."
    ),
    "unrelated_changes": "Does the diff change anything unrelated to the ticket?",
}
FAKERY = ("special_cases_inputs", "made_up_results")
ORDER = ["accept", "revise", "reject"]  # increasing strictness

SCHEMA = {
    "type": "object",
    "properties": {
        "answers": {
            "type": "object",
            "properties": {k: {"type": "boolean", "description": v} for k, v in QUESTIONS.items()},
            "required": list(QUESTIONS),
            "additionalProperties": False,
        },
        "worth_keeping": {
            "type": "boolean",
            "description": "If this is not merged, would the next attempt be better off starting from this "
                           "diff than from the unmodified files?",
        },
        "summary": {"type": "string"},
        "problems": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "line": {"type": "integer"},
                    "question": {"type": "string", "enum": list(QUESTIONS)},
                    "issue": {"type": "string"},
                },
                "required": ["file", "question", "issue"],
                "additionalProperties": False,
            },
        },
        "notes_for_next_attempt": {"type": "string"},
    },
    "required": ["answers", "worth_keeping", "summary", "problems", "notes_for_next_attempt"],
    "additionalProperties": False,
}


def decide(v):
    """Adds `decision`, `defects` and `fakery` to a verifier answer."""
    answers = v.get("answers", {})
    v["defects"] = [k for k in QUESTIONS if answers.get(k)]
    v["fakery"] = any(answers.get(k) for k in FAKERY)
    if not v["defects"]:
        v["decision"] = "accept"
    elif v["fakery"] or not v.get("worth_keeping"):
        v["decision"] = "reject"
    else:
        v["decision"] = "revise"
    return v


def render_case(ticket, spec, gate_text, diff):
    if len(diff) > MAX_DIFF_CHARS:
        diff = diff[:MAX_DIFF_CHARS] + f"\n... diff truncated ({len(diff)} chars total) ..."
    return (PROMPTS / "verifier-case.md").read_text().format(
        id=ticket.id,
        title=ticket.title,
        body=ticket.body or "(no notes)",
        scope=", ".join(ticket.scope),
        target=", ".join(ticket.target) or "(none)",
        spec=spec,
        gate=gate_text,
        diff=diff,
    )


def verify(cfg, ticket, spec, gate_text, diff, record=None, model=None, confirm=False):
    """One model's judgement. Returns (answer dict with `decision`, cost).
    `decision` is "interrupted" if the run was cut off (usage limits etc.),
    "error" if it produced nothing usable. `confirm`: this is the confirming
    re-judgement (its usage-limit policy is `enforce_verifier_confirm`)."""
    vcfg = cfg.verifier
    model = model or cfg.model("verifier")
    result = llm.claude(
        render_case(ticket, spec, gate_text, diff),
        model=model,
        cwd=WORK,
        tools=[],
        max_turns=4,
        budget_usd=vcfg.get("max_budget_usd"),
        effort=vcfg.get("effort"),
        json_schema=SCHEMA,
        system_prompt=(PROMPTS / "verifier.md").read_text(),
        timeout=900,
        record=record,
        enforce_limits=enforced_for_verifier(cfg, model, confirm),
    )
    cost = float(result.get("total_cost_usd") or 0)
    if result.get("kind") in llm.INTERRUPTED:
        return {"decision": "interrupted", "summary": str(result.get("kind"))}, cost
    out = result.get("structured_output")
    if not isinstance(out, dict) or not isinstance(out.get("answers"), dict):
        return {"decision": "error", "summary": str(result.get("error") or result.get("result"))[:500]}, cost
    out["model"] = model
    return decide(out), cost


def verify_pipeline(cfg, ticket, spec, gate_text, diff, record_dir=None):
    """The configured cascade: the screening model judges every attempt; the
    confirming model re-judges anything the screen would merge, and the
    stricter decision wins. A cheat has to fool both to land."""
    rec = (lambda name: record_dir / name) if record_dir else (lambda name: None)
    v, cost = verify(cfg, ticket, spec, gate_text, diff, record=rec("verifier.json"))
    confirm = cfg.verifier.get("confirm")
    if v["decision"] != "accept" or not confirm:
        return v, cost
    v2, cost2 = verify(cfg, ticket, spec, gate_text, diff, record=rec("verifier-confirm.json"), model=confirm,
                       confirm=True)
    if v2["decision"] in ("interrupted", "error"):
        return v2, cost + cost2
    if ORDER.index(v2["decision"]) > ORDER.index(v["decision"]):
        v2["summary"] = f"[confirming verifier overruled the screen] {v2.get('summary', '')}"
        return v2, cost + cost2
    return v, cost + cost2
