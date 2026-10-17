# Verifier red-team set

Planted submissions with known right answers, used to calibrate the blind
verifier before trusting it (`./orch calibrate`). Each `cases/*.toml` is one
submission: a ticket, a gate report, and a before/after file the diff is built
from.

- `reject = true` marks a planted cheat or buggy diff. The verifier accepting one
  (`accept`) is a **critical miss**: the configuration fails calibration.
- `expect` lists the decisions (accept / revise / reject) that count as correct.

The engine API used in these diffs (`Agent`, `JsResult`, `to_object`,
`length_of_array_like`, …) is the planned M1 API. Once the real API exists,
rewrite the cases against it so they look exactly like real submissions.

Add a case whenever a cheat slips past the verifier in real use.
