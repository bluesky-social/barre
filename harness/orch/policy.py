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
    ".claude/**",
    ".github/**",
    ".cargo/**",
    "CLAUDE.md",
    "Cargo.toml",
    "Cargo.lock",
    "crates/*/Cargo.toml",
    "**/build.rs",
    ".gitmodules",
    ".gitignore",
    "rust-toolchain*",
    "clippy.toml",
    "rustfmt.toml",
    "t262",
    "spec",
    "orch",
    "e2e",
]

# Crates allowed to talk to the outside world and to know about test262.
HOST_CRATES = ["crates/kahvi-cli/**", "crates/kahvi-host/**", "crates/kahvi-runtime/**"]

# Added lines matching these reject the attempt outright, except in the
# exempted paths: (pattern, reason[, exempt globs]).
REJECT = [
    (r"\btodo!\s*\(", "todo!() left in code"),
    (r"\bunimplemented!\s*\(", "unimplemented!() left in code"),
    (r"#\s*\[\s*(test\b|cfg\s*\(\s*test)", "workers may not add tests"),
    (r"#!?\s*\[\s*allow\s*\(", "lint suppression is architect-only"),
    (r"\bunsafe\b", "unsafe is architect-only"),
    (r"\bstd::(process|fs|env|net)\b|\bCommand::new\b", "engine code must not touch processes, files, env or network",
     HOST_CRATES),
    (r"\binclude_(str|bytes)!", "embedding files is not allowed"),
    (r"\bextern\s+crate\b", "new crate dependencies are architect-only"),
    (r"(?i:test262)|\$262|AsyncTestComplete|\$DONE|\$DONOTEVALUATE|KAHVI-T262|assert\.sameValue|_FIXTURE",
     "reference to the test harness in engine code", HOST_CRATES),
]

# Added lines matching these are shown to the verifier for judgement.
FLAG = [
    (r"\bpanic!\s*\(", "panic! crashes the engine; errors should be JS exceptions"),
    (r"\bunreachable!\s*\(", "unreachable! must be truly unreachable"),
    (r"\.unwrap\(\)|\.expect\(", "unwrap/expect can crash the engine"),
    (r"\b(TODO|FIXME|XXX|HACK)\b", "leftover marker"),
    (r"(?i)\b(for now|simplif\w*|stub\w*|placeholder|temporar\w*|workaround|good enough|hard-?cod\w*)\b",
     "shortcut language"),
    (r"(?i)not implemented", "admits missing functionality: fine if honest, check it"),
    (r"(?i)\bskip\w*\b", "skipping something: check what and why"),
]

_REJECT = [(re.compile(r[0]), r[1], r[2] if len(r) > 2 else []) for r in REJECT]
_FLAG = [(re.compile(p), why) for p, why in FLAG]


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


def lint(diff_text):
    """Returns (rejects, flags): lists of {file, line, rule, text}."""
    rejects, flags = [], []
    for file, line, text in added_lines(diff_text):
        if not file.endswith(".rs"):
            continue
        for rx, why, exempt in _REJECT:
            if rx.search(text) and not matches(file, exempt):
                rejects.append({"file": file, "line": line, "rule": why, "text": text.strip()[:200]})
        for rx, why in _FLAG:
            if rx.search(text):
                flags.append({"file": file, "line": line, "rule": why, "text": text.strip()[:200]})
    return rejects, flags
