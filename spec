#!/bin/sh
# Look up Yoga C++ definitions (the spec). Usage: ./spec computeFlexBasisForChild | ./spec --find baseline
exec python3 "$(dirname "$0")/harness/spec.py" "$@"
