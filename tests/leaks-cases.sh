#!/bin/sh
# The cases of tests/leaks.sh (the change `sluice-host-owns-its-workers`,
# design D10): a test that leaves a watcher fails, names it and leaves
# nothing; a test that leaves nothing passes and says nothing; and a test
# that fails keeps its status. It prints each failure as one line,
# leaks-cases: <case>: <what>, and nothing where every case holds.
set -eu

leaks=$(cd "$(dirname "$0")" && pwd)/leaks.sh
out=$(mktemp)
err=$(mktemp)
trap 'rm -f "$out" "$err"' EXIT
failed=0

fail() {
  echo "leaks-cases: $1: $2"
  failed=1
}

run() {
  status=0
  sh "$leaks" "$@" >"$out" 2>"$err" || status=$?
}

named="A test that leaves a watcher"
run sh -c '(exec sleep 61 >/dev/null 2>&1 </dev/null &) ; exit 0'
[ "$status" -eq 1 ] || fail "$named" "exits $status, not 1"
grep -q "pid [0-9]* sleep 61" "$err" || fail "$named" "names no watcher: $(cat "$err")"
if pgrep -f "^sleep 61$" >/dev/null 2>&1; then
  fail "$named" "the watcher runs on"
  pkill -f "^sleep 61$" || true
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
