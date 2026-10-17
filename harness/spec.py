#!/usr/bin/env python3
"""Look up definitions in Yoga's C++ source (vendor/yoga/yoga/), barre's spec.

  ./spec computeFlexBasisForChild          print a function's definition, with file:line
  ./spec Node::resolveFlexGrow             a member function (out-of-line or inline in a class)
  ./spec compute_flex_basis_for_child      snake_case works too (barre's names)
  ./spec Node                              a class/struct/enum: the whole definition
  ./spec --refs calculateLayoutImpl        also print short (< 60 lines) functions it calls
  ./spec --find baseline                   list definitions whose name contains the text
  ./spec --find-text "isBaselineLayout"    list definitions whose body contains the text

The extractor is a brace matcher, not a C++ parser: it strips comments,
strings and preprocessor lines, tracks namespace/class scopes, and treats a
`{` whose statement head has a parameter list as a function body. Every
function, constructor, operator, class, struct, union, enum and `using`
alias with a body in vendor/yoga/yoga/ is indexed. Overloads all print.
The comment block directly above a definition is printed with it.
"""

import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# Worker worktrees don't check out submodules; the orchestrator points this
# at the main checkout's vendor directory.
YOGA = Path(os.environ.get("BARRE_VENDOR") or ROOT / "vendor") / "yoga" / "yoga"

REFS_MAX_LINES = 60      # only functions shorter than this are appended by --refs
REFS_MAX = 12            # at most this many referenced definitions
REFS_MAX_CHARS = 9000    # and at most this much text

IDENT = r"[A-Za-z_]\w*"
KEYWORDS = {
    "if", "for", "while", "switch", "return", "sizeof", "alignof", "decltype", "static_assert",
    "catch", "noexcept", "requires", "new", "delete", "throw", "co_return", "co_await",
}


@dataclass
class Def:
    name: str            # qualified by enclosing classes, never by namespaces: Node::resolveFlexGrow
    kind: str            # function | class | struct | union | enum | using | enum-decl
    file: str            # relative to the repository root
    line: int            # first line of the definition (not its comment)
    start: int           # offsets into the file's text, including the leading comment
    end: int
    body_start: int = 0  # offset of the first character after the head (for --refs)
    text: str = ""
    calls: list = field(default_factory=list)

    @property
    def short(self):
        return self.name.rsplit("::", 1)[-1]

    @property
    def lines(self):
        return self.text.count("\n") + 1


# -- cleaning ----------------------------------------------------------------

def clean(src):
    """The source with comments, string/char literals and preprocessor lines
    replaced by spaces (newlines kept), so offsets and line numbers match."""
    out = list(src)
    i, n = 0, len(src)
    at_line_start = True

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if c == "\n":
            at_line_start = True
            i += 1
            continue
        if at_line_start and c in " \t":
            i += 1
            continue
        if at_line_start and c == "#":
            j = i
            while j < n:
                if src[j] == "\n" and src[j - 1] != "\\":
                    break
                j += 1
            blank(i, j)
            i = j
            continue
        at_line_start = False
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            blank(i, j)
            i = j
        elif c == 'R' and src.startswith('R"', i) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'R"([^(]*)\(', src[i:])
            if m:
                close = ")" + m.group(1) + '"'
                j = src.find(close, i + m.end())
                j = n if j < 0 else j + len(close)
                blank(i + 1, j)
                i = j
            else:
                i += 1
        elif c in "\"'":
            # A digit separator (1'000) is not a char literal.
            if c == "'" and i > 0 and src[i - 1].isalnum() and i + 1 < n and src[i + 1].isalnum() \
                    and not src[i - 1].isalpha():
                i += 1
                continue
            j = i + 1
            while j < n and src[j] != c and src[j] != "\n":
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, min(j, n))
            i = j + 1
        else:
            i += 1
    return "".join(out)


def match_brace(text, i):
    """Index just past the brace matching the `{` at i."""
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def top_level_paren(head):
    """Index of the first `(` in head not nested in <...> template brackets
    or [...] (attributes, lambda captures)."""
    angle = square = 0
    for k, c in enumerate(head):
        if c == "[":
            square += 1
        elif c == "]":
            square -= 1
        elif square:
            continue
        elif c == "<" and not head[k - 1:k + 1] == "<<" and not head[k:k + 2] == "<<":
            angle += 1
        elif c == ">" and angle and head[k - 1] != "-":
            angle -= 1
        elif c == "(" and angle == 0:
            return k
    return -1


def function_name(head):
    """The (possibly qualified) declarator name of a function head, or None."""
    p = top_level_paren(head)
    if p < 0:
        return None
    before = head[:p].rstrip()
    m = re.search(r"((?:" + IDENT + r"\s*(?:<[^<>()]*>)?\s*::\s*)*)(operator\s*(?:\(\)|[^\w\s(]+|" + IDENT
                  + r")|~?" + IDENT + r")$", before)
    if not m:
        return None
    last = re.sub(r"\s+", "", m.group(2))
    if re.match(r"operator\w", last):
        last = "operator " + last[len("operator"):]  # a conversion: `operator YGValue`
    if last in KEYWORDS:
        return None
    quals = [re.sub(r"<.*", "", q).strip() for q in m.group(1).split("::") if q.strip()]
    rest = head[:p][:m.start()].strip()
    # `x = foo(...)` or `return foo(...)`: an expression, not a declarator.
    if re.search(r"[=,]\s*$", rest) or rest.endswith("return"):
        return None
    return quals, last


def enclosing_head(text, i, floor):
    """The statement head ending at the `{` at i: everything back to the last
    `;`, `{` or `}` at or after floor."""
    j = i - 1
    while j >= floor and text[j] not in ";{}":
        j -= 1
    return j + 1


# -- indexing ----------------------------------------------------------------

def leading_comment_start(src, start):
    """Start of the comment block (and attributes) directly above `start`."""
    lines_before = src[:start].split("\n")
    # lines_before[-1] is the indentation of the head's own line.
    k = len(lines_before) - 1
    begin = start - len(lines_before[-1])
    in_block = False
    while k > 0:
        line = lines_before[k - 1].strip()
        if in_block:
            if line.startswith("/*"):
                in_block = False
            k -= 1
            continue
        if line.startswith("//") or line.startswith("*") or line.startswith("/*"):
            if line.endswith("*/") and not line.startswith("/*"):
                in_block = True
            k -= 1
            continue
        if line.endswith("*/"):
            in_block = not line.startswith("/*")
            k -= 1
            continue
        if line.startswith("template"):
            k -= 1
            continue
        break
    return sum(len(l) + 1 for l in lines_before[:k]) if k < len(lines_before) - 1 else begin


def index_file(path, rel):
    src = path.read_text(errors="replace")
    text = clean(src)
    defs = []
    # Scope stack entries: (kind, name, close_offset). kind is ns | class | fn | other.
    scopes = []
    i, n = 0, len(text)
    stmt_start = 0

    def class_prefix():
        return [name for kind, name, _ in scopes if kind == "class" and name]

    def line_of(off):
        return src.count("\n", 0, off) + 1

    while i < n:
        while scopes and i >= scopes[-1][2]:
            scopes.pop()
        c = text[i]
        if c in ";}":
            i += 1
            stmt_start = i
            continue
        if c != "{":
            i += 1
            continue
        inner = scopes[-1][0] if scopes else "ns"
        head_start = stmt_start
        head = text[head_start:i]
        flat = " ".join(head.split())
        close = match_brace(text, i)
        if inner not in ("ns", "class"):
            # Inside a function or an initialiser: nothing to index.
            i = close
            stmt_start = i
            continue
        m = re.match(r"^(?:template\s*<.*>\s*)?(?:typedef\s+)?(class|struct|union|enum(?:\s+class|\s+struct)?)\b(.*)$",
                     flat)
        if re.match(r"^(?:inline\s+)?namespace\b", flat) or re.match(r"^extern\s*$", flat):
            name = flat.split()[-1] if re.match(r"^(?:inline\s+)?namespace\s+\S", flat) else ""
            scopes.append(("ns", name, close))
            i += 1
            stmt_start = i
            continue
        if m and "(" not in m.group(2).split(":")[0]:
            kind = m.group(1).split()[0]
            words = re.findall(IDENT + r"(?:\s*<[^{]*?>)?", m.group(2).split(":")[0] if not m.group(2).strip().startswith(":") else "")
            words = [re.sub(r"\s*<.*", "", w) for w in words if w not in ("final", "alignas")]
            words = [w for w in words if not re.fullmatch(r"[A-Z0-9_]+", w) or len(words) == 1]
            name = words[-1] if words else ""
            if name:
                qual = "::".join(class_prefix() + [name])
                start = leading_comment_start(src, head_start + len(head) - len(head.lstrip()))
                end = close
                # Include a trailing `;` for type definitions.
                k = end
                while k < n and text[k] in " \t":
                    k += 1
                if k < n and text[k] == ";":
                    end = k + 1
                defs.append(Def(qual, kind, rel, line_of(head_start + len(head) - len(head.lstrip())),
                                start, end, i))
            if kind == "enum":
                i = close
                stmt_start = i
            else:
                scopes.append(("class", name, close))
                i += 1
                stmt_start = i
            continue
        fn = function_name(head)
        if fn is not None:
            # A constructor's member initialisers (`: a_{b}, c_{d} {`) use
            # braces: skip them until the brace that opens the body.
            body = i
            after = head[top_level_paren(head):]
            if re.search(r"\)\s*(?:noexcept\s*)?:", after) or re.search(r"\)\s*(?:noexcept\s*)?:", " ".join(after.split())):
                while body < n:
                    k = body - 1
                    while k > 0 and text[k] in " \t\n":
                        k -= 1
                    if text[k].isalnum() or text[k] in "_>":
                        body = match_brace(text, body)
                        while body < n and text[body] != "{":
                            body += 1
                        continue
                    break
                close = match_brace(text, body)
            quals, last = fn
            name = "::".join(class_prefix() + quals + [last])
            first = head_start + len(head) - len(head.lstrip())
            defs.append(Def(name, "function", rel, line_of(first), leading_comment_start(src, first), close, body))
            i = close
            stmt_start = i
            continue
        # A brace initialiser or something else: skip it whole. A namespace-
        # scope `const YGValue YGValueAuto = {...};` is indexed as a constant.
        cm = re.search(r"(" + IDENT + r")\s*(?:=\s*)?$", flat)
        if inner == "ns" and cm and re.search(r"\bconst(?:expr)?\b", flat):
            first = head_start + len(head) - len(head.lstrip())
            k = close
            while k < n and text[k] in " \t":
                k += 1
            end = k + 1 if k < n and text[k] == ";" else close
            defs.append(Def(cm.group(1), "constant", rel, line_of(first), leading_comment_start(src, first), end, end))
        i = close
        # `int x{0};` continues the statement up to the `;`.
        continue

    # `using Name = ...;` aliases and the C API's YG_ENUM_DECL(Name, ...).
    for m in re.finditer(r"(?m)^[ \t]*using\s+(" + IDENT + r")\s*=[^;]*;", text):
        defs.append(Def(m.group(1), "using", rel, line_of(m.start(1)), leading_comment_start(src, m.start()),
                        m.end(), m.end()))
    for m in re.finditer(r"YG_ENUM_DECL\(\s*(" + IDENT + r")", src):
        end = match_paren(src, src.index("(", m.start())) + 1
        if end < len(src) and src[end] == ";":
            end += 1
        defs.append(Def(m.group(1), "enum-decl", rel, line_of(m.start()), leading_comment_start(src, m.start()),
                        end, end))
    for d in defs:
        d.text = src[d.start:d.end].rstrip()
        # Strip namespace qualifiers that crept in (`yoga::Node::foo`).
        d.name = re.sub(r"^(?:(?:facebook|yoga|detail)::)+", "", d.name)
        if d.kind == "function":
            body = text[d.body_start:d.end]
            calls = re.findall(r"(?:\b|::|\.|->)(" + IDENT + r")\s*(?:<[^<>;(){}]*>)?\s*\(", body)
            d.calls = [c for c in dict.fromkeys(calls) if c not in KEYWORDS]
    return defs


def match_paren(text, i):
    depth = 0
    while i < len(text):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return len(text) - 1


_INDEX = None


def load():
    global _INDEX
    if _INDEX is None:
        if not YOGA.is_dir():
            sys.exit(f"spec: no Yoga source at {YOGA} (git submodule update --init vendor/yoga)")
        defs = []
        for p in sorted(YOGA.rglob("*")):
            if p.suffix in (".cpp", ".h"):
                rel = "vendor/yoga/yoga/" + p.relative_to(YOGA).as_posix()
                defs += index_file(p, rel)
        _INDEX = defs
    return _INDEX


# -- lookup ------------------------------------------------------------------

def camel(name):
    """compute_flex_basis_for_child -> computeFlexBasisForChild."""
    if "_" not in name.strip("_") or name.isupper():
        return name
    first, *rest = name.split("_")
    return first + "".join(w[:1].upper() + w[1:] for w in rest if w)


def lookup(name, defs=None):
    defs = load() if defs is None else defs
    name = name.strip().removeprefix("yoga::").removeprefix("facebook::yoga::")
    candidates = [name]
    if "::" in name:
        head, last = name.rsplit("::", 1)
        candidates.append(head + "::" + camel(last))
    else:
        candidates.append(camel(name))
    for cand in candidates:
        exact = [d for d in defs if d.name == cand]
        if exact:
            return exact
    for cand in candidates:
        if "::" in cand:
            # A trailing component match: `Node::foo` for `yoga::Node::foo`.
            tail = [d for d in defs if d.name.endswith("::" + cand)]
            if tail:
                return tail
            continue
        short = [d for d in defs if d.short == cand]
        if short:
            return short
    lower = {c.lower() for c in candidates}
    return [d for d in defs if d.short.lower() in lower or d.name.lower() in lower]


def render(d):
    return f"// {d.name}  ({d.kind}, {d.file}:{d.line})\n{dedent(d.text)}\n"


def dedent(text):
    lines = text.split("\n")
    widths = [len(l) - len(l.lstrip()) for l in lines[1:] + lines[:1] if l.strip()]
    w = min(widths) if widths else 0
    return "\n".join(l[w:] if l[:w].strip() == "" else l.lstrip() for l in lines)


def refs_for(found, defs):
    """Short definitions called from the found functions, in call order.
    Free functions come before members, and one-line accessors (a member
    whose body is a single statement) are left out: they are noise."""
    seen = {id(d) for d in found}
    out, chars = [], 0
    by_short = {}
    for d in defs:
        if d.kind == "function":
            by_short.setdefault(d.short, []).append(d)
    wanted = []
    for d in found:
        for call in d.calls:
            for r in by_short.get(call, [])[:3]:
                if r.short == d.short or r.lines >= REFS_MAX_LINES:
                    continue
                if "::" in r.name and accessor(r):
                    continue
                wanted.append(r)
    wanted.sort(key=lambda r: "::" in r.name)  # stable: call order within each group
    for r in wanted:
        if id(r) in seen:
            continue
        seen.add(id(r))
        if len(out) >= REFS_MAX or chars + len(r.text) > REFS_MAX_CHARS:
            break
        out.append(r)
        chars += len(r.text)
    return out


def accessor(d):
    """A member whose body is one statement (a getter or setter)."""
    head_len = max(0, d.body_start - d.start)
    return d.text[head_len:].count(";") <= 1 and d.lines <= 6


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__.strip())
        return 0 if argv else 2
    defs = load()
    if argv[0] in ("--find", "--find-text"):
        if len(argv) < 2:
            print("usage: ./spec --find <text>", file=sys.stderr)
            return 2
        q = " ".join(argv[1:]).lower()
        qc = camel(" ".join(argv[1:])).lower()
        if argv[0] == "--find":
            hits = [d for d in defs if q in d.name.lower() or qc in d.name.lower()]
        else:
            hits = [d for d in defs if q in d.text.lower()]
        for d in sorted(hits, key=lambda d: (d.name, d.file)):
            print(f"{d.name:<55} {d.kind:<9} {d.file}:{d.line}  ({d.lines} lines)")
        if not hits:
            print(f"no definitions match {' '.join(argv[1:])!r}")
            return 1
        return 0
    refs = argv[0] == "--refs"
    names = argv[1:] if refs else argv
    status = 0
    found_all = []
    for name in names:
        found = lookup(name, defs)
        if not found:
            print(f"no definition named {name!r} in {YOGA.relative_to(ROOT) if YOGA.is_relative_to(ROOT) else YOGA}; "
                  f"try ./spec --find {name.rsplit('::', 1)[-1]}", file=sys.stderr)
            status = 1
            continue
        for d in found:
            print(render(d))
        found_all += found
    if refs and found_all:
        extra = refs_for(found_all, defs)
        if extra:
            print("// ---- short functions it calls ----\n")
            for d in extra:
                print(render(d))
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
