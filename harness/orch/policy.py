"""What workers may touch and which added lines are suspicious.

Owned by the architect. Changing this file changes what the swarm can get
away with, so treat edits like changes to the test suite.
"""

import re

# Paths no worker ticket may modify, whatever its scope says.
PROTECTED = [
    "vendor/**",
    "harness/**",
    "conformance/**",
    "tickets/**",
    "docs/**",
    "bench/**",
    ".claude/**",
    ".github/**",
    ".cargo/**",
    "CLAUDE.md",
    "Cargo.toml",
    "Cargo.lock",
    "crates/*/Cargo.toml",
    "**/build.rs",
    "**/tests/**",
    "**/benches/**",
    ".gitmodules",
    ".gitignore",
    "rust-toolchain*",
    "clippy.toml",
    "rustfmt.toml",
    "conf",
    "spec",
    "orch",
]

# The C ABI crate: the only place raw pointers and `unsafe` are expected.
FFI_CRATES = ["crates/barre-ffi/**"]

# Added lines matching these reject the attempt outright:
# (rule id, pattern, reason[, exempt globs]). A ticket can waive a rule by id
# (`allow = ["unsafe"]`); tickets are architect-owned, so that is the
# architect's decision, not the worker's.
REJECT = [
    ("todo", r"\btodo!\s*\(", "todo!() left in code; unfinished parts call not_implemented!(\"name\")"),
    ("unimplemented", r"\bunimplemented!\s*\(", "unimplemented!() left in code; use not_implemented!(\"name\")"),
    ("tests", r"#\s*\[\s*(test\b|cfg\s*\(\s*test|bench\b)|#\s*\[\s*\w+::test\b", "workers may not add tests"),
    ("allow", r"#!?\s*\[\s*(allow|expect)\s*\(|#!?\s*\[\s*cfg_attr\s*\(.*\ballow\s*\(",
     "lint suppression is architect-only"),
    ("unsafe", r"\bunsafe\b(?!\s*\(\s*no_mangle\s*\))", "unsafe is only for the FFI crate", FFI_CRATES),
    ("io", r"\bstd::(fs|env|net|io::std(in|out|err))\b|\bCommand::new\b|\b(option_)?env!\s*\(",
     "engine code must not touch files, the environment, the network or stdio"),
    ("process", r"\bstd::process\b|\bprocess::(exit|abort)\b|\bexit\s*\(\s*-?\d",
     "exiting or aborting the process is architect-only (not_implemented! is the way to stop)"),
    ("print", r"\b(e?print(ln)?|dbg)!\s*\(", "printing from engine code is not allowed"),
    ("embed", r"\binclude(_str|_bytes)?!", "embedding files is not allowed"),
    ("extern-crate", r"\bextern\s+crate\b", "new crate dependencies are architect-only"),
    ("catch-unwind", r"\bcatch_unwind\b|\bpanic::(set_hook|take_hook|resume_unwind)\b|\bAssertUnwindSafe\b",
     "catching panics hides not_implemented! and real bugs"),
    ("global-state", r"\bstatic\s+mut\b|\bthread_local!|\bstatic\s+\w+\s*:\s*[^=]*\b(Atomic\w+|Mutex|RwLock|"
     r"OnceLock|OnceCell|LazyLock|Lazy|Cell|RefCell|UnsafeCell)\b", "global state is architect-only"),
    ("harness", r"(?i:\bgtest\b|googletest|expected-pass|tests/generated)|\bYogaTest\b|\bTEST(_F|_P)?\s*\(|"
     r"\bYG\w+Test\b|\bBARRE_[A-Z_]+", "reference to the test harness in engine code"),
]

# Added lines matching these are shown to the verifier for judgement:
# (pattern, reason) or a callable (text -> reason or None).
FLAG = [
    (r"\bpanic!\s*\(", "panic! aborts the host through the C ABI; only not_implemented! should stop layout"),
    (r"\bunreachable!\s*\(", "unreachable! must be truly unreachable"),
    (r"\bunsafe\b(?!\s*\(\s*no_mangle\s*\))", "unsafe: check the pointer is valid and the C API's ownership rules hold"),
    (r"\.unwrap\(\)|\.expect\(", "unwrap/expect can abort layout: check it cannot fail"),
    (r"\.unwrap_or(_default|_else)?\(|\.ok\(\)|\bunwrap_or_else\b|\.ok_or\b",
     "a default instead of a failure: check Yoga does the same"),
    (r"\b(TODO|FIXME|XXX|HACK)\b", "leftover marker"),
    (r"(?i)\b(for now|simplif\w*|stub\w*|placeholder|temporar\w*|workaround|good enough|hard-?cod\w*|"
     r"approximat\w*|heuristic\w*|fudge|magic)\b", "shortcut language"),
    (r"(?i)not.implemented", "admits missing functionality: fine if honest, check it"),
    (r"(?i)\bskip\w*\b", "skipping something: check what and why"),
    (r"(?i)\b(deliberate\w*|intentional\w*|differs? from yoga|unlike yoga|yoga bug|bug in yoga)\b",
     "claims a deliberate difference from Yoga: the ticket must ask for it"),
    (r"(?:==|!=|<=|>=)\s*-?(?:[2-9]|\d{2,})(?:\.0*)?(?:_?f(?:32|64))?\b(?!\s*[*/+-])",
     "comparison with a specific number: check it is Yoga's condition, not a test's size"),
    (r"\b(len|count|index|idx|i|depth|line_index)\(?\)?\s*(==|!=)\s*[1-9]\d*\b",
     "a specific child count or index: check it is not keyed on a test's tree"),
    (r"^\s*\|?\s*-?\d+\.\d*(?:_?f(?:32|64))?\s*(?:(?:\||\.\.=?)\s*-?\d+(?:\.\d*)?\s*)*(?:if\b.*)?=>",
     "a match arm on a float literal: check it is not a table of test sizes"),
    (r"(==|!=)\s*\"|\"\s*=>|\.(contains|starts_with|ends_with)\(\s*\"",
     "compares against a string: layout code has no reason to look at strings"),
]


def _odd_float(text):
    """A float literal with three or more significant digits (33.333, 146.5,
    0.6667). Layout code needs 0, 1, 0.5 and the like; a precise value is a
    test's expected size more often than not."""
    for m in re.finditer(r"(?<![\w.])(\d+\.\d*|\d*\.\d+)(?:[eE][+-]?\d+)?(?:_?f(?:32|64))?(?![\w.])", text):
        digits = m.group(1).replace(".", "").lstrip("0").rstrip("0")
        if len(digits) >= 3:
            return f"precise float literal {m.group(0)}: Yoga's source has no such constant; is it a test's value?"
    return None


FLAG_FUNCS = [_odd_float]

_REJECT = [(r[0], re.compile(r[1]), r[2], r[3] if len(r) > 3 else []) for r in REJECT]
_FLAG = [(re.compile(p), why) for p, why in FLAG]
RULES = [r[0] for r in REJECT]


def glob_to_regex(glob):
    out = ""
    i = 0
    while i < len(glob):
        if glob.startswith("**/", i):
            out += "(?:.*/)?"
            i += 3
        elif glob.startswith("**", i):
            out += ".*"
            i += 2
        elif glob[i] == "*":
            out += "[^/]*"
            i += 1
        elif glob[i] == "?":
            out += "[^/]"
            i += 1
        else:
            out += re.escape(glob[i])
            i += 1
    return re.compile(out + r"\Z")


def matches(path, globs):
    return any(glob_to_regex(g).match(path) for g in globs)


def added_lines(diff_text):
    """Yields (file, line_number, text) for every added line in a unified diff."""
    file = None
    line_no = 0
    for raw in diff_text.splitlines():
        if raw.startswith("+++ "):
            target = raw[4:].strip()
            file = target[2:] if target.startswith("b/") else None
        elif raw.startswith("@@"):
            m = re.search(r"\+(\d+)", raw)
            line_no = int(m.group(1)) if m else 0
        elif file is None:
            continue
        elif raw.startswith("+"):
            yield file, line_no, raw[1:]
            line_no += 1
        elif raw.startswith(" "):
            line_no += 1


def lint(diff_text, allow=()):
    """Returns (rejects, flags): lists of {file, line, rule, text}. `allow`
    lists REJECT rule ids the ticket waives."""
    rejects, flags = [], []
    for file, line, text in added_lines(diff_text):
        if not file.endswith(".rs"):
            continue
        code = _strip_comment(text)
        for rid, rx, why, exempt in _REJECT:
            # The harness rule also reads comments: a comment naming a test is
            # how a special case usually explains itself.
            subject = text if rid == "harness" else code
            if rid not in allow and rx.search(subject) and not matches(file, exempt):
                rejects.append({"file": file, "line": line, "rule": why, "text": text.strip()[:200]})
        for rx, why in _FLAG:
            if rx.search(text):
                flags.append({"file": file, "line": line, "rule": why, "text": text.strip()[:200]})
        for fn in FLAG_FUNCS:
            why = fn(code)
            if why:
                flags.append({"file": file, "line": line, "rule": why, "text": text.strip()[:200]})
    return rejects, flags


def _strip_comment(text):
    """The code part of a line: drops a `//` comment (not inside a string)."""
    in_str = False
    for i, c in enumerate(text):
        if c == '"' and (i == 0 or text[i - 1] != "\\"):
            in_str = not in_str
        elif not in_str and text.startswith("//", i):
            return text[:i]
    return text
