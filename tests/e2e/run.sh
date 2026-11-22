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
  # Each script gets its own empty settings folder, never the user's ~/.config/glyphium.
  cfg="$out/config-$n"; rm -rf "$cfg"; mkdir -p "$cfg"
  # Header lines choose the shell (`# shell: zsh` runs zsh with the settings folder as HOME) and
  # stand in for the assistant's model (`# assist-stub: REPLY`).
  shell=$(sed -n 's/^# shell: //p' "$here/$n.script" | head -1)
  stub=$(sed -n 's/^# assist-stub: //p' "$here/$n.script" | head -1)
  # env takes its -u options before any NAME=VALUE.
  unset_vars=(-u GLYPHIUM_ASSIST_STUB); set_vars=(GLYPHIUM_CONFIG_DIR="$cfg")
  [ -n "$stub" ] && unset_vars=() && set_vars+=(GLYPHIUM_ASSIST_STUB="$stub")
  if [ "$shell" = zsh ]; then
    : > "$cfg/.zshrc"  # an empty one, so zsh doesn't start its new-user wizard
    unset_vars+=(-u ZDOTDIR); set_vars+=(HOME="$cfg")
    cmd=(/bin/zsh)
  else
    cmd=(/bin/bash --norc --noprofile)
  fi
  env "${unset_vars[@]}" "${set_vars[@]}" "$bin" --config "$cfg/config.toml" --script "$here/$n.script" -e "${cmd[@]}" &
  pid=$!
  ( sleep 90; kill -9 $pid 2>/dev/null ) & watchdog=$!
  wait $pid; rc=$?
  kill $watchdog 2>/dev/null; wait $watchdog 2>/dev/null
  echo "-- $n exit $rc"
  [ $rc -eq 0 ] || failed=1
done
echo "screenshots in $out"
exit $failed
