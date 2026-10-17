#!/usr/bin/env python3
"""Put a generated Markdown block into README.md between markers.

Usage: readme_section.py NAME MARKDOWN_FILE README_MD

The block lives between <!-- NAME:start --> and <!-- NAME:end -->. If the
README has no such markers yet, a new section is added just before
"## Using it", after the results table and any earlier sections, so a new
section only needs to call this script. Pass rates go in the one results
table instead (readme_results.py); this is for blocks such as the benchmarks.
"""
import re
import sys

ANCHOR = "## Using it"


def main() -> int:
    name, block_path, readme_path = sys.argv[1:4]
    if not re.fullmatch(r"[a-z0-9-]+", name):
        print(f"bad section name {name!r}", file=sys.stderr)
        return 1
    start, end = f"<!-- {name}:start -->", f"<!-- {name}:end -->"
    with open(block_path) as f:
        block = f.read().strip()
    with open(readme_path) as f:
        text = f.read()
    section = f"{start}\n\n{block}\n\n{end}"
    pattern = re.compile(re.escape(start) + r".*?" + re.escape(end), re.S)
    if pattern.search(text):
        new = pattern.sub(lambda _: section, text, count=1)
    elif ANCHOR in text:
        new = text.replace(ANCHOR, f"{section}\n\n{ANCHOR}", 1)
    else:
        print(f"{readme_path}: no markers for {name} and no '{ANCHOR}' heading", file=sys.stderr)
        return 1
    with open(readme_path, "w") as f:
        f.write(new)
    return 0


if __name__ == "__main__":
    sys.exit(main())
