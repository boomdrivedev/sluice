#!/bin/sh
# Run a test task and fail it where it leaves a process
# (`a-leaked-child-fails-its-test@9a82a5147ba6`; the change
# `sluice-host-owns-its-workers`, design D10; the change
# `a-test-stops-the-hosts-it-starts`, design D2):
#
#   leaks.sh <command> [<argument>]...
#
# runs the command in a process group of its own, with a scratch folder
# of its own as TMPDIR and a state folder of its own as XDG_STATE_HOME.
# The scratch folder's name is short: it adds 10 characters to each path
# under TMPDIR, and a socket's path holds 103 at most.
# A SIGINT, SIGTERM or SIGHUP that stops the script goes to the command's
# process group, and the script goes on when the command ends.
# When the command ends, each sluice host process that a state folder in
# the scratch folder names has one second to end by itself: an idle host
# process stays a moment after its last client. Each one that still runs
# is left by the test. The script stops it, which ends its workers, and
# waits for it. Then a process of the test is also one of the command's
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

scratch=$(mktemp -d "${TMPDIR:-/tmp}/lk.XXXXXX")
scratch=$(cd "$scratch" && pwd -P)
mkdir "$scratch/state"
trap 'rm -rf "$scratch"' EXIT

runs() {
  kill -0 "$1" 2>/dev/null
}

# The command in a process group of its own, its standard input kept.
exec 3<&0
if command -v setsid >/dev/null 2>&1; then
  TMPDIR=$scratch XDG_STATE_HOME=$scratch/state setsid "$@" <&3 &
else
  TMPDIR=$scratch XDG_STATE_HOME=$scratch/state \
    perl -e 'setpgrp(0, 0) or die "setpgrp: $!\n"; exec @ARGV or die "$ARGV[0]: $!\n"' "$@" <&3 &
fi
group=$!
for s in INT TERM HUP; do
  # shellcheck disable=SC2064 # the signal's name, fixed as each trap is set
  trap "kill -s $s -- -$group 2>/dev/null || true" "$s"
done
# A trapped signal ends a wait early: wait again while the command runs.
while :; do
  status=0
  wait "$group" || status=$?
  runs "$group" || break
done

# Each host process that a state folder of the test names: the pid and
# the program that its version file gives. The file stays after the host
# ends, and another process can then hold its pid. So a pid is the host
# only while its command line is the program with `host` or `daemon`.
given=$(find "$scratch" -path '*/sluice/host/version' -type f 2>/dev/null |
  while read -r file; do
    sed -n 's/.*"pid":\([0-9][0-9]*\).*"program":"\([^"]*\)".*/\1 \2/p' "$file"
  done | sort -u)

# The hosts of the test that run, each as `pid command`. Each by its
# whole command line: procps's pgrep gives the name alone unless -a,
# which on macOS also lets pgrep's ancestors match.
running() {
  [ -n "$given" ] || return 0
  all=$(pgrep -alf . 2>/dev/null || true)
  printf '%s\n' "$given" | while read -r pid program; do
    printf '%s\n' "$all" | awk -v p="$pid" -v c="$program" \
      '$1 == p && (index($0, p " " c " host") == 1 || index($0, p " " c " daemon") == 1) { print; exit }'
  done
}

# An idle host process stays a moment after its last client: each has a
# second to end.
i=0
left_hosts=$(running)
while [ -n "$left_hosts" ] && [ $i -lt 10 ]; do
  sleep 0.1
  i=$((i + 1))
  left_hosts=$(running)
done

# Each host left is named, then stopped: it ends its workers as it stops.
hosts=$(printf '%s\n' "$left_hosts" | sed -n 's/^\([0-9][0-9]*\) .*/\1/p')
named=$(printf '%s\n' "$left_hosts" | sed -n 's/^\([0-9][0-9]* .*\)/  pid \1/p')
for pid in $hosts; do
  kill -TERM "$pid" 2>/dev/null || true
done
for pid in $hosts; do
  i=0
  while runs "$pid" && [ $i -lt 300 ]; do
    sleep 0.1
    i=$((i + 1))
  done
done

# The pids of the test's other processes that are left, and each host
# that did not stop.
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
if [ -z "$hosts" ] && [ -z "$pids" ]; then
  exit "$status"
fi
echo "leaks.sh: the test left these processes running; they are ended now:" >&2
if [ -n "$named" ]; then
  printf '%s\n' "$named" >&2
fi
if [ -n "$pids" ]; then
  listed=" $(printf '%s\n' "$pids" | paste -s -d ' ' -) "
  pgrep -alf . 2>/dev/null | awk -v pids="$listed" \
    'index(pids, " " $1 " ") { print "  pid " $0 }' >&2
fi
for pid in $hosts; do
  if runs "$pid"; then
    pids="$pids
$pid"
  fi
done
for pid in $pids; do
  kill -TERM "$pid" 2>/dev/null || true
done
if [ -n "$pids" ]; then
  sleep 2
fi
for pid in $pids; do
  if runs "$pid"; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
done
exit 1
