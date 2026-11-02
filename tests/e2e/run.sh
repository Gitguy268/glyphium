#!/bin/bash
# Runs each script through the real app: window, GPU, PTY, shell. Opens a window per script.
# usage: tests/e2e/run.sh [OUT_DIR] [name ...]   Screenshots land in OUT_DIR.
set -u
here=$(cd "$(dirname "$0")" && pwd)
bin="$here/../../target/release/glyphium"
out=${1:-$(mktemp -d)}; shift || true
mkdir -p "$out"; cd "$out"
names=("$@"); [ ${#names[@]} -eq 0 ] && names=($(cd "$here" && ls *.script | sed 's/\.script$//'))
failed=0
for n in "${names[@]}"; do
  echo "== $n"
  # A hung app must fail the run rather than hang it, so each script gets a time limit.
  "$bin" --script "$here/$n.script" -e /bin/bash --norc --noprofile &
  pid=$!
  ( sleep 90; kill -9 $pid 2>/dev/null ) & watchdog=$!
  wait $pid; rc=$?
  kill $watchdog 2>/dev/null; wait $watchdog 2>/dev/null
  echo "-- $n exit $rc"
  [ $rc -eq 0 ] || failed=1
done
echo "screenshots in $out"
exit $failed
