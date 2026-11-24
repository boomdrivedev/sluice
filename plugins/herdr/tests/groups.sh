#!/bin/sh
# The plugin's tests in four groups at once (tests/run.sh --group), each
# group's output printed whole once it ends, in order; fails where any
# group fails. Each test has its own folder, servers and leases, and each
# group its own scratch directory and process.
set -u
here=$(cd "$(dirname "$0")" && pwd -P)
of=4
out=$(mktemp -d "${SLUICE_TMP:-${TMPDIR:-/tmp}}/sluice-herdr-groups.XXXXXX")
pids=
i=0
while [ "$i" -lt "$of" ]; do
  sh "$here/run.sh" --group "$i/$of" >"$out/$i" 2>&1 &
  pids="$pids $!"
  i=$((i + 1))
done
status=0
for p in $pids; do
  wait "$p" || status=1
done
i=0
while [ "$i" -lt "$of" ]; do
  echo "== group $i of $of"
  cat "$out/$i"
  i=$((i + 1))
done
rm -rf "$out"
exit "$status"
