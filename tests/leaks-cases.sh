#!/bin/sh
# The cases of tests/leaks.sh (the change `sluice-host-owns-its-workers`,
# design D10; the change `a-test-stops-the-hosts-it-starts`, design D2):
# a test that leaves a watcher fails, names it and leaves nothing; a test
# that leaves a host process fails, names it and leaves nothing, whether
# it ends or is stopped; a host process that ends by itself a moment
# after its test is no leak; a test that leaves nothing passes and says
# nothing; and a test that fails keeps its status. A host process here is
# a detached process that a state folder's sluice/host/version names, as
# sluice host's names its own pid and program. A version file whose pid
# another process now holds names no host. It prints each failure as one line,
# leaks-cases: <case>: <what>, and nothing where every case holds.
set -eu

leaks=$(cd "$(dirname "$0")" && pwd)/leaks.sh
out=$(mktemp)
err=$(mktemp)
mark=$(mktemp)
host=
stale=
trap 'rm -f "$out" "$err" "$mark" "$host" "$stale"' EXIT
failed=0

fail() {
  echo "leaks-cases: $1: $2"
  failed=1
}

run() {
  status=0
  MARK=$mark sh "$leaks" "$@" >"$out" 2>"$err" || status=$?
}

named="A test that leaves a watcher"
run sh -c '(exec sleep 61 >/dev/null 2>&1 </dev/null &) ; exit 0'
[ "$status" -eq 1 ] || fail "$named" "exits $status, not 1"
grep -q "pid [0-9]* sleep 61" "$err" || fail "$named" "names no watcher: $(cat "$err")"
if pgrep -f "^sleep 61$" >/dev/null 2>&1; then
  fail "$named" "the watcher runs on"
  pkill -f "^sleep 61$" || true
fi

# A test that starts a host process in a process group of its own, its
# state folder in the test's TMPDIR, and writes the mark when it has.
# Then it exits 0, or runs `sleep <then>` where given. The host is perl
# named `/leaks-cases/<seconds>/sluice host`, as `sluice host` is named
# by its program, and it sleeps <seconds>.
host=$(mktemp)
cat >"$host" <<'EOS'
mkdir -p "$TMPDIR/host-state/sluice/host"
program=/leaks-cases/$1/sluice
perl -e '$0 = "$ARGV[0] host"; open(STDIN, "</dev/null"); open(STDOUT, ">/dev/null"); open(STDERR, ">/dev/null"); setpgrp(0, 0); sleep $ARGV[1]' "$program" "$1" &
printf '{"pid":%s,"since":"now","program":"%s"}\n' $! "$program" >"$TMPDIR/host-state/sluice/host/version"
echo started >"$MARK"
if [ -n "${2:-}" ]; then
  exec sleep "$2"
fi
exit 0
EOS

# A test that leaves a version file that names the pid it is given.
stale=$(mktemp)
cat >"$stale" <<'EOS'
mkdir -p "$TMPDIR/s/sluice/host"
printf '{"pid":%s,"program":"/x/sluice"}\n' "$1" >"$TMPDIR/s/sluice/host/version"
EOS

hosts_of() {
  pgrep -f "^/leaks-cases/$1/sluice host" 2>/dev/null || true
}

named="A test that leaves a host process"
run sh "$host" 62
[ "$status" -eq 1 ] || fail "$named" "exits $status, not 1"
grep -q "pid [0-9]* /leaks-cases/62/sluice host" "$err" || fail "$named" "names no host process: $(cat "$err")"
if [ -n "$(hosts_of 62)" ]; then
  fail "$named" "the host process runs on"
  pkill -f "^/leaks-cases/62/sluice host" || true
fi

named="A host process that ends by itself"
run sh "$host" 0.3
[ "$status" -eq 0 ] || fail "$named" "exits $status: $(cat "$err")"
[ ! -s "$out" ] && [ ! -s "$err" ] || fail "$named" "says $(cat "$out" "$err")"

named="A version file whose pid another process holds"
sleep 66 &
other=$!
run sh "$stale" "$other"
[ "$status" -eq 0 ] || fail "$named" "exits $status: $(cat "$err")"
kill -0 "$other" 2>/dev/null || fail "$named" "the other process was ended"
kill "$other" 2>/dev/null || true
wait "$other" 2>/dev/null || true

named="A test that is stopped while its host process runs"
: >"$mark"
status=0
MARK=$mark sh "$leaks" sh "$host" 64 63 >"$out" 2>"$err" &
stopped=$!
i=0
while [ ! -s "$mark" ] && [ $i -lt 50 ]; do
  sleep 0.1
  i=$((i + 1))
done
kill -TERM "$stopped"
wait "$stopped" || status=$?
[ "$status" -eq 1 ] || fail "$named" "exits $status, not 1"
grep -q "pid [0-9]* /leaks-cases/64/sluice host" "$err" || fail "$named" "names no host process: $(cat "$err")"
if [ -n "$(hosts_of 64)" ]; then
  fail "$named" "the host process runs on"
  pkill -f "^/leaks-cases/64/sluice host" || true
fi
if pgrep -f "^sleep 63$" >/dev/null 2>&1; then
  fail "$named" "the test runs on"
  pkill -f "^sleep 63$" || true
fi

named="A test that reads its standard input"
status=0
said=$(echo kept | sh "$leaks" sh -c 'read -r line; echo "$line"' 2>"$err") || status=$?
[ "$status" -eq 0 ] || fail "$named" "exits $status: $(cat "$err")"
[ "$said" = kept ] || fail "$named" "read \"$said\", not its input"

named="A test that leaves nothing"
run sh -c 'sleep 0.1 & wait; exit 0'
[ "$status" -eq 0 ] || fail "$named" "exits $status: $(cat "$err")"
[ ! -s "$out" ] && [ ! -s "$err" ] || fail "$named" "says $(cat "$out" "$err")"

named="A test that fails"
run sh -c 'exit 3'
[ "$status" -eq 3 ] || fail "$named" "exits $status, not 3"

named="Usage"
run
[ "$status" -eq 2 ] || fail "$named" "with no command exits $status, not 2"

exit "$failed"
