"""The blind verifier: judges an attempt from the ticket, the spec, the gate
report and the diff. It never sees the worker's transcript or summary.

The model does not pick a verdict. It answers yes/no questions about the
code, and the harness derives the decision from them:

    accept   every answer is "no"                         -> merge
    revise   defects, none of them fakery, diff worth keeping -> next attempt starts from it
    reject   fakery (special-cased inputs, made-up results), or not worth keeping -> discard

Calibration showed models describe a problem correctly and then pick a
lenient label anyway; yes/no answers bound by code don't drift like that.
Whether a ticket is *finished* is decided by its test262 target, not here.
"""

import llm
from common import PROMPTS, WORK

MAX_DIFF_CHARS = 60_000

QUESTIONS = {
    "special_cases_inputs": (
        "Does some branch, table or constant single out particular input values, strings, names or lengths "
        "in a way that looks chosen to produce expected outputs (a table of known answers, a branch for "
        "exact test values)? Not this: values the spec itself singles out, or an honest implementation "
        "limit whose path throws a not-implemented error."
    ),
    "made_up_results": (
        "On some path, does the code return or produce something the spec algorithm would not (a default, "
        "the input unchanged, a guess, a shortcut answer) instead of computing it or throwing a "
        "not-implemented error?"
    ),
    "swallows_errors": (
        "Is some abrupt completion that the spec propagates with ? ignored, replaced by a default, or "
        "turned into a different error?"
    ),
    "skips_observable_steps": (
        "Is a spec step with observable effects (property access, calls, conversions) missing, reordered, "
        "or replaced by direct access to internals?"
    ),
    "contradicts_spec": (
        "Apart from the above, does some path behave differently from the spec (wrong value, wrong error "
        "type, wrong edge case)? Throwing the engine's not-implemented error for an unimplemented part is "
        "not a contradiction."
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


def verify(cfg, ticket, spec, gate_text, diff, record=None, model=None):
    """One model's judgement. Returns (answer dict with `decision`, cost).
    `decision` is "interrupted" if the run was cut off (usage limits etc.),
    "error" if it produced nothing usable."""
    vcfg = cfg.verifier
    result = llm.claude(
        render_case(ticket, spec, gate_text, diff),
        model=model or cfg.model("verifier"),
        cwd=WORK,
        tools=[],
        max_turns=4,
        budget_usd=vcfg.get("max_budget_usd"),
        effort=vcfg.get("effort"),
        json_schema=SCHEMA,
        system_prompt=(PROMPTS / "verifier.md").read_text(),
        timeout=900,
        record=record,
    )
    cost = float(result.get("total_cost_usd") or 0)
    if result.get("kind") in llm.INTERRUPTED:
        return {"decision": "interrupted", "summary": str(result.get("kind"))}, cost
    out = result.get("structured_output")
    if not isinstance(out, dict) or not isinstance(out.get("answers"), dict):
        return {"decision": "error", "summary": str(result.get("error") or result.get("result"))[:500]}, cost
    out["model"] = model or cfg.model("verifier")
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
    v2, cost2 = verify(cfg, ticket, spec, gate_text, diff, record=rec("verifier-confirm.json"), model=confirm)
    if v2["decision"] in ("interrupted", "error"):
        return v2, cost + cost2
    if ORDER.index(v2["decision"]) > ORDER.index(v["decision"]):
        v2["summary"] = f"[confirming verifier overruled the screen] {v2.get('summary', '')}"
        return v2, cost + cost2
    return v, cost + cost2
