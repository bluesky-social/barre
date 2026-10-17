#!/usr/bin/env bash
# Commit a README update to main from CI.
#
# Usage: readme_commit.sh MESSAGE COMMAND...
#
# COMMAND rewrites README.md (for example readme_results.py). Several jobs
# write the README on each push to main, and their rows sit next to each
# other, so a rebase would conflict; instead, when the push is rejected this
# resets to the new main and runs COMMAND again. Exit status 2 from COMMAND
# means "nothing to publish" and is not a failure.
set -u
msg=$1
shift
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
for attempt in 1 2 3 4 5; do
  "$@"
  status=$?
  [ $status -eq 2 ] && exit 0
  [ $status -ne 0 ] && exit $status
  git diff --quiet README.md && exit 0
  git commit -q -m "$msg" README.md
  git push origin HEAD:main && exit 0
  echo "push rejected (attempt $attempt); retrying on the new main" >&2
  sleep $((attempt * 3))
  git fetch -q origin main && git reset -q --hard FETCH_HEAD || exit 1
done
exit 1
