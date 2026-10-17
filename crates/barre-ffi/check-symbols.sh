#!/bin/sh
# Checks that libbarre_ffi exports every function and global declared by
# Yoga's public headers (vendor/yoga/yoga/Yoga.h and what it includes).
# Usage: crates/barre-ffi/check-symbols.sh [path/to/libbarre_ffi.a]
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
lib=${1:-$root/target/debug/libbarre_ffi.a}
[ -f "$lib" ] || (cd "$root" && cargo build -q -p barre-ffi)
headers="YGConfig.h YGEnums.h YGNode.h YGNodeLayout.h YGNodeStyle.h YGPixelGrid.h YGValue.h"

declared=$(mktemp)
exported=$(mktemp)
trap 'rm -f "$declared" "$exported"' EXIT

for h in $headers; do
  cat "$root/vendor/yoga/yoga/$h"
done | tr '\n' ' ' | tr ';' '\n' | grep 'YG_EXPORT' |
  sed -E 's/.*YG_EXPORT//' |
  grep -oE '(YG[A-Za-z0-9]+)[[:space:]]*(\(|$)' |
  sed -E 's/[[:space:]]*\(?$//' >"$declared.raw"
# YG_ENUM_DECL(Name, ...) declares NameToString.
for h in $headers; do cat "$root/vendor/yoga/yoga/$h"; done |
  grep -A1 'YG_ENUM_DECL(' | grep -oE '^ *YG[A-Za-z]+,' | tr -d ' ,' |
  sed 's/$/ToString/' >>"$declared.raw"
sort -u "$declared.raw" >"$declared"
rm -f "$declared.raw"

nm -g "$lib" 2>/dev/null | awk '$2 ~ /^[TDRSB]$/ { print $3 }' | sed 's/^_//' | grep '^YG' | sort -u >"$exported"

missing=$(comm -23 "$declared" "$exported")
count=$(wc -l <"$declared" | tr -d ' ')
if [ -n "$missing" ]; then
  echo "missing exports:"
  echo "$missing"
  exit 1
fi
echo "ok: all $count declared YG symbols are exported"
