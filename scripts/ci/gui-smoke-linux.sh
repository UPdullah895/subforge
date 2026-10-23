#!/usr/bin/env bash
# GUI smoke test for CI: start the app on an X display, require that its main window
# ("SubForge") appears and that the process is still alive after a while, then stop it.
#
#   xvfb-run -a scripts/ci/gui-smoke-linux.sh <app executable> [seconds=15] [log file]
#
# Needs xdotool. Exit 0 ok, 1 failed, 2 usage.
set -uo pipefail

app=${1:?usage: gui-smoke-linux.sh <app executable> [seconds] [log file]}
seconds=${2:-15}
log=${3:-gui-smoke.log}
title='SubForge'

if [ -z "${DISPLAY:-}" ]; then
  echo "FAIL: DISPLAY is not set; run under xvfb-run" >&2
  exit 2
fi
command -v xdotool >/dev/null || { echo "FAIL: xdotool is not installed" >&2; exit 2; }

# Own process group, so the AppImage runtime and the app it starts are stopped together.
setsid "$app" >"$log" 2>&1 &
pid=$!

stop() {
  kill -TERM -- "-$pid" 2>/dev/null
  for _ in 1 2 3 4 5; do kill -0 "$pid" 2>/dev/null || return 0; sleep 1; done
  kill -KILL -- "-$pid" 2>/dev/null
}

# A visible (mapped) window whose title is exactly "SubForge". xdotool's --name regex ignores
# case, and GTK also creates an unmapped leader window named after the binary ("subforge").
find_window() {
  local id
  for id in $(xdotool search --onlyvisible --name "^${title}\$" 2>/dev/null); do
    if [ "$(xdotool getwindowname "$id" 2>/dev/null)" = "$title" ]; then
      echo "$id"
      return
    fi
  done
}

window=""
for ((i = 0; i < seconds; i++)); do
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"
    echo "FAIL: the app exited after ${i}s with status $?; log:" >&2
    cat "$log" >&2
    exit 1
  fi
  [ -n "$window" ] || window=$(find_window)
done

if [ -z "$window" ]; then
  echo "FAIL: no window titled SubForge after ${seconds}s; log:" >&2
  cat "$log" >&2
  stop
  exit 1
fi

echo "GUI smoke passed: window $window ($(xdotool getwindowname "$window")) open, process alive after ${seconds}s"
stop
exit 0
