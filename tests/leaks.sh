#!/bin/sh
# Run a shell test task and fail it where it leaves a process
# (`a-leaked-child-fails-its-test@9a82a5147ba6`; the change
# `sluice-host-owns-its-workers`, design D10):
#
#   leaks.sh <command> [<argument>]...
#
# runs the command in a process group of its own, with a scratch folder
# of its own as TMPDIR and a state folder of its own as XDG_STATE_HOME.
# When the command ends, it stops each sluice host process that a state
# folder in the scratch folder names, which ends that process's workers,
# and waits for it. Then a process of the test is one of the command's
# process group, or a worker that a state folder in the scratch folder
# records. The script names each that is left, by its pid and its
# command, ends it, SIGTERM first and SIGKILL after two seconds, and
# fails. Otherwise it exits with the command's status.
#
# It reads processes with pgrep alone: the gate's sandbox refuses ps,
# which is set-user-ID on macOS.
set -eu

[ $# -ge 1 ] || {
  echo "usage: leaks.sh <command> [<argument>]..." >&2
  exit 2
}

scratch=$(mktemp -d "${TMPDIR:-/tmp}/sluice-leaks.XXXXXX")
scratch=$(cd "$scratch" && pwd -P)
mkdir "$scratch/tmp" "$scratch/state"
trap 'rm -rf "$scratch"' EXIT

# The command in a process group of its own, its standard input kept.
exec 3<&0
if command -v setsid >/dev/null 2>&1; then
  TMPDIR=$scratch/tmp XDG_STATE_HOME=$scratch/state setsid "$@" <&3 &
else
  TMPDIR=$scratch/tmp XDG_STATE_HOME=$scratch/state \
    perl -e 'setpgrp(0, 0) or die "setpgrp: $!\n"; exec @ARGV or die "$ARGV[0]: $!\n"' "$@" <&3 &
fi
group=$!
status=0
wait "$group" || status=$?

runs() {
  kill -0 "$1" 2>/dev/null
}

# Each sluice host process that a state folder of the test names is no
# leak: stopped, it ends its workers.
hosts=$(find "$scratch" -path '*/sluice/host/version' -type f 2>/dev/null |
  while read -r file; do
    sed -n 's/.*"pid":\([0-9][0-9]*\).*/\1/p' "$file"
  done)
for pid in $hosts; do
  if runs "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
  fi
done
for pid in $hosts; do
  i=0
  while runs "$pid" && [ $i -lt 300 ]; do
    sleep 0.1
    i=$((i + 1))
  done
done

# The pids of the test's processes that are left.
left() {
  {
    pgrep -g "$group" . 2>/dev/null || true
    find "$scratch" -path '*/sluice/host/workers/*' -type f 2>/dev/null |
      while read -r file; do
        pid=$(basename "$file")
        if runs "$pid"; then
          echo "$pid"
        fi
      done
  } | sort -un
}

pids=$(left)
if [ -z "$pids" ]; then
  exit "$status"
fi
echo "leaks.sh: the test left these processes running; they are ended now:" >&2
listed=" $(printf '%s\n' "$pids" | paste -s -d ' ' -) "
pgrep -lf . 2>/dev/null | awk -v pids="$listed" \
  'index(pids, " " $1 " ") { print "  pid " $0 }' >&2
for pid in $pids; do
  kill -TERM "$pid" 2>/dev/null || true
done
sleep 2
for pid in $pids; do
  if runs "$pid"; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
done
exit 1
