#!/bin/sh
# shellcheck disable=SC2016 # jq programs name their own $variables.
# The herdr plugin's scripts against stand-in `herdr` and `sluice` on PATH:
# recorded overview documents in, the herdr calls made checked, each herdr
# server a socket file in the test's folder answered from that socket's
# files. A test that counts what the plugin runs puts tests/fake-logged
# first on PATH, whose `jq` and `git` log each call. git runs isolated
# from the user's config and credentials on scratch repositories, and
# nothing here reaches a network, a credential or a running herdr.

set -u
here=$(cd "$(dirname "$0")" && pwd -P)
plugin=$(dirname "$here")

# Under sluice's sandbox only SLUICE_TMP is writable beside the checkout.
scratch=$(mktemp -d "${SLUICE_TMP:-${TMPDIR:-/tmp}}/sluice-herdr.XXXXXX")

plugin_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$plugin/herdr-plugin.toml")

# A lease this run holds for a test, as a watcher of this plugin would, so
# --ensure starts none and what the test asks of the watcher stays put:
# the ensure's signal to it is counted in $woken.
hold_lease() {
  printf '%s\n%s\n%s\n%s\n' $$ "$(procs identity $$)" "$plugin" "$plugin_version" >"$S/watch.lease"
}

# Wait up to $1 seconds for the shell condition $2, checking each second.
within() {
  n=$1
  while [ "$n" -gt 0 ]; do
    eval "$2" && return 0
    sleep 1
    n=$((n - 1))
  done
  eval "$2"
}

# Wait up to $2 seconds for process $1 to end, checking each second; say
# how long it took in $took. A process is waited for, never slept on: under
# a loaded gate, ending takes longer than any fixed sleep allows.
ends_within() {
  began=$(date +%s)
  while kill -0 "$1" 2>/dev/null; do
    took=$(($(date +%s) - began))
    [ "$took" -ge "$2" ] && return 1
    sleep 1
  done
  took=$(($(date +%s) - began))
}

# Wait up to 30 seconds for what the plugin runs, the herdr, sluice, jq and
# git calls logged, to stand still for three seconds.
settled() {
  st_n=30
  st_last=
  st_same=0
  while [ "$st_n" -gt 0 ]; do
    st_now=$(cat "$FAKE"/*-calls 2>/dev/null | wc -c)
    if [ "$st_now" = "$st_last" ]; then
      st_same=$((st_same + 1))
      [ "$st_same" -ge 3 ] && return 0
    else
      st_same=0
      st_last=$st_now
    fi
    sleep 1
    st_n=$((st_n - 1))
  done
  return 1
}

# Have the running watcher look three times, by an ensure each, made with
# the folder $1 first on PATH as the watcher was started, each look known
# by its pane list; then a tick, at which it looks again.
looks() {
  for i in 1 2 3; do
    : >"$FAKE/herdr-calls"
    PATH="$1:$PATH" sh "$plugin/bin/watch.sh" --ensure
    within 10 'grep -q ": pane list$" "$FAKE/herdr-calls"' || fail "the watcher did not look at ensure $i"
  done
  sleep 2
}

# A folder of stand-ins, made for the test, that log each call of the
# tools named as tests/fake-logged does jq's and git's, in
# $FAKE/<tool>-calls; its path printed.
logging() {
  mkdir -p "$t/logged"
  for x in "$@"; do
    printf '#!/bin/sh\n. "%s/logged.sh"\n' "$here" >"$t/logged/$x"
    chmod +x "$t/logged/$x"
  done
  echo "$t/logged"
}

# Whether every process named has ended.
ended() {
  for p in "$@"; do kill -0 "$p" 2>/dev/null && return 1; done
  return 0
}

# The fake sluice's watches this test started that still run, by the pids
# they wrote in its own directory: never a pattern over command lines,
# which another run of this suite from the same checkout matches as well.
watching() {
  [ -f "$FAKE/watch-pids" ] || return 0
  while read -r p; do
    kill -0 "$p" 2>/dev/null && echo "$p"
  done <"$FAKE/watch-pids"
}

# The pids of the sources a watcher started.
sources_of() {
  cat "$S/run/$1"/*.pids 2>/dev/null | cut -f1
}

# One of lib/procs.sh's functions.
procs() {
  sh -c '. "$1/lib/procs.sh"; shift; "$@"' _ "$plugin" "$@"
}

# The pid holding a lease: the first word of its first line.
holder_of() {
  head -1 "$1" 2>/dev/null | cut -d' ' -f1
}

# A watcher a test started, through --ensure or itself, is ended with it:
# only those whose lease is in this run's scratch directory.
reap() {
  for l in "$scratch"/t*/state/watch.lease "$scratch"/t*/state/*/*/watch.lease; do
    [ -f "$l" ] || continue
    p=$(holder_of "$l")
    [ -n "$p" ] && [ "$p" != $$ ] && kill "$p" 2>/dev/null
  done
  sleep 1
}
trap 'reap; rm -rf "$scratch"' EXIT
woken=0
trap 'woken=$((woken + 1))' USR1
scratch=$(cd "$scratch" && pwd -P)
# git looks for a repository no higher than the scratch directory: the gate
# runs the suite with it inside sluice's own bare repository.
export GIT_CEILING_DIRECTORIES="$scratch"

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@localhost
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@localhost
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_ASKPASS SSH_ASKPASS
unset HERDR_PLUGIN_CONTEXT_JSON HERDR_SOCKET_PATH HERDR_ENV
# SLUICE_HERDR_SH names the shell every script runs under as `sh`, as
# herdr runs them: dash, as Debian's /bin/sh is, or the system's.
if [ -n "${SLUICE_HERDR_SH:-}" ]; then
  mkdir -p "$scratch/sh"
  ln -s "$(command -v "$SLUICE_HERDR_SH")" "$scratch/sh/sh"
  export PATH="$scratch/sh:$PATH"
fi
echo "scripts run under $(command -v sh) -> $(readlink "$(command -v sh)" || echo itself)"
export PATH="$here/fake:$PATH" HERDR_BIN_PATH="$here/fake/herdr" HERDR_PLUGIN_ROOT="$plugin"
NOW=$(jq -n '"2026-10-16T19:45:10Z" | fromdateiso8601')
export SLUICE_HERDR_NOW="$NOW"

# The repositories: `repo` with sluice's records, its worktrees `work`
# (3 commits ahead of main) and `other`; `plain` and `third`, repositories
# a test gives records or none; `home`, no repository at all.
repo=$scratch/repo
git init -q -b main "$repo"
git -C "$repo" commit -q --allow-empty -m root
main=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" worktree add -q -b work "$scratch/work" main
for i in 1 2 3; do git -C "$scratch/work" commit -q --allow-empty -m "w$i"; done
git -C "$repo" worktree add -q -b other "$scratch/other" main
git init -q -b main "$scratch/plain"
git -C "$scratch/plain" commit -q --allow-empty -m root
git init -q -b main "$scratch/third"
git -C "$scratch/third" commit -q --allow-empty -m root
mkdir -p "$scratch/home"
common=$(cd "$repo/.git" && pwd -P)
plain_common=$(cd "$scratch/plain/.git" && pwd -P)
third_common=$(cd "$scratch/third/.git" && pwd -P)
work=$scratch/work
other=$scratch/other

failures=0
failed_before=0
count=0
skips=0
name=

# A fresh state, config and stand-in directory for one test.
fresh() {
  reap
  name=$1
  failed_before=$failures
  count=$((count + 1))
  t=$scratch/t$count
  mkdir -p "$t/state" "$t/config" "$t/fake"
  export HERDR_PLUGIN_STATE_DIR="$t/state" HERDR_PLUGIN_CONFIG_DIR="$t/config" FAKE="$t/fake"
  export SLUICE_HERDR_NOW="$NOW"
  # The host's state, which every herdr server's views read.
  S=$t/state
  mkdir -p "$S/docs" "$S/marks"
  : >"$FAKE/herdr-calls"
  : >"$FAKE/sluice-calls"
  : >"$FAKE/stdin-lines"
  woken=0
  serve herdr
  panes front-none
}

# Make the herdr server named $1 the one in the environment: its socket a
# file in the test's folder, made if it is not there, and $H the folder of
# that socket's files, which tests/fake/herdr answers from.
serve() {
  export HERDR_SOCKET_PATH="$t/$1.sock"
  H=$HERDR_SOCKET_PATH.d
  [ -e "$HERDR_SOCKET_PATH" ] || : >"$HERDR_SOCKET_PATH"
  mkdir -p "$H"
}

# The folder in the host's state of the herdr server named $1.
folder_of() {
  echo "$S/servers/$(printf '%s' "$t/$1.sock" | cksum | cut -d' ' -f1)"
}

fail() {
  failures=$((failures + 1))
  printf 'FAIL %s: %s\n' "$name" "$*"
}

# Whether a real ps (or /proc) can be read here, for the few tests that
# need one; where not (sluice's sandbox on macOS), the test is skipped,
# said and counted.
ps_error=$(ps -o pid= -p $$ 2>&1 >/dev/null)
if [ -r /proc/$$/stat ] || ps -o pid= -p $$ >/dev/null 2>&1; then ps_runs=true; else ps_runs=false; fi
needs_ps() {
  [ "$ps_runs" = true ] && return 0
  echo "skip: $name: ps can't run here (${ps_error:-no output})"
  skips=$((skips + 1))
  return 1
}

# How the processes a process started are listed here: by ps, or, where
# sluice's sandbox on macOS refuses the set-uid ps, by pgrep, which it lets
# run. A test that lists them is skipped only where neither can.
if ps -o pid= -p $$ >/dev/null 2>&1; then
  lists="ps"
else
  pgrep -P $$ >/dev/null 2>&1
  if [ $? -le 1 ]; then lists="pgrep"; else lists=""; fi
fi
needs_children() {
  [ -n "$lists" ] && return 0
  echo "skip: $name: neither ps nor pgrep can run here (${ps_error:-no output})"
  skips=$((skips + 1))
  return 1
}

# The processes whose parent is pid $1, one a line: its pid, then its
# command line.
children_of() {
  if [ "$lists" = ps ]; then
    ps -A -o pid=,ppid=,args= | awk -v p="$1" '$2 == p { c = $1; sub(/^ *[0-9]+ +[0-9]+ +/, ""); print c, $0 }'
  else
    pgrep -lf -P "$1"
  fi
}

pass() {
  [ "$failures" = "$failed_before" ] && printf 'ok   %s\n' "$name"
  return 0
}

calls() { cat "$FAKE/herdr-calls"; }
has() { grep -qF -- "$1" "$FAKE/herdr-calls" || fail "no herdr call with: $1"; }
hasnt() { if grep -qF -- "$1" "$FAKE/herdr-calls"; then fail "a herdr call with: $1"; fi; }

# The server's panes: p1 in work (workspace w1, tab t1), p2 in other (w2,
# t2), p3 in home (w1, t1). $1 names the pane in front, or front-none.
panes() {
  jq -n -c --arg work "$work" --arg other "$other" --arg home "$scratch/home" --arg f "$1" '
    {result: {panes: [
      {pane_id: "p1", tab_id: "t1", workspace_id: "w1", agent: "claude", cwd: $work, foreground_cwd: $work, focused: ($f == "p1")},
      {pane_id: "p2", tab_id: "t2", workspace_id: "w2", agent: "claude", cwd: $other, foreground_cwd: $other, focused: ($f == "p2")},
      {pane_id: "p3", tab_id: "t1", workspace_id: "w1", cwd: $home, foreground_cwd: $home, focused: false}]}}' >"$H/panes.json"
  jq -n -c --arg f "$1" '{result: {tabs: [{tab_id: "t1", focused: ($f == "p1")}, {tab_id: "t2", focused: ($f == "p2")}]}}' >"$H/tabs.json"
  jq -n -c --arg f "$1" '{result: {workspaces: [{workspace_id: "w1", focused: ($f == "p1")}, {workspace_id: "w2", focused: ($f == "p2")}]}}' >"$H/spaces.json"
}

# The token sluice_branch=$2 on pane $1, as an orchestrator sets it,
# beside another source's token in the one map herdr keeps.
named() {
  jq -c --arg p "$1" --arg b "$2" '.result.panes |= map(if .pane_id == $p then .tokens = {sluice_branch: $b, other_tool: "x"} else . end)' \
    "$H/panes.json" >"$H/panes.next" && mv "$H/panes.next" "$H/panes.json"
}

# The server's panes, one for each pair of arguments: a pane's id and its
# folder, each in workspace w1 and tab t1, none in front.
panes_at() {
  pa=
  while [ $# -ge 2 ]; do
    pa="$pa$1	$2
"
    shift 2
  done
  printf '%s' "$pa" | jq -R -s -c 'split("\n") | map(select(length > 0) | split("\t"))
    | {result: {panes: map({pane_id: .[0], tab_id: "t1", workspace_id: "w1", agent: "claude",
        cwd: .[1], foreground_cwd: .[1], focused: false})}}' >"$H/panes.json"
}

# A document for repo: main with nothing queued, changed by the jq in $1;
# each branch's `recent_paths` and `recent_failed` from the records the
# test gave (record_of), as sluice forms them from its live records.
doc() {
  jq -n -c --arg repo "$common" --arg main "$main" --arg work "$work" --arg other "$other" "
    {schema: \"sluice/overview@1\", host: \"test\", repository: \$repo, at: \"2026-10-16T19:45:10Z\",
     branches: [{branch: \"main\", at: \$main, version: 1, window: 1, parallel: 2, depth: 0,
                 candidates: [], submissions: [], recent: []}],
     sessions: [], previews: []} | ${1:-.}" | recorded
}

# A document with the fields sluice reads from the records record_of gave:
# for each branch, the folder of each submission a recent blame or
# conflict names, and the failed tests of each a blame names.
recorded() {
  if [ -s "${FAKE:-/nonexistent}/records.jsonl" ]; then
    jq -c --slurpfile r "$FAKE/records.jsonl" '
      if (.branches | type) == "array" then .branches |= map(
        if type == "object" and (.recent | type) == "array" then
          .branch as $b
          | [.recent[] | select(.event == "blamed" or .event == "conflict") | .id] as $named
          | [.recent[] | select(.event == "blamed") | .id] as $blamed
          | .recent_paths = (([$r[] | select(.branch == $b and (.id as $i | $named | index($i))) | {(.id | tostring): .path}] | add) // {})
          | .recent_failed = (([$r[] | select(.branch == $b and (.id as $i | $blamed | index($i))) | {(.id | tostring): .failed}] | add) // {})
        else . end) else . end'
  else
    cat
  fi
}

# Submission #$1 from worktree $2 in state $3.
sub() { echo "{id: $1, state: \"$3\", from: {agent: \"claude\", worktree: \"w\", path: $2}}"; }

feed() {
  # $@ documents, one per argument, handed to document.sh as the watcher does.
  for d in "$@"; do
    printf '%s\n' "$d" | sh "$plugin/bin/document.sh" local
  done
}

# Keep the panes of the server in the environment in its folder, as the
# watcher does at that server's ensure (D6).
kept() {
  sh -c '. "$1/lib/common.sh"; . "$lib/panes.sh"; mkdir -p "$server"; keep_panes "$server"' _ "$plugin"
}

# The server's tokens, its panes read first, as at the minute's pass.
tokens() { sh "$plugin/bin/tokens.sh" --read "$@"; }
# The forecasts from every server's kept panes, the environment's read first.
forecast() {
  kept
  sh "$plugin/bin/forecast.sh"
}

testing95=$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$work' testing)]
  | .branches[0].candidates = [{id: \"c1\", submissions: [95], revision: \"x\", state: \"testing\",
      started: \"2026-10-16T19:42:00Z\", limit: 900, targets: [\"#check-fast\"], executor: \"sandbox\", ran: 0, replayed: 0, failed: []}]")
blamed87='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:00Z", event: "blamed", id: 87, combination: "b03f4a00e8"}]'
landed94='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:33:10Z", event: "landed", ids: [94], to: "46cc4290c0"}]'
conflict53='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:30Z", event: "conflict", id: 53, paths: ["a.rs"], with: []}]'
# release, whose submissions come from other worktrees: #12 under test, 3
# of 8 tasks ended; #13 to #15 queued; two agents' checks held to it.
release_busy='.branches += [{branch: "release", at: $main, version: 1, window: 1, parallel: 1, depth: 4,
    candidates: [{id: "c9", submissions: [12], revision: "x", state: "testing", started: "2026-10-16T19:42:00Z",
      limit: 900, targets: ["#check-fast"], executor: "sandbox", ran: 0, replayed: 0, failed: [],
      tasks: ([range(3) | {target: "t\(.):test", state: "passed"}] + [range(5) | {target: "u\(.):test", state: "waiting"}])}],
    submissions: [12, 13, 14, 15 | {id: ., state: (if . == 12 then "testing" else "queued" end),
      from: {agent: "unit", worktree: "u", path: "/elsewhere/u\(.)"}}],
    recent: []}]
  | .sessions += [{worktree: "/elsewhere/u1", branch: "release", run: {started: "2026-10-16T19:44:00Z"}},
      {worktree: "/elsewhere/u2", branch: "release", run: {started: "2026-10-16T19:44:00Z"}}]'
release_idle='.branches += [{branch: "release", at: $main, version: 1, window: 1, depth: 0, candidates: [], submissions: [], recent: []}]'
# #$1's live record on the branch $4, main where none is given: submitted
# from the folder $2, its failed tests $3 as JSON. A document doc() makes
# after it carries them where a recent line names #$1.
record_of() {
  jq -n -c --argjson id "$1" --arg p "$2" --argjson f "${3:-[]}" --arg b "${4:-main}" \
    '{id: $id, branch: $b, path: $p, failed: $f}' >>"$FAKE/records.jsonl"
}

frame_of() {
  # $1 directory of the focused pane, then flags
  d=$1
  shift
  SLUICE_HERDR_DIR=$d sh "$plugin/bin/status.sh" "$@" </dev/null
}

## the manifest

fresh 'manifest: a broken one refused'
# A copy of the plugin, broken three ways.
cp -R "$plugin/bin" "$plugin/lib" "$t/"
sed 's|bin/jump.sh|bin/nope.sh|; s|action = "open-link"|action = "gone"|' "$plugin/herdr-plugin.toml" >"$t/herdr-plugin.toml"
echo 'width = 80%' >>"$t/herdr-plugin.toml"
if sh "$here/manifest.sh" "$t/herdr-plugin.toml" >"$t/out" 2>&1; then fail "a broken manifest passed"; fi
grep -q 'runs bin/nope.sh, which is not in the plugin' "$t/out" || fail "a missing script not named: $(cat "$t/out")"
grep -q 'a link handler names no action of this plugin: "gone"' "$t/out" || fail "an unknown action not named"
grep -q 'a value herdr cannot read: width = 80%' "$t/out" || fail "an unreadable line not named"
grep -v '^id = ' "$plugin/herdr-plugin.toml" >"$t/herdr-plugin.toml"
if sh "$here/manifest.sh" "$t/herdr-plugin.toml" >"$t/out" 2>&1; then fail "a manifest with no id passed"; fi
grep -q ': no id$' "$t/out" || fail "the missing id not named: $(cat "$t/out")"
pass

## the stand-ins

fresh 'stand-ins: herdr answers from its socket, jq and git are logged'
"$here/fake/herdr" pane list | jq -e '.result.panes | length == 3' >/dev/null || fail "herdr did not answer from its socket's files"
grep -qx 'herdr.sock: pane list' "$FAKE/herdr-calls" || fail "herdr logged: $(calls)"
serve gone
rm -f "$HERDR_SOCKET_PATH"
"$here/fake/herdr" pane list >/dev/null && fail "a server whose socket is gone answered"
out=$(cd "$work" && PATH="$here/fake-logged:$PATH" jq -n -c '[1,
  2]')
[ "$out" = '[1,2]' ] || fail "jq answered: $out"
[ "$(cat "$FAKE/jq-calls")" = 'work: -n -c [1,   2]' ] || fail "jq logged: $(cat "$FAKE/jq-calls")"
out=$(cd "$work" && PATH="$here/fake-logged:$PATH" git rev-parse HEAD)
[ "$out" = "$(git -C "$work" rev-parse HEAD)" ] || fail "git answered: $out"
[ "$(cat "$FAKE/git-calls")" = 'work: rev-parse HEAD' ] || fail "git logged: $(cat "$FAKE/git-calls")"
pass

## herdr-watch

fresh 'watch: started twice'
hold_lease
sh "$plugin/bin/watch.sh" --ensure || fail "ensure exited non-zero"
[ "$woken" -ge 1 ] || fail "the lease's watcher was not woken"
[ "$(cat "$(folder_of herdr)/socket" 2>/dev/null)" = "$HERDR_SOCKET_PATH" ] || fail "the server is not registered with the watcher"
sleep 1
[ "$(holder_of "$S/watch.lease")" = $$ ] || fail "a second watcher took the lease"
grep -q 'watching' "$S/watch.log" 2>/dev/null && fail "a second watcher started: $(cat "$S/watch.log")"
[ -s "$FAKE/sluice-calls" ] && fail "a second watcher ran sluice"
pass

fresh 'watch: two servers, one watcher'
printf '%s\n' "$testing95" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
for s in one two; do
  serve "$s"
  panes front-none
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure || fail "the ensure in $s exited non-zero"
done
within 20 'grep -q "^one.sock: pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls" &&
  grep -q "^two.sock: pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls"' ||
  fail "not both servers' panes have their tokens: $(calls)"
for s in one two; do
  grep -q "^$s.sock: pane list" "$FAKE/herdr-calls" || fail "server $s's panes were not read"
done
[ "$(grep -c 'watching (pid' "$S/watch.log")" = 1 ] || fail "not one watcher: $(cat "$S/watch.log")"
w=$(holder_of "$S/watch.lease")
kill -0 "$w" 2>/dev/null || fail "the lease's watcher is not running"
rm -f "$FAKE/hold"
pass

fresh 'watch: watcher gone'
# The lease names a watcher that has ended.
dead=$(sh -c 'echo $$')
printf '%s\n%s\n%s\n%s\n' "$dead" "pid:$dead" "$plugin" "$plugin_version" >"$S/watch.lease"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 'w=$(holder_of "$S/watch.lease") && [ "$w" != "$dead" ] && kill -0 "$w" 2>/dev/null' ||
  fail "no new watcher took the lease: $(cat "$S/watch.lease")"
within 15 'grep -q "overview --json" "$FAKE/sluice-calls"' || fail "the new watcher did not look for repositories"
kill "$w" 2>/dev/null
ends_within "$w" 10 || fail "the new watcher did not stop"
# Then one naming a pid another process has since: this one, with another
# start time.
printf '%s\n%s\n%s\n%s\n' $$ "start:1" "$plugin" "$plugin_version" >"$S/watch.lease"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
[ "$woken" = 0 ] || fail "the process given the lease's pid was signalled"
within 15 'w=$(holder_of "$S/watch.lease") && [ "$w" != $$ ] && kill -0 "$w" 2>/dev/null' ||
  fail "no new watcher took a lease naming another process: $(cat "$S/watch.lease")"
pass

fresh 'watch: a lock left by a watcher that died'
mkdir "$S/lease.lock"
sh -c 'echo $$ >"$1/pid"' _ "$S/lease.lock"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 >/dev/null 2>&1
grep -q "overview --json" "$FAKE/sluice-calls" || fail "a dead watcher's lock stopped the next one"
pass

fresh 'watch: a lock with no owner'
mkdir "$S/lease.lock"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 >/dev/null 2>&1
grep -q "overview --json" "$FAKE/sluice-calls" || fail "a lock with no owner stopped the watcher"
pass

fresh 'watch: a watcher stops only its own sources'
if needs_ps; then
  doc >"$FAKE/watch-work.jsonl"
  touch "$FAKE/hold"
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
  a=$!
  within 15 'sources_of "$a" | grep -q .' || fail "the first watcher started no source"
  # The first watcher stalls, and a reinstalled plugin's takes the lease
  # and starts its own source before the first wakes.
  kill -STOP "$a" $(pgrep -P "$a") 2>/dev/null
  again=$t/reinstalled
  cp -R "$plugin" "$again"
  HERDR_PLUGIN_ROOT=$again SLUICE_HERDR_TICK=1 sh "$again/bin/watch.sh" --ticks 60 2>/dev/null &
  b=$!
  within 30 '[ "$(holder_of "$S/watch.lease")" = "$b" ] && sources_of "$b" | grep -q .' ||
    fail "the second watcher did not take over and start its source"
  # The second watcher's sources, before the first one wakes.
  theirs=$(sources_of "$b")
  kill -CONT "$a" $(pgrep -P "$a") 2>/dev/null
  within 15 '! kill -0 "$a" 2>/dev/null' || fail "the stalled watcher did not give way"
  [ "$(holder_of "$S/watch.lease")" = "$b" ] || fail "the lease is not the second watcher's"
  for p in $theirs; do
    kill -0 "$p" 2>/dev/null || fail "the first watcher's stop ended the second's source $p"
  done
  rm -f "$FAKE/hold"
  kill "$b" 2>/dev/null
  wait 2>/dev/null
  pass
fi

fresh "watch: a killed watcher's sources are ended"
if needs_ps; then
  doc >"$FAKE/watch-work.jsonl"
  # A watcher killed outright left a source running under its pid.
  sleep 60 &
  orphan=$!
  dead=$(sh -c 'echo $$')
  mkdir -p "$S/run/$dead"
  printf '%s\t%s\n' "$orphan" "$(procs identity "$orphan")" >"$S/run/$dead/x.pids"
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
  if ! ends_within "$orphan" 20; then
    fail "the dead watcher's source still runs after ${took}s"
    kill "$orphan"
  fi
  [ -d "$S/run/$dead" ] && fail "the dead watcher's sources are still listed"
  pass
fi

fresh "watch: a source is known without ps's start times"
if needs_ps; then
  doc >"$FAKE/watch-work.jsonl"
  touch "$FAKE/hold"
  PATH="$here/fake-busybox-ps:$PATH" SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
  w=$!
  within 15 'grep -q " add $work\$" "$FAKE/stdin-lines"' || fail "no source started"
  # Looks at three ensures and their ticks, while the source is held open.
  looks "$here/fake-busybox-ps"
  [ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] ||
    fail "a new source at a look: $(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls") started"
  kill "$w" 2>/dev/null
  wait "$w" 2>/dev/null
  rm -f "$FAKE/hold"
  pass
fi

fresh 'watch: a watcher stopped in its source'"'"'s first second ends it'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# The source's identity is read only after ten seconds here.
SLUICE_HERDR_IDENTITY_DELAY=10 SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
w=$!
within 15 'grep -q " add $work\$" "$FAKE/stdin-lines"' || fail "no source started"
kill -TERM "$w"
wait "$w" 2>/dev/null
within 5 '[ -z "$(watching)" ]' || fail "the source outlived its watcher"
rm -f "$FAKE/hold"
for p in $(watching); do kill "$p" 2>/dev/null; done
pass

fresh 'watch: a pid kept alone and used again is left alone'
# A source that ended in its first second left only its pid, since taken
# by another process.
sleep 60 &
other_process=$!
dead=$(sh -c 'echo $$')
mkdir -p "$S/run/$dead"
printf '%s\tpid:%s\n' "$other_process" "$other_process" >"$S/run/$dead/x.pids"
printf '%s\tpid:%s:%s\n' "$other_process" "$other_process" "$dead" >"$S/run/$dead/y.pids"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
kill -0 "$other_process" 2>/dev/null || fail "a process that only has a recorded pid was killed"
kill "$other_process" 2>/dev/null
pass

fresh 'watch: without ps, one source and it is ended'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# No ps that runs, and on macOS no /proc: nothing can be told by identity.
PATH="$here/fake-no-ps:$PATH" SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 'grep -q " add $work\$" "$FAKE/stdin-lines"' || fail "no source started"
looks "$here/fake-no-ps"
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] ||
  fail "a new source at a look: $(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls") started"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
within 5 '[ -z "$(watching)" ]' || fail "the source outlived its watcher"
rm -f "$FAKE/hold"
pass

fresh "watch: without ps, an ended watcher's sources are left alone"
sleep 60 &
orphan=$!
dead=$(sh -c 'echo $$')
mkdir -p "$S/run/$dead"
printf '%s\tchild:pid:%s\n' "$orphan" "$orphan" >"$S/run/$dead/x.pids"
printf '%s\tpid:%s:%s\n' "$orphan" "$orphan" "$dead" >"$S/run/$dead/y.pids"
PATH="$here/fake-no-ps:$PATH" SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>"$t/log"
kill -0 "$orphan" 2>/dev/null || fail "a pid that could not be checked was ended"
kill "$orphan" 2>/dev/null
# Said where nothing can be read; with /proc (Linux) the pid-only lines
# are simply no one's.
if [ ! -r /proc/$$/stat ]; then
  [ "$(grep -c 'cannot tell an ended watcher' "$t/log")" = 1 ] || fail "not said once: $(cat "$t/log")"
fi
pass

fresh 'watch: a pid used again by another process is left alone'
# After a reboot the state is still there, and the pid is someone else's.
sleep 60 &
other_process=$!
dead=$(sh -c 'echo $$')
mkdir -p "$S/run/$dead"
printf '%s\t%s\n' "$other_process" "start:1" >"$S/run/$dead/x.pids"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
kill -0 "$other_process" 2>/dev/null || fail "a process that only has a recorded pid was killed"
kill "$other_process" 2>/dev/null
[ -d "$S/run/$dead" ] && fail "the dead watcher's sources are still listed"
pass

fresh 'watch: a reinstalled plugin replaces its watcher'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 30 2>/dev/null &
a=$!
sleep 2
# A reinstall is a new plugin directory, here with a segment that says so.
again=$t/reinstalled
cp -R "$plugin" "$again"
echo '# reinstalled' >>"$again/bin/segment.sh"
HERDR_PLUGIN_ROOT=$again SLUICE_HERDR_TICK=1 sh "$again/bin/watch.sh" --ensure
if ! ends_within "$a" 20; then
  fail "the old watcher kept running after ${took}s"
elif [ "$took" -gt 4 ]; then
  echo "note: the old watcher took ${took}s to end"
fi
within 20 '[ "$(sed -n 3p "$S/watch.lease")" = "$again" ]' || fail "the lease is not the reinstalled watcher's: $(cat "$S/watch.lease")"
within 20 'grep -q "# reinstalled" "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh"' || fail "the segment was not copied again"
rm -f "$FAKE/hold"
kill "$a" 2>/dev/null
pass

fresh 'watch: a plugin put back stops the host'"'"'s watcher'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
again=$t/installed
cp -R "$plugin" "$again"
HERDR_PLUGIN_ROOT=$again SLUICE_HERDR_TICK=1 sh "$again/bin/watch.sh" --ticks 60 2>"$t/log" &
a=$!
within 15 'sources_of "$a" | grep -q .' || fail "no source started"
theirs=$(sources_of "$a")
# A former version put back in the plugin's folder: its manifest is newer
# than the lease, which was written a second or more before.
sleep 1
touch "$again/herdr-plugin.toml"
ends_within "$a" 10 || fail "the watcher still runs ${took}s after its plugin was put back"
within 5 'ended $theirs' || fail "its sources outlived it"
[ -f "$S/watch.lease" ] && fail "it left its lease"
# The next server's event starts the watcher of the folder installed.
HERDR_PLUGIN_ROOT=$again SLUICE_HERDR_TICK=1 sh "$again/bin/watch.sh" --ensure
within 15 'b=$(holder_of "$S/watch.lease") && [ -n "$b" ] && [ "$b" != "$a" ] && kill -0 "$b" 2>/dev/null' ||
  fail "no watcher started after the plugin was put back"
# A plugin folder removed stops it too.
rm -f "$again/herdr-plugin.toml"
ends_within "$b" 10 || fail "the watcher still runs ${took}s after its plugin was removed"
rm -f "$FAKE/hold"
pass

fresh 'watch: herdr gone'
touch "$H/down"
# Sixty ticks, which a watcher that kept going takes a minute to run, so
# its end is waited for and the bound sits far from both.
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
w=$!
if ! ends_within "$w" 20; then
  fail "the watcher kept going ${took}s without herdr"
  kill "$w" 2>/dev/null
elif [ "$took" -gt 4 ]; then
  echo "note: the watcher took ${took}s to end without herdr"
fi
[ -f "$S/watch.lease" ] && fail "the watcher left its lease"
[ -e "$(folder_of herdr)/socket" ] && fail "the server is still registered"
pass

fresh 'watch: herdr stops'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# The server's socket, a plain file here, which a server removes as it stops.
sock=$HERDR_SOCKET_PATH
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
w=$!
within 15 'sources_of "$w" | grep -q .' || fail "no source started"
theirs=$(sources_of "$w")
rm -f "$sock"
ends_within "$w" 15 || fail "the watcher still runs ${took}s after its server stopped"
within 5 'ended $theirs' || fail "its sources outlived its server"
[ -f "$S/watch.lease" ] && fail "the watcher left its lease"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
pass

fresh 'watch: one of two servers stops'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
serve one
panes front-none
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
w=$!
within 15 'sources_of "$w" | grep -q .' || fail "no source started"
theirs=$(sources_of "$w")
serve two
panes front-none
sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q "^two.sock: pane list" "$FAKE/herdr-calls"' || fail "the second server is not served"
rm -f "$t/one.sock"
within 10 '[ ! -e "$(folder_of one)/socket" ]' || fail "the stopped server is still served"
# The server still running, which shows `3 ahead · clean`, is called at
# the minute's pass.
: >"$FAKE/herdr-calls"
within 15 'grep -q "^two.sock: pane list$" "$FAKE/herdr-calls"' || fail "the server still running is not called"
grep -q '^one.sock:' "$FAKE/herdr-calls" && fail "the stopped server is called: $(calls)"
kill -0 "$w" 2>/dev/null || fail "the watcher stopped with one of its servers"
for p in $theirs; do kill -0 "$p" 2>/dev/null || fail "its source $p stopped with one of its servers"; done
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
pass

fresh 'watch: another herdr server in its place'
printf '%s\n' "$testing95" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
sock=$HERDR_SOCKET_PATH
# Its ticks are long, so only the new server's startup has it look here.
PATH="$here/fake-logged:$PATH" SLUICE_HERDR_TICK=60 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
a=$!
within 15 'grep -q "pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls"' || fail "the first server's pane got no token"
# The server stops once the watcher is still, as a server that stops after
# it was told its tokens finds it. A tokens.sh keeps what it set after its
# calls, and the start's tick may run more than one: replaced while one
# still goes, the new server's registration would be told the old one's
# tokens were set.
settled || fail "the watcher did not settle"
theirs=$(sources_of "$a")
# The server stops and another starts at its path, with a socket of its
# own, and runs the plugin's startup.
rm -f "$sock"
: >"$sock"
: >"$FAKE/herdr-calls"
sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q "pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls"' ||
  fail "the new server was not given the tokens the old one had: $(calls)"
grep -q 'pane list' "$FAKE/herdr-calls" || fail "the new server's panes were not read"
kill -0 "$a" 2>/dev/null || fail "the watcher did not go on"
[ "$(holder_of "$S/watch.lease")" = "$a" ] || fail "the lease is not the watcher's: $(cat "$S/watch.lease")"
for p in $theirs; do kill -0 "$p" 2>/dev/null || fail "its source $p stopped"; done
rm -f "$FAKE/hold"
kill "$a" 2>/dev/null
wait "$a" 2>/dev/null
pass

fresh 'watch: idle ticks run nothing'
# Panes in a worktree with nothing to say and outside git: no token.
jq -n -c --arg other "$other" --arg home "$scratch/home" '{result: {panes: [
  {pane_id: "p2", tab_id: "t2", workspace_id: "w2", agent: "claude", cwd: $other, foreground_cwd: $other, focused: false},
  {pane_id: "p3", tab_id: "t1", workspace_id: "w1", cwd: $home, foreground_cwd: $home, focused: false}]}}' >"$H/panes.json"
doc >"$FAKE/watch-other.jsonl"
touch "$FAKE/hold"
PATH="$here/fake-logged:$PATH" SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 '[ -n "$(ls "$S"/docs/*.json 2>/dev/null)" ]' || fail "no document kept"
settled || fail "the watcher did not settle"
for f in herdr sluice jq git; do : >"$FAKE/$f-calls"; done
# Five ticks and more, with nothing changed.
sleep 6
for f in herdr sluice jq git; do
  [ -s "$FAKE/$f-calls" ] && fail "$f ran in idle ticks: $(cat "$FAKE/$f-calls")"
done
grep -q 'overview --json --watch --stdin' "$FAKE/sluice-calls" && fail "the source was started again"
# Its ticks still test the server: one that stops is seen at the next.
rm -f "$HERDR_SOCKET_PATH"
ends_within "$w" 5 || fail "the watcher still runs ${took}s after its server stopped"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
pass

fresh 'watch: a change wakes it'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# A tick of a minute: whatever follows within seconds came from a wake.
SLUICE_HERDR_TICK=60 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 'grep -q "pane report-metadata p1 .*--token sluice=3 ahead · clean" "$FAKE/herdr-calls"' || fail "no token at the start: $(calls)"
# The source prints a document with #95 under test, from p1's worktree.
: >"$FAKE/herdr-calls"
printf '%s\n' "$testing95" >"$FAKE/next-work.tmp" && mv "$FAKE/next-work.tmp" "$FAKE/next-work.jsonl"
within 5 'grep -q "pane report-metadata p1 .*--token sluice=#95 testing 3m/15m" "$FAKE/herdr-calls"' ||
  fail "the token did not follow the document: $(calls)"
# The popup asks for a repository the watcher does not read: it is looked
# for at once.
SLUICE_HERDR_DIR=$scratch/plain sh "$plugin/bin/status.sh" --once </dev/null >/dev/null
within 5 'grep -q " add $scratch/plain\$" "$FAKE/stdin-lines"' || fail "the popup's ask was not looked for: $(cat "$FAKE/stdin-lines")"
kill -0 "$w" 2>/dev/null || fail "the watcher stopped: $(cat "$t/log")"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
pass

fresh "watch: a former version's watchers give way"
doc >"$FAKE/watch-work.jsonl"
# A former version's watcher of the server in the environment, as one ran
# before record 0094: its lease, documents and sources in its server's
# folder. It gives way once its lease no longer names it, its source ended
# with it, as that version did.
former() {
  d=$(folder_of "$1")
  mkdir -p "$d/docs" "$d/told.d/x" "$d/checks" "$d/probe" "$d/want" "$d/run" "$d/failed"
  echo "its log" >"$d/watch.log"
  : >"$d/nudge"
  sh -c '
    d=$1
    sleep 300 &
    echo $! >"$d/source"
    mkdir -p "$d/run/$$"
    printf "%s\tx\n" "$!" >"$d/run/$$/x.pids"
    printf "%s %s\n" $$ "$(date +%s)" >"$d/watch.lease"
    while sleep 1; do
      [ "$(head -1 "$d/watch.lease" 2>/dev/null | cut -d" " -f1)" = $$ ] && continue
      kill "$(cat "$d/source")"
      rm -rf "$d/run/$$"
      exit 0
    done' _ "$d" &
  echo $! >"$t/former-$1"
  within 5 '[ -s "$d/watch.lease" ]' || fail "the former watcher of $1 did not start"
}
serve one
panes front-none
former one
serve two
panes front-none
former two
olds=$(cat "$(folder_of one)/source" "$(folder_of two)/source")
# One killed outright before, leaving a source it listed, which is ended
# where a process can be told by its identity.
if [ "$ps_runs" = true ]; then
  sleep 60 &
  orphan=$!
  dead=$(sh -c 'echo $$')
  mkdir -p "$S/servers/3/run/$dead"
  printf '%s\t%s\n' "$orphan" "$(procs identity "$orphan")" >"$S/servers/3/run/$dead/x.pids"
fi
serve one
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
for s in one two; do
  ends_within "$(cat "$t/former-$s")" 10 || fail "the former watcher of $s still runs after ${took}s"
done
within 5 'ended $olds' || fail "the former watchers' sources outlived them"
if [ "$ps_runs" = true ]; then
  ends_within "$orphan" 10 || fail "a killed former watcher's source still runs after ${took}s"
  kill "$orphan" 2>/dev/null
fi
serve two
sh "$plugin/bin/watch.sh" --ensure
within 10 '[ ! -e "$(folder_of one)/docs" ] && [ ! -e "$(folder_of two)/run" ]' || fail "the former folders are left: $(ls "$(folder_of one)")"
for s in one two; do
  [ "$(cat "$(folder_of "$s")/watch.log" 2>/dev/null)" = "its log" ] || fail "the former watch.log of $s is gone"
  [ -e "$(folder_of "$s")/watch.lease" ] && fail "the former lease of $s is left"
done
[ -e "$S/servers/3/run" ] && fail "the killed former watcher's folder is left"
[ "$(grep -c 'watching (pid' "$S/watch.log")" = 1 ] || fail "not one watcher: $(cat "$S/watch.log")"
: >"$FAKE/herdr-calls"
within 10 'grep -q "^one.sock: pane list" "$FAKE/herdr-calls" && grep -q "^two.sock: pane list" "$FAKE/herdr-calls"' ||
  fail "one watcher does not serve both servers: $(calls)"
for p in $olds; do kill "$p" 2>/dev/null; done
pass

# The documents kept, counted.
docs_kept() {
  set -- "$S"/docs/*.json
  if [ -e "$1" ]; then echo $#; else echo 0; fi
}

# The add lines on the sources' input for folder $1, counted.
adds_of() {
  grep -c " add $1\$" "$FAKE/stdin-lines" 2>/dev/null
}

# A document of the repository plain or third ($1), changed by the jq in $2.
doc_in() {
  case $1 in plain) dr=$plain_common ;; *) dr=$third_common ;; esac
  doc "${2:-.}" | jq -c --arg r "$dr" '.repository = $r'
}

# The mark of the repository whose git folder is $1: `none` or `records`.
mark_of() {
  cut -d' ' -f1 "$S/marks/$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; sum "$1"' "$plugin/bin/x" "$1")" 2>/dev/null
}

queued3='.branches[0].depth = 1 | .branches[0].submissions = [{id: 3, state: "queued", from: {agent: "claude", worktree: "w"}}]'

fresh 'watch: no configuration'
# A pane in the worktree work, of a repository with sluice's records, and
# one in plain, a repository with sluice and none.
panes_at p1 "$work" p4 "$scratch/plain"
doc_in plain '.branches = []' >"$FAKE/watch-plain.jsonl"
doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 12 '$work' queued)]" >"$FAKE/watch-work.jsonl"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "watch ran other than once: $(cat "$FAKE/sluice-calls")"
[ "$(adds_of "$work")" = 1 ] && [ "$(adds_of "$scratch/plain")" = 1 ] || fail "not both added: $(cat "$FAKE/stdin-lines")"
set -- "$S"/docs/*.json
[ "$#" = 1 ] && [ "$(jq -r .repository "$1")" = "$common" ] || fail "not the one repository with records kept: $*"
[ "$(mark_of "$plain_common")" = none ] || fail "plain is not marked as having no records"
pass

fresh 'watch: one source serves three repositories'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
doc_in third "$queued3" >"$FAKE/watch-third.jsonl"
panes_at p1 "$work" p4 "$scratch/plain" p5 "$scratch/third"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "not one source: $(cat "$FAKE/sluice-calls")"
[ "$(cut -d' ' -f1 "$FAKE/stdin-lines" | sort -u | wc -l | tr -d ' ')" = 1 ] || fail "not one input: $(cat "$FAKE/stdin-lines")"
for f in "$work" "$scratch/plain" "$scratch/third"; do
  [ "$(adds_of "$f")" = 1 ] || fail "$f not added once: $(cat "$FAKE/stdin-lines")"
done
[ "$(docs_kept)" = 3 ] || fail "not three documents kept: $(ls "$S/docs")"
pass

fresh 'watch: two worktrees of one repository add one folder'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc "$queued3" >"$FAKE/watch-other.jsonl"
panes_at p1 "$work" p2 "$other"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(grep -c ' add ' "$FAKE/stdin-lines")" = 1 ] || fail "not one add line: $(cat "$FAKE/stdin-lines")"
pass

fresh 'watch: a pane in a new repository adds it on the same input'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
touch "$FAKE/hold"
panes_at p1 "$work"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 '[ "$(adds_of "$work")" = 1 ]' || fail "work was not added"
panes_at p1 "$work" p4 "$scratch/plain"
sh "$plugin/bin/watch.sh" --ensure
within 10 '[ "$(adds_of "$scratch/plain")" = 1 ]' || fail "plain was not added: $(cat "$FAKE/stdin-lines")"
[ "$(cut -d' ' -f1 "$FAKE/stdin-lines" | sort -u | wc -l | tr -d ' ')" = 1 ] || fail "not one input: $(cat "$FAKE/stdin-lines")"
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "a second source started: $(cat "$FAKE/sluice-calls")"
rm -f "$FAKE/hold"
pass

fresh 'watch: panes of two servers in one repository'
for d in work other; do
  doc >"$FAKE/watch-$d.jsonl"
done
touch "$FAKE/hold"
# Server one's pane is in the worktree work, server two's in other: two
# worktrees of one repository.
serve one
panes_at p1 "$work"
serve two
panes_at p2 "$other"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
serve one
sh "$plugin/bin/watch.sh" --ensure
within 15 '[ -n "$(watching)" ]' || fail "no watch started"
within 10 'grep -q "^one.sock: pane list" "$FAKE/herdr-calls" && grep -q "^two.sock: pane list" "$FAKE/herdr-calls"' ||
  fail "not both servers' panes were read: $(calls)"
# Looked at again at each server's ensure and at the ticks.
for s in two one; do
  serve "$s"
  sh "$plugin/bin/watch.sh" --ensure
  sleep 2
done
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "watch ran other than once: $(cat "$FAKE/sluice-calls")"
[ "$(watching | wc -l | tr -d ' ')" = 1 ] || fail "not one watch runs: $(watching)"
[ "$(grep -c ' add ' "$FAKE/stdin-lines")" = 1 ] || fail "not one add line: $(cat "$FAKE/stdin-lines")"
rm -f "$FAKE/hold"
pass

fresh 'watch: the last pane leaves'
record_of 87 "$work" '["tests:t-cards"]'
{
  doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 87 '$work' testing)]"
  doc "$blamed87"
} >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
for s in one two; do
  serve "$s"
  panes_at p1 "$work"
done
# Long ticks: only the servers' ensures have it read their panes.
serve one
SLUICE_HERDR_TICK=60 sh "$plugin/bin/watch.sh" --ensure
serve two
sh "$plugin/bin/watch.sh" --ensure
within 15 'grep -q "notification show sluice: #87 blamed on main --body tests:t-cards failed." "$FAKE/herdr-calls"' ||
  fail "the blame was not told with its tests: $(calls)"
src=$(watching)
[ -n "$src" ] || fail "no watch runs"
# The repository's pane closes in server one: server two's is in it still.
serve one
panes_at p3 "$scratch/home"
: >"$FAKE/herdr-calls"
sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q "^one.sock: pane list" "$FAKE/herdr-calls"' || fail "server one's panes were not read again"
sleep 2
grep -q ' remove ' "$FAKE/stdin-lines" && fail "removed while a pane of another server is in it: $(cat "$FAKE/stdin-lines")"
[ -n "$(ls "$S"/docs/*.json 2>/dev/null)" ] || fail "the document went while a pane is in its repository"
# Then server two's: no pane of any server is in the repository.
serve two
panes_at p3 "$scratch/home"
sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q " remove $work\$" "$FAKE/stdin-lines"' || fail "its folder was not removed: $(cat "$FAKE/stdin-lines")"
within 5 '[ -z "$(find "$S/docs" "$S/told" -type f)" ]' ||
  fail "the repository's state is left: $(find "$S/docs" "$S/told" -type f)"
for p in $src; do kill -0 "$p" 2>/dev/null || fail "the source stopped with the repository's folder"; done
for v in --once '--dashboard --once'; do
  # shellcheck disable=SC2086 # the flags, split
  out=$(SLUICE_HERDR_DIR=$scratch/home sh "$plugin/bin/status.sh" $v </dev/null)
  printf '%s' "$out" | grep -qF -e "$common" -e '#87' && fail "status.sh $v still holds the repository: $out"
done
w=$(holder_of "$S/watch.lease")
kill -0 "$w" 2>/dev/null || fail "the watcher stopped with the repository's folder"
# A pane back in it is a first sight: the blame its first document holds
# is not told again.
doc "$blamed87" >"$FAKE/watch-work.jsonl"
: >"$FAKE/herdr-calls"
panes_at p1 "$work"
sh "$plugin/bin/watch.sh" --ensure
within 15 '[ -n "$(ls "$S"/docs/*.json 2>/dev/null)" ]' || fail "the repository was not read again"
[ "$(adds_of "$work")" = 2 ] || fail "not added again on the same input: $(cat "$FAKE/stdin-lines")"
settled
hasnt 'notification show'
rm -f "$FAKE/hold"
pass

fresh 'watch: a pane closes in one of two repositories'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
touch "$FAKE/hold"
panes_at p1 "$work" p4 "$scratch/plain"
SLUICE_HERDR_TICK=60 sh "$plugin/bin/watch.sh" --ensure
within 15 '[ "$(docs_kept)" = 2 ]' || fail "not both documents kept"
src=$(watching)
panes_at p1 "$work"
sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q " remove $scratch/plain\$" "$FAKE/stdin-lines"' || fail "plain was not removed: $(cat "$FAKE/stdin-lines")"
grep -q " remove $work\$" "$FAKE/stdin-lines" && fail "work was removed"
for p in $src; do kill -0 "$p" 2>/dev/null || fail "the source stopped"; done
within 5 '[ "$(docs_kept)" = 1 ]' || fail "plain's document is left: $(ls "$S/docs")"
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -q '#3' || fail "the popup no longer holds work: $out"
rm -f "$FAKE/hold"
pass

fresh 'watch: three servers, two repositories, four processes'
if needs_children; then
  # Two repositories with sluice's records: repo, in its worktree other,
  # and plain. Nothing in them gives a pane a token.
  doc >"$FAKE/watch-other.jsonl"
  doc_in plain >"$FAKE/watch-plain.jsonl"
  touch "$FAKE/hold"
  serve one
  panes_at p2 "$other"
  serve two
  panes_at p4 "$scratch/plain"
  serve three
  panes_at p2 "$other" p4 "$scratch/plain"
  for s in one two three; do
    serve "$s"
    SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
  done
  within 15 '[ "$(grep -c " add " "$FAKE/stdin-lines")" = 2 ]' || fail "not two folders: $(cat "$FAKE/stdin-lines")"
  w=$(holder_of "$S/watch.lease")
  settled || fail "the watcher did not settle"
  # The watcher's processes, a tick on: itself and its children, and the
  # reader's children; what a source's command starts is its own.
  got=
  for i in 1 2 3 4 5 6 7 8 9 10; do
    got=$(children_of "$w" | while read -r c_pid c_args; do
      case $c_args in
      *"overview --json --watch --stdin"*) echo command ;;
      *bin/document.sh*)
        echo reader
        children_of "$c_pid" | while read -r _ r_args; do echo "the reader's: $r_args"; done
        ;;
      "sleep "*) echo sleep ;;
      *) echo "other: $c_args" ;;
      esac
    done | sort | tr '\n' ' ')
    [ "$got" = 'command reader sleep ' ] && break
    sleep 1
  done
  [ "$got" = 'command reader sleep ' ] || fail "the watcher's processes beside it: $got"
  kill -0 "$w" 2>/dev/null || fail "the watcher is not running"
  rm -f "$FAKE/hold"
  pass
fi

fresh 'watch: ten repositories, four processes'
if needs_children; then
  set --
  for i in 0 1 2 3 4 5 6 7 8 9; do
    git init -q -b main "$t/r$i"
    git -C "$t/r$i" commit -q --allow-empty -m root
    doc | jq -c --arg r "$(cd "$t/r$i/.git" && pwd -P)" '.repository = $r' >"$FAKE/watch-r$i.jsonl"
    set -- "$@" "p$i" "$(cd "$t/r$i" && pwd -P)"
  done
  panes_at "$@"
  touch "$FAKE/hold"
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
  within 20 '[ "$(docs_kept)" = 10 ]' || fail "not ten documents: $(ls "$S/docs")"
  w=$(holder_of "$S/watch.lease")
  settled || fail "the watcher did not settle"
  got=
  for i in 1 2 3 4 5 6 7 8 9 10; do
    got=$(children_of "$w" | while read -r c_pid c_args; do
      case $c_args in
      *"overview --json --watch --stdin"*) echo command ;;
      *bin/document.sh*) echo reader ;;
      "sleep "*) echo sleep ;;
      *) echo "other: $c_args" ;;
      esac
    done | sort | tr '\n' ' ')
    [ "$got" = 'command reader sleep ' ] && break
    sleep 1
  done
  [ "$got" = 'command reader sleep ' ] || fail "the watcher's processes beside it: $got"
  [ "$(grep -c ' add ' "$FAKE/stdin-lines")" = 10 ] || fail "not ten folders: $(cat "$FAKE/stdin-lines")"
  rm -f "$FAKE/hold"
  pass
fi

fresh 'watch: a repository with records is read from its subscription'
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
panes_at p4 "$scratch/plain"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(mark_of "$plain_common")" = records ] || fail "plain is not marked as having records"
hold_lease
out=$(frame_of "$scratch/plain" --once)
printf '%s' "$out" | grep -q '#3' || fail "the popup does not show #3: $out"
grep -v 'overview --json --watch --stdin' "$FAKE/sluice-calls" | grep -q overview && fail "another overview ran: $(cat "$FAKE/sluice-calls")"
pass

fresh 'watch: a repository with no sluice has no records'
doc_in plain '.set_up = false | .branches = []' >"$FAKE/watch-plain.jsonl"
panes_at p4 "$scratch/plain"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(mark_of "$plain_common")" = none ] || fail "plain is not marked as having no records"
[ -z "$(ls "$S"/docs/ 2>/dev/null)" ] || fail "a document of it is kept: $(ls "$S/docs")"
hold_lease
out=$(frame_of "$scratch/plain" --once)
printf '%s' "$out" | grep -q '^No sluice records in this repository.$' || fail "the popup said: $out"
pass

fresh 'watch: a repository with no sluice is added again five minutes on'
doc_in plain '.set_up = false | .branches = []' >"$FAKE/watch-plain.jsonl"
touch "$FAKE/hold"
panes_at p4 "$scratch/plain"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 '[ "$(mark_of "$plain_common")" = none ]' || fail "plain is not marked as having no records"
# A look with no nudge, as the popup's ask makes, within five minutes.
want=$S/want/$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; sum "$1"' "$plugin/bin/x" "$plain_common")
echo "$scratch/plain" >"$want"
within 10 '[ ! -e "$want" ]' || fail "the ask was not taken up"
settled
[ "$(adds_of "$scratch/plain")" = 1 ] || fail "added again within five minutes: $(cat "$FAKE/stdin-lines")"
# Sluice is set up there, and five minutes have passed since the mark.
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
echo "none $((NOW - 301))" >"$S/marks/$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; sum "$1"' "$plugin/bin/x" "$plain_common")"
echo "$scratch/plain" >"$want"
within 10 '[ "$(adds_of "$scratch/plain")" = 2 ]' || fail "not added again five minutes on: $(cat "$FAKE/stdin-lines")"
within 10 '[ -n "$(ls "$S"/docs/*.json 2>/dev/null)" ]' || fail "the document that came is not kept"
rm -f "$FAKE/hold"
pass

fresh "watch: a repository's first submission shows at its next document"
doc_in plain '.branches = []' >"$FAKE/watch-plain.jsonl"
touch "$FAKE/hold"
panes_at p4 "$scratch/plain"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 '[ "$(mark_of "$plain_common")" = none ]' || fail "plain is not marked as having no records"
doc_in plain "$queued3" >"$FAKE/next-plain.tmp" && mv "$FAKE/next-plain.tmp" "$FAKE/next-plain.jsonl"
within 10 '[ -n "$(ls "$S"/docs/*.json 2>/dev/null)" ]' || fail "the next document is not kept"
out=$(frame_of "$scratch/plain" --once)
printf '%s' "$out" | grep -q '#3' || fail "the popup does not show #3: $out"
[ "$(adds_of "$scratch/plain")" = 1 ] || fail "added more than once: $(cat "$FAKE/stdin-lines")"
rm -f "$FAKE/hold"
pass

fresh "watch: a source of the user's"
cat >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml" <<'TOML'
# a stream of the user's own
[[sources]]
name = "build machine"
command = ["my-overview-stream", "app"]
TOML
jq -n -c '{schema: "sluice/overview@1", host: "build", repository: "/srv/app.git", at: "2026-10-16T19:45:10Z", branches: [], sessions: [], previews: []}' >"$FAKE/stream.jsonl"
touch "$FAKE/hold"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
w=$!
within 15 '[ -n "$(ls "$S"/docs/build--app-*.json 2>/dev/null)" ]' || fail "its document is not kept under its host and repository"
# No pane is in its repository: looked at again, it is left running, and
# its document kept.
looks "$here/fake"
[ "$(cat "$FAKE/stream-calls" 2>/dev/null)" = app ] || fail "the source did not run once with its argv: $(cat "$FAKE/stream-calls")"
[ -n "$(ls "$S"/docs/build--app-*.json 2>/dev/null)" ] || fail "its document went for want of panes"
kill -0 "$(sources_of "$w" | head -1)" 2>/dev/null || fail "the source was stopped"
grep -q 'overview --json --watch' "$FAKE/sluice-calls" && fail "the default source ran beside the user's"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
pass

fresh "watch: a source of the user's that names its folders"
cat >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml" <<'TOML'
[[sources]]
name = "local"
command = ["sluice", "overview", "--json", "--watch", "--stdin"]
folders = true
TOML
doc "$queued3" >"$FAKE/watch-work.jsonl"
panes_at p1 "$work"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "not run once: $(cat "$FAKE/sluice-calls")"
[ "$(adds_of "$work")" = 1 ] || fail "work not added once: $(cat "$FAKE/stdin-lines")"
pass

fresh 'watch: a source with no folders has its input closed'
cat >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml" <<'TOML'
[[sources]]
name = "build machine"
command = ["my-overview-stream", "app"]
TOML
panes_at p1 "$work"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
[ "$(cat "$FAKE/stream-calls" 2>/dev/null)" = app ] || fail "not run once: $(cat "$FAKE/stream-calls")"
[ "$(cat "$FAKE/stream-input" 2>/dev/null)" = closed ] || fail "its input read as: $(cat "$FAKE/stream-input")"
pass

fresh 'watch: the former key runs no source'
cat >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml" <<'TOML'
[[sources]]
name = "local"
command = ["sluice", "overview", "--json", "--watch"]
each_repository = true
TOML
panes_at p1 "$work"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>"$t/log"
said='sources.toml line 4: each_repository is not a key: use folders = true'
grep -qF "$said" "$t/log" || fail "the log does not say so: $(cat "$t/log")"
hold_lease
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -qF "$said" || fail "the popup said: $out"
grep -q overview "$FAKE/sluice-calls" && fail "a source ran: $(cat "$FAKE/sluice-calls")"
pass

fresh 'watch: configuration it cannot read'
printf 'ahead_check = false\ncommand = [\n  "a"]\n' >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml"
got=$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; . "$lib/sources.sh"; read_sources' "$plugin/bin/x" 2>"$t/err")
grep -q 'sources.toml:2:' "$t/err" || fail "the error does not name line 2: $(cat "$t/err")"
[ "$(printf '%s' "$got" | jq -r '.sources[0].command | join(" ")')" = "sluice overview --json --watch --stdin" ] || fail "not the default: $got"
for bad in 'folders = trueish' 'name = "a\q"' 'name = "a" "b"'; do
  printf '[[sources]]\ncommand = ["x"]\n%s\n' "$bad" >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml"
  got=$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; . "$lib/sources.sh"; read_sources' "$plugin/bin/x" 2>"$t/err")
  grep -q 'sources.toml:3:' "$t/err" || fail "$bad: the error does not name line 3: $(cat "$t/err")"
  [ "$(printf '%s' "$got" | jq -r '.sources[0].command | join(" ")' 2>/dev/null)" = "sluice overview --json --watch --stdin" ] || fail "$bad: not the default: $got"
done
printf 'ahead_check = false # off\n[[sources]]\nname = "a \\"b\\""\ncommand = ["x", "y"]\nfolders = true\n' >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml"
got=$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; . "$lib/sources.sh"; read_sources' "$plugin/bin/x")
[ "$got" = '{"ahead_check":false,"sources":[{"name":"a \"b\"","command":["x","y"],"folders":true}]}' ] || fail "read as $got"
pass

fresh 'watch: two documents'
feed "$(doc)" "$testing95" "$(doc '.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:45:00Z", event: "landed", ids: [95], to: "aaaaaaa"}]')"
f=$(ls "$S"/docs/*.json)
[ "$(jq -r '.branches[0].recent[0].ids[0]' "$f")" = 95 ] || fail "the state file is not the last document"
set -- "$S"/docs/*.tmp.*
[ -e "$1" ] && fail "a temporary file was left"
pass

fresh 'watch: a large document costs what a small one does'
# Every tool the reader may run, logged, beside the stand-in herdr and
# sluice, which log their own.
L=$(logging jq git mv rm cat cksum cut basename mkdir date sed awk grep head tail tr sort touch ln wc shasum sha256sum openssl)
started() { cat "$FAKE"/*-calls | grep -vc 'notification show'; }
feed "$(doc)"
small=$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 1 '$work' queued)]")
# Twenty submissions across three branches, and five conflicts not yet
# told, of submissions whose worktrees it carries.
large=$(doc '.branches = [range(3) as $i | .branches[0] + {branch: ["main", "release", "next"][$i],
    submissions: [range(101; 121) | select((. - 101) % 3 == $i)
      | {id: ., state: "queued", from: {agent: "claude", worktree: "w", path: (if . % 2 == 0 then $work else $other end)}}],
    recent: (if $i == 0 then [range(101; 106) | {schema: "sluice/log@1", at: "2026-10-16T19:44:\(. - 90)Z",
      event: "conflict", id: ., paths: ["a.rs"], with: []}] else [] end)}]
  | .branches |= map(.depth = (.submissions | length))')
for f in "$FAKE"/*-calls; do : >"$f"; done
printf '%s\n' "$small" | PATH="$L:$PATH" sh "$plugin/bin/document.sh" local
one=$(started)
for f in "$FAKE"/*-calls; do : >"$f"; done
printf '%s\n' "$large" | PATH="$L:$PATH" sh "$plugin/bin/document.sh" local
twenty=$(started)
[ "$(grep -c 'notification show sluice: #10[1-5] conflicts on main' "$FAKE/herdr-calls")" = 5 ] || fail "not five toasts: $(calls)"
[ "$one" -gt 0 ] || fail "nothing was logged"
[ "$twenty" -le "$one" ] || fail "the large document started $twenty processes, the small one $one: $(cat "$FAKE"/*-calls)"
[ "$(jq '[.[]] | length' "$S"/docs/*.paths)" = 20 ] || fail "not every submission's worktree learned: $(cat "$S"/docs/*.paths)"
pass

fresh 'watch: read by every server'
for s in one two; do
  serve "$s"
  panes front-none
done
serve one
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 12 '$work' testing)]
  | .branches[0].candidates = [{id: \"c1\", submissions: [12], revision: \"x\", state: \"testing\",
      started: \"2026-10-16T19:42:00Z\", limit: 900, targets: [\"#check-fast\"], executor: \"sandbox\", ran: 0, replayed: 0, failed: []}]")"
# One state file, the host's, whichever server's watcher kept it.
kept=$(find "$S" -name '*.json' -path '*/docs/*')
[ "$kept" = "$(ls "$S"/docs/*.json 2>/dev/null)" ] && [ "$(printf '%s\n' "$kept" | wc -l)" -eq 1 ] ||
  fail "not one state file in the host's state: $kept"
for s in one two; do
  serve "$s"
  out=$(SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" --once </dev/null)
  printf '%s' "$out" | grep -q '▶ testing main + #12 w' || fail "the popup in server $s shows: $out"
done
pass

fresh 'watch: a document missing fields'
feed "$testing95"
tokens
has '--token sluice=#95 testing'
feed "$(doc '.branches[0] |= del(.recent, .submissions, .candidates) | del(.sessions, .previews)')" 2>"$t/err"
for f in "$S"/docs/*.paths; do
  jq -e 'type == "object"' "$f" >/dev/null 2>&1 || fail "$(basename "$f") is no longer an object: $(cat "$f")"
done
tokens 2>>"$t/err"
has '--clear-token sluice'
grep -q 'jq: error' "$t/err" && fail "jq failed: $(cat "$t/err")"
pass

fresh 'watch: a document it cannot read'
feed "$testing95"
f=$(ls "$S"/docs/*.json)
cp "$f" "$t/before"
cp "${f%.json}.paths" "$t/before.paths"
feed "$(doc '.branches = "garbled"')" 2>"$t/err"
cmp -s "$f" "$t/before" || fail "a document it cannot read replaced the last good one"
cmp -s "${f%.json}.paths" "$t/before.paths" || fail "a document it cannot read changed the submissions learned"
grep -q 'cannot read' "$t/err" || fail "nothing logged: $(cat "$t/err")"
pass

fresh 'watch: what a held place failed is kept to its last line'
feed "$(doc '.branches[0].held_checkouts = [{place: "sluice/try/main/48", failed: "preview: stopping\nusage: scripts/preview up\n\n",
      tries: 4, at: "2026-10-16T19:40:00Z", stopped: true}, {place: 7}, "x"]
  | .branches += [{branch: "release", depth: 0}]
  | .sessions = [{worktree: $work, branch: "main", release_failed: {failed: "one\ntwo\n", tries: 1, stopped: true}}]
  | .previews = [{name: "docs", branch: "main", release_failed: {failed: "a\nb", tries: 3, stopped: true}}]')"
f=$(ls "$S"/docs/*.json)
out=$(jq -c '[.branches[] | .held_checkouts | map(.failed)]' "$f")
[ "$out" = '[["usage: scripts/preview up"],[]]' ] || fail "the branches' held checkouts read as $out"
[ "$(jq -r '.sessions[0].release_failed.failed' "$f")" = two ] || fail "a worktree's failure read as $(jq -c .sessions "$f")"
[ "$(jq -r '.previews[0].release_failed.failed' "$f")" = b ] || fail "a preview's failure read as $(jq -c .previews "$f")"
pass

fresh 'watch: source exits'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc_in plain "$queued3" >"$FAKE/watch-plain.jsonl"
touch "$FAKE/hold"
panes_at p1 "$work" p4 "$scratch/plain"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 '[ "$(grep -c " add " "$FAKE/stdin-lines")" = 2 ]' || fail "not both added: $(cat "$FAKE/stdin-lines")"
first=$(head -1 "$FAKE/watch-pids")
kill "$first"
within 10 '[ "$(watching | wc -l | tr -d " ")" = 1 ] && [ "$(watching)" != "$first" ]' || fail "no new source runs: $(cat "$FAKE/sluice-calls")"
next=$(watching)
within 5 '[ "$(grep -c "^$next add " "$FAKE/stdin-lines")" = 2 ]' || fail "the new source's input: $(cat "$FAKE/stdin-lines")"
for f in "$work" "$scratch/plain"; do
  grep -q "^$next add $f\$" "$FAKE/stdin-lines" || fail "$f not added again"
done
rm -f "$FAKE/hold"
pass

fresh "watch: a killed watcher's source ends"
if needs_ps; then
  doc "$queued3" >"$FAKE/watch-work.jsonl"
  touch "$FAKE/hold"
  panes_at p1 "$work"
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>/dev/null &
  a=$!
  within 15 '[ -n "$(watching)" ]' || fail "no source started"
  src=$(watching)
  kill -KILL "$a"
  wait "$a" 2>/dev/null
  # A pane event starts the next watcher.
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
  within 10 'ended $src' || fail "the killed watcher's source still runs"
  within 5 '[ "$(watching | wc -l | tr -d " ")" = 1 ]' || fail "not one source runs: $(watching)"
  rm -f "$FAKE/hold"
  pass
fi

fresh 'watch: the watcher only reads'
record_of 9 "$work" '["cli:test"]'
doc "$queued3" >"$FAKE/watch-work.jsonl"
doc '.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:00Z", event: "blamed", id: 9, combination: "c"}]' >>"$FAKE/watch-work.jsonl"
panes_at p1 "$work"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 2 2>/dev/null
grep -q 'notification show sluice: #9 blamed' "$FAKE/herdr-calls" || fail "the blame was not told: $(calls)"
if grep -v -e 'overview --json --watch --stdin' -e 'submit main --check --json' "$FAKE/sluice-calls" | grep -q .; then
  fail "other sluice commands ran: $(cat "$FAKE/sluice-calls")"
fi
pass

## the reader

fresh 'document: a blame the watcher did not see queued is told from the document'
hold_lease
panes front-none
kept
# The watcher's first document of the repository holds a blame it never saw
# queued; the next, the blame again: the toast and the jump read the
# document's folder and tests.
blamed9='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:00Z", event: "blamed", id: 9, combination: "c"}]'
record_of 9 "$work" '["cli:test"]'
feed "$(doc)" "$(doc "$blamed9")"
has 'notification show sluice: #9 blamed on main --body cli:test failed.'
: >"$FAKE/herdr-calls"
sh "$plugin/bin/jump.sh"
has 'agent focus p1'
grep -q status "$FAKE/sluice-calls" && fail "the reader ran sluice status: $(cat "$FAKE/sluice-calls")"
pass

fresh 'document: a conflict in the second repository is told from the document'
hold_lease
# One source serves repo and plain: a line with plain's folder holds a
# conflict of #4, whose folder the document gives.
conflict4='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:30Z", event: "conflict", id: 4, paths: ["b.rs"], with: []}]'
record_of 4 "$scratch/plain"
for d in "$(doc | jq -c --arg f "$work" '. + {folder: $f}')" \
  "$(doc_in plain | jq -c --arg f "$scratch/plain" '. + {folder: $f}')" \
  "$(doc_in plain "$conflict4" | jq -c --arg f "$scratch/plain" '. + {folder: $f}')"; do
  printf '%s\n' "$d"
done | sh "$plugin/bin/document.sh" local
has 'notification show sluice: #4 conflicts on main --body b.rs'
[ "$(jq -r '.["4"].path' "$S"/docs/*--plain-*.paths)" = "$scratch/plain" ] || fail "its folder not learned: $(cat "$S"/docs/*.paths)"
grep -q status "$FAKE/sluice-calls" && fail "the reader ran sluice status: $(cat "$FAKE/sluice-calls")"
pass

## herdr-status

fresh 'status: testing'
board=$(jq -c --arg repo "$common" '.repository = $repo' "$here/docs/board.json")
printf '%s\n' "$board" | sh "$plugin/bin/document.sh" local
frame_of "$work" --once >"$t/frame"
sed "s|$common|REPO|" "$t/frame" >"$t/got"
diff -u "$here/docs/board.frame" "$t/got" >"$t/diff" || fail "the frame differs: $(cat "$t/diff")"
pass

# #12 given a subject, #13 none from unit/forecast, under test for 130 s of
# 900 s; changed by the jq in $1.
named_doc() {
  doc ".branches[0].depth = 2 | .branches[0].submissions = [
      {id: 12, state: \"testing\", subject: \"Batch review cards\", from: {agent: \"claude\", worktree: \"unit/review\"}},
      {id: 13, state: \"testing\", from: {agent: \"claude\", worktree: \"unit/forecast\"}}]
    | .branches[0].candidates = [{id: \"c1\", submissions: [12, 13], revision: \"x\", state: \"testing\",
        started: \"2026-10-16T19:43:00Z\", limit: 900, targets: [\"#check-fast\"], executor: \"sandbox\", ran: 0, replayed: 0, failed: []}]
    | ${1:-.}"
}

fresh 'status: a combination names its submissions'
feed "$(named_doc)"
frame_of "$work" --once >"$t/frame"
grep -qxF '    ▶ testing main + #12 Batch review cards + #13 unit/forecast  2m10s / 15m' "$t/frame" ||
  fail "without tasks: $(cat "$t/frame")"
feed "$(named_doc '.branches[0].candidates[0].tasks = [{target: "a:test", state: "passed"}, {target: "b:test", state: "running"}]')"
frame_of "$work" --once >"$t/frame"
grep -qxF '    ▶ testing main + #12 Batch review cards + #13 unit/forecast  1/2 ▓▓▓▓▓░░░░░  2m10s / 15m' "$t/frame" ||
  fail "with tasks: $(cat "$t/frame")"
pass

fresh 'status: a landing of two names both'
feed "$(doc '.branches[0].recent = [{schema: "sluice/log@1", at: "2026-10-16T19:44:00Z", event: "landed", ids: [12, 13], to: "c41e8f2aa0"}]
  | .branches[0].recent_subjects = {"12": "Batch review cards"} | .branches[0].recent_from = {"12": "unit/review", "13": "unit/forecast"}')"
frame_of "$work" --once >"$t/frame"
grep -qxF '    ✓ 19:44 #12 Batch review cards, #13 unit/forecast landed as c41e8f2' "$t/frame" || fail "the landing reads: $(cat "$t/frame")"
pass

fresh 'status: a document that names no subjects names numbers'
feed "$(doc '.branches[0].recent = [{schema: "sluice/log@1", at: "2026-10-16T19:44:00Z", event: "landed", ids: [11], to: "9ecc34a1b2"}]')"
frame_of "$work" --once >"$t/frame"
grep -qxF '    ✓ 19:44 #11 landed as 9ecc34a' "$t/frame" || fail "the landing reads: $(cat "$t/frame")"
pass

fresh 'status: a subject with control characters stays on its line'
# #14 queued with the subject $1, a jq string.
queued14() { doc ".branches[0].depth = 1 | .branches[0].submissions = [{id: 14, state: \"queued\", subject: $1, from: {agent: \"herdr\", worktree: \"pull/41\"}}]"; }
feed "$(queued14 '"Herdr\npopup\u001b[31m"')"
frame_of "$work" --once >"$t/frame"
grep -qxF '    #14 Herdr popup [31m  queued    PR 41 herdr' "$t/frame" || fail "the row reads: $(cat "$t/frame")"
grep -q "$(printf '\033')" "$t/frame" && fail "the subject's escape reached the frame: $(cat -v "$t/frame")"
# Painted, the popup holds the same escapes as with a plain subject.
painted() { printf '%s\n' "$1" | jq -L "$plugin/lib" -r --argjson now "$NOW" 'include "words"; rows($now; true; null)[] | .text' | tr -cd '\033' | wc -c; }
[ "$(painted "$(queued14 '"Herdr\npopup\u001b[31m"')")" = "$(painted "$(queued14 '"Herdr popup"')")" ] ||
  fail "the subject changed the popup's colors"
pass

fresh 'status: tasks redraw with the version unmoved'
feed "$testing95"
(
  sleep 1
  feed "$(printf '%s' "$testing95" | jq -c '.branches[0].candidates[0].tasks = [{target: "a:test", state: "passed",
      started: "2026-10-16T19:42:00Z", ended: "2026-10-16T19:43:05Z", exit: 0}, {target: "b:test", state: "running"}]')"
  printf x
) | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
first=$(awk 'BEGIN{RS="\f"} NR==1' "$t/out")
second=$(awk 'BEGIN{RS="\f"} NR==2' "$t/out")
printf '%s' "$first" | grep -q '░' && fail "a combination whose runner says nothing shows a bar: $first"
printf '%s' "$second" | grep -q '▶ testing main + #95 w  1/2 ▓▓▓▓▓░░░░░  3m10s / 15m' || fail "the redraw does not count the tasks: $second"
printf '%s' "$second" | grep -q '^        ✓ a:test    1m05s$' || fail "the redraw does not list a:test: $second"
printf '%s' "$second" | grep -q '^        … b:test  running$' || fail "the redraw does not list b:test: $second"
pass

fresh 'status: a task list it cannot read is left out'
feed "$(printf '%s' "$testing95" | jq -c '.branches[0].candidates[0].tasks = ["a:test", {state: "passed"}, {target: "c:test", state: "waiting"}]')"
frame_of "$work" --once >"$t/frame"
grep -q '▶ testing main + #95 w  0/1 ░░░░░░░░░░' "$t/frame" || fail "the bad tasks were counted: $(cat "$t/frame")"
grep -q '^        · c:test  waiting$' "$t/frame" || fail "the good task is missing: $(cat "$t/frame")"
pass

fresh 'status: rows are links'
weird="$scratch/a b&c"
mkdir -p "$weird"
blame_logged='.branches[0].candidates = [{id: "c9", submissions: [9], revision: "x", state: "failed", targets: [], executor: null, ran: 1, replayed: 0, failed: [{task: "cli:test", log: "/logs/c9/cli:test.log"}]}]'
both="[$(sub 95 '$work' testing), $(sub 96 "\"$weird\"" queued)]"
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = $both | $blame_logged | $blamed87")"
out=$(SLUICE_HERDR_DIR=$work SLUICE_HERDR_LINKS=1 sh "$plugin/bin/status.sh" --once)
e=$(printf '\033')
printf '%s' "$out" | grep -qF "${e}]8;;sluice-herdr:focus?path=$(printf '%s' "$work" | jq -sRr @uri)${e}\\[1] #95" || fail "no focus link on #95: $(printf '%s' "$out" | cat -v)"
printf '%s' "$out" | grep -qF "sluice-herdr:focus?path=$(printf '%s' "$weird" | jq -sRr @uri)" || fail "the odd path is not encoded"
printf '%s' "$out" | grep -qF "sluice-herdr:log?path=%2Flogs%2Fc9%2Fcli%3Atest.log&task=cli%3Atest" || fail "no log link: $(printf '%s' "$out" | cat -v)"
# The odd path comes back whole: no pane is in it, which the toast names.
sh "$plugin/bin/link.sh" "sluice-herdr:focus?path=$(printf '%s' "$weird" | jq -sRr @uri)"
has "notification show sluice: no pane for $weird"
out=$(SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" --dashboard --once)
printf '%s' "$out" | grep -q '\[1\]' && fail "the dashboard numbers its rows"
pass

fresh 'status: links to paths that are not ASCII'
cafe="$scratch/café"
mkdir -p "$cafe"
logged='.branches[0].candidates = [{id: "c9", submissions: [9], revision: "x", state: "failed", targets: [], executor: null, ran: 1, replayed: 0, failed: [{task: "tést", log: "/logs/é/tést.log"}]}]'
one="[$(sub 97 "\"$cafe\"" queued)]"
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = $one | $logged | $blamed87")"
sh "$plugin/bin/link.sh" "sluice-herdr:focus?path=$(printf '%s' "$cafe" | jq -sRr @uri)" 2>"$t/err"
has "notification show sluice: no pane for $cafe"
HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"p1"}' sh "$plugin/bin/link.sh" "sluice-herdr:log?path=$(printf '%s' "/logs/é/tést.log" | jq -sRr @uri)" 2>>"$t/err"
has '--env SLUICE_HERDR_LOG=/logs/é/tést.log --env SLUICE_HERDR_TASK=tést'
# Again with gawk as awk in a UTF-8 locale, where a decoder not in the C
# locale makes %C3%A9 two characters.
if command -v gawk >/dev/null 2>&1; then
  mkdir -p "$t/gawk"
  ln -s "$(command -v gawk)" "$t/gawk/awk"
  : >"$FAKE/herdr-calls"
  PATH="$t/gawk:$PATH" LC_ALL=C.UTF-8 sh "$plugin/bin/link.sh" "sluice-herdr:focus?path=$(printf '%s' "$cafe" | jq -sRr @uri)" 2>>"$t/err"
  has "notification show sluice: no pane for $cafe"
  PATH="$t/gawk:$PATH" LC_ALL=C.UTF-8 HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"p1"}' sh "$plugin/bin/link.sh" "sluice-herdr:log?path=$(printf '%s' "/logs/é/tést.log" | jq -sRr @uri)" 2>>"$t/err"
  has '--env SLUICE_HERDR_LOG=/logs/é/tést.log'
else
  echo "note: $name: no gawk here, so the UTF-8 gawk case did not run"
fi
[ -s "$t/err" ] && fail "logged: $(cat "$t/err")"
pass

fresh 'status: a link focuses, opens, or is left alone'
blame_logged='.branches[0].candidates = [{id: "c9", submissions: [9], revision: "x", state: "failed", targets: [], executor: null, ran: 1, replayed: 0, failed: [{task: "cli:test", log: "/logs/c9/cli:test.log"}]}]'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$other' testing)] | $blame_logged")"
sh "$plugin/bin/link.sh" "sluice-herdr:focus?path=$(printf '%s' "$other" | jq -sRr @uri)"
has 'agent focus p2'
HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"p1"}' sh "$plugin/bin/link.sh" "sluice-herdr:log?path=%2Flogs%2Fc9%2Fcli%3Atest.log&task=cli%3Atest"
has 'plugin pane open --plugin agentplot.sluice --entrypoint log --placement split --direction right --target-pane p1 --env SLUICE_HERDR_LOG=/logs/c9/cli:test.log --env SLUICE_HERDR_TASK=cli:test'
: >"$FAKE/herdr-calls"
sh "$plugin/bin/link.sh" "sluice-herdr:log?path=%2Fetc%2Fpasswd" 2>"$t/err"
sh "$plugin/bin/link.sh" "sluice-herdr:focus?path=%2Felsewhere" 2>>"$t/err"
sh "$plugin/bin/link.sh" "sluice-herdr:run?cmd=rm" 2>>"$t/err"
hasnt 'plugin pane open'
hasnt 'focus'
[ "$(grep -c 'left alone\|not a link' "$t/err")" = 3 ] || fail "not each logged: $(cat "$t/err")"
pass

fresh 'status: the log split'
seq 1 300 | sed 's/^/line /' >"$t/cli.log"
out=$(printf q | SLUICE_HERDR_LOG=$t/cli.log SLUICE_HERDR_TASK=cli:test sh "$plugin/bin/log.sh")
printf '%s\n' "$out" | grep -qx 'log of cli:test' || fail "no heading: $out"
# Not a terminal: its last 200 lines, from line 101.
printf '%s\n' "$out" | grep -qx 'line 101' || fail "the tail does not start at line 101"
printf '%s\n' "$out" | grep -qx 'line 100' && fail "more than the last 200 lines"
printf '%s\n' "$out" | grep -qx 'line 300' || fail "the tail misses the last line"
# q closes it while its input is still open.
mkfifo "$t/keys"
{
  printf 'xq'
  sleep 8
} >"$t/keys" &
writer=$!
began=$(date +%s)
SLUICE_HERDR_LOG=$t/cli.log sh "$plugin/bin/log.sh" <"$t/keys" >/dev/null
[ $(($(date +%s) - began)) -lt 5 ] || fail "q did not close it"
kill "$writer" 2>/dev/null
out=$(printf q | SLUICE_HERDR_LOG=/nowhere/cli.log sh "$plugin/bin/log.sh")
printf '%s\n' "$out" | grep -qx 'cannot read /nowhere/cli.log' || fail "said: $out"
pass

fresh 'status: no control characters from a link or a document'
logged='.branches[0].candidates = [{id: "c9", submissions: [9], revision: "x", state: "failed", targets: [], executor: null, ran: 1, replayed: 0, failed: [{task: "cli:test", log: "/logs/c9/cli:test.log", system: "linux-arm64"}, {task: "evil\u001b]52;c;eA==\u0007", log: "/logs/c9/evil.log"}]}]'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$work' testing)] | $logged | $blamed87")"
HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"p1"}' sh "$plugin/bin/link.sh" "sluice-herdr:log?path=%2Flogs%2Fc9%2Fcli%3Atest.log&task=%1b]52;c;eA==%07"
has '--env SLUICE_HERDR_TASK=cli:test'
e=$(printf '\033')
grep -q "$e" "$FAKE/herdr-calls" && fail "an escape reached herdr: $(cat -v "$FAKE/herdr-calls")"
out=$(SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" --once)
printf '%s' "$out" | grep -q "$e" && fail "an escape reached the popup: $(printf '%s' "$out" | cat -v)"
printf '%s' "$out" | grep -q 'log of evil\]52;c;eA==' || fail "the cleaned task is not shown: $out"
printf '%s' "$out" | grep -q 'log of cli:test (linux-arm64): /logs/c9/cli:test.log' || fail "the system is not named: $out"
pass

fresh 'status: a number does what a click does'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$other' testing)]")"
printf '1' | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >/dev/null
has 'agent focus p2'
pass

fresh 'status: two hosts'
printf '%s\n' "$board" | sh "$plugin/bin/document.sh" local
printf '%s\n' "$board" | jq -c '.host = "build" | .repository = "/srv/repo/.git"' | sh "$plugin/bin/document.sh" "build machine"
frame_of "$work" --once >"$t/frame"
grep -q "^$common on test\$\|^$common on host-a\$" "$t/frame" || fail "no block for this host"
grep -q '^/srv/repo/.git on build$' "$t/frame" || fail "no block for the other host"
pass

fresh 'status: redraws on change'
feed "$testing95"
(
  sleep 1
  feed "$(doc '.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:45:00Z", event: "landed", ids: [95], to: "abcdef0"}]')"
  printf x
) | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
first=$(awk 'BEGIN{RS="\f"} NR==1' "$t/out")
second=$(awk 'BEGIN{RS="\f"} NR==2' "$t/out")
printf '%s' "$first" | grep -q '▶ testing main + #95 w' || fail "the first frame does not show #95 testing"
printf '%s' "$second" | grep -q '✓ 19:45 #95 landed as abcdef0' || fail "the redraw does not show the landing: $second"
printf '%s' "$second" | grep -q '▶ testing' && fail "the redraw still shows #95 under test"
pass

fresh 'status: close'
feed "$testing95"
printf '\033' | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >/dev/null || fail "Escape did not close it cleanly"
printf 'q' | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >/dev/null || fail "q did not close it cleanly"
pass

fresh 'status: all repositories'
feed "$testing95"
printf '%s\n' "$board" | jq -c '.repository = "/srv/elsewhere.git"' | sh "$plugin/bin/document.sh" "build" ""
printf 'a' | SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
awk 'BEGIN{RS="\f"} NR==1' "$t/out" | grep -q elsewhere && fail "the first frame shows another repository"
awk 'BEGIN{RS="\f"} NR==2' "$t/out" | grep -q '^/srv/elsewhere.git on host-a' || fail "a does not show every repository"
pass

fresh "status: the focused pane from herdr's context"
feed "$testing95"
ctx=$(jq -c --arg w "$work" '.focused_pane_cwd = $w' "$here/docs/context.json")
HERDR_PLUGIN_CONTEXT_JSON=$ctx sh "$plugin/bin/status.sh" --once | grep -q '▶ testing main + #95 w' || fail "the popup did not read the focused pane's repository"
pass

fresh 'status: a plain directory'
hold_lease
out=$(frame_of "$scratch/home" --once)
printf '%s' "$out" | grep -q '^No sluice records in this repository.$' || fail "said: $out"
echo "none $NOW" >"$S/marks/$(sh -c '. "$HERDR_PLUGIN_ROOT/lib/common.sh"; sum "$1"' "$plugin/bin/x" "$(cd "$scratch/plain/.git" && pwd -P)")"
out=$(frame_of "$scratch/plain" --once)
printf '%s' "$out" | grep -q '^No sluice records in this repository.$' || fail "in a repository without records it said: $out"
out=$(frame_of "$other" --once)
printf '%s' "$out" | grep -q '^Waiting for sluice' || fail "before the watcher read it, it said: $out"
[ -n "$(ls "$S/want")" ] || fail "the watcher was not asked to read it"
pass

# A combination of 30 tasks: 18 passed, a:failed
# failed, b:running running, 10 waiting; and 5 entries under RECENT.
tall=$(printf '%s\n' "$board" | jq -c '.branches[0].candidates[0].tasks = (
    [range(18) | {target: "p\(.):test", state: "passed", started: "2026-10-16T19:43:00Z", ended: "2026-10-16T19:43:\(10 + .)Z"}]
    + [{target: "a:failed", state: "failed", started: "2026-10-16T19:43:00Z", ended: "2026-10-16T19:44:00Z"}, {target: "b:running", state: "running"}]
    + [range(10) | {target: "w\(.):test", state: "waiting"}])
  | .branches[0].recent += [range(3) | {schema: "sluice/log@1", at: "2026-10-16T19:3\(.):00Z", event: "landed", ids: [80 + .], to: "abcdef0"}]')

fresh 'status: a tall frame fits the terminal'
printf '%s\n' "$tall" | sh "$plugin/bin/document.sh" local
printf q | LINES=20 SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
first=$(awk 'BEGIN{RS="\f"} NR==1' "$t/out")
[ "$(printf '%s\n' "$first" | wc -l)" -le 20 ] || fail "the frame is taller than 20 lines: $first"
printf '%s' "$first" | grep -q '^main at 9ecc34a · 3 submissions' || fail "the branch's heading went: $first"
printf '%s' "$first" | grep -q '▶ testing main + #12 Batch review cards + #13 plugin-board  19/30' || fail "the combination went: $first"
printf '%s' "$first" | grep -q '^        ✗ a:failed ' || fail "the failed task went: $first"
printf '%s' "$first" | grep -q '^        … b:running ' || fail "the running task went: $first"
printf '%s' "$first" | grep -q '^        … 18 passed, 10 waiting$' || fail "the tasks are not counted: $first"
printf '%s' "$first" | grep -q 'p0:test' && fail "a passed task is still listed: $first"
printf '%s' "$first" | grep -q '^q close' || fail "the footer went: $first"
# Room enough: every task listed, as before.
printf q | LINES=60 SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
awk 'BEGIN{RS="\f"} NR==1' "$t/out" | grep -q '^        · w9:test  *waiting$' || fail "with room the waiting tasks are not listed: $(cat "$t/out")"
# Shorter still: the bottom is cut and said.
printf q | LINES=12 SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
first=$(awk 'BEGIN{RS="\f"} NR==1' "$t/out")
[ "$(printf '%s\n' "$first" | wc -l)" -le 12 ] || fail "the frame is taller than 12 lines: $first"
printf '%s' "$first" | grep -q '^        ✗ a:failed ' || fail "at 12 lines the failed task went: $first"
printf '%s' "$first" | grep -q '^… [0-9]* more lines · ↓ scrolls$' || fail "the cut is not said: $first"
pass

fresh 'status: a tall frame scrolls'
printf '%s\n' "$tall" | sh "$plugin/bin/document.sh" local
# Down twice, Page Down twice, past the end, Up once, then q: six frames.
printf '\033[B\033[B\033[6~\033[6~\033[Aq' | LINES=12 SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
nth() { awk -v n="$1" 'BEGIN{RS="\f"} NR==n {sub(/^\n/, ""); print}' "$t/out"; }
second=$(nth 3)
printf '%s' "$second" | grep -q '^… 2 lines above · ↑ scrolls$' || fail "two lines down does not say so: $second"
printf '%s' "$second" | grep -q '^… [0-9]* more lines$' || fail "the lines below are not said: $second"
[ "$(printf '%s\n' "$second" | wc -l)" -le 12 ] || fail "a scrolled frame is taller than 12 lines: $second"
last=$(nth 5)
printf '%s' "$last" | grep -q 'more lines' && fail "the end of the rows still says more: $last"
printf '%s' "$last" | grep -q '✓ 19:32 #82 landed' || fail "the last row is not reached: $last"
[ "$(printf '%s\n' "$last" | wc -l)" -le 12 ] || fail "the last frame is taller than 12 lines: $last"
above=$(printf '%s' "$last" | sed -n 's/^… \([0-9]*\) lines above.*/\1/p')
up=$(nth 6)
printf '%s' "$up" | grep -q "^… $((above - 1)) lines above" || fail "Up from the end does not move one line: $up"
pass

fresh 'status: a branch with nothing queued is left out'
feed "$(printf '%s\n' "$board" | jq -c '.branches += [range(30) | {branch: "feature/b\(.)", at: "1234567", depth: 0, submissions: [], candidates: [], recent: []}]')"
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -q 'feature/b' && fail "an idle branch is shown: $out"
printf '%s' "$out" | grep -q '^main at 9ecc34a' || fail "the busy branch went: $out"
printf '%s' "$out" | grep -q '^30 other branches with nothing queued$' || fail "the idle branches are not counted: $out"
feed "$(printf '%s\n' "$board" | jq -c '.branches |= map(.depth = 0 | .submissions = [] | .candidates = [] | .recent = [])')"
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -q '^No branch has a queue.$' || fail "with every branch idle it said: $out"
printf '%s' "$out" | grep -q 'other branch' && fail "with every branch idle it counts them: $out"
pass

fresh 'status: an idle branch is shown a quarter of an hour after its last event'
fresh_doc() { printf '%s\n' "$board" | jq -c --arg at "$1" '.branches += [{branch: "feature/done", at: "1234567", depth: 0, submissions: [], candidates: [],
  recent: [{schema: "sluice/log@1", at: $at, event: "landed", ids: [77], to: "abcdef0"}]}]'; }
feed "$(fresh_doc 2026-10-16T19:40:00Z)"
frame_of "$work" --once | grep -q '^feature/done at 1234567' || fail "a landing five minutes old is not shown"
feed "$(fresh_doc 2026-10-16T19:20:00Z)"
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -q 'feature/done' && fail "a landing 25 minutes old is still shown: $out"
pass

fresh 'status: a pane outside git shows every repository'
hold_lease
feed "$testing95"
out=$(frame_of "$scratch/home" --once)
printf '%s' "$out" | grep -qF "$scratch/home is in no git repository. Every repository held:" || fail "said: $out"
printf '%s' "$out" | grep -q '▶ testing main + #95 w' || fail "the held repository is not shown: $out"
printf '%s' "$out" | grep -q 'No sluice records' && fail "it still says no records: $out"
pass

fresh 'status: without sluice'
mkdir -p "$t/bin"
for c in git jq awk cksum; do ln -s "$(command -v "$c")" "$t/bin/$c"; done
ln -s "$here/fake/herdr" "$t/bin/herdr"
PATH="$t/bin:/usr/bin:/bin" SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>"$t/log"
grep -q 'sluice is not on PATH' "$t/log" || fail "the log does not say so: $(cat "$t/log")"
out=$(PATH="$t/bin:/usr/bin:/bin" frame_of "$work" --once)
printf '%s' "$out" | grep -q 'sluice is not on PATH' || fail "the popup said: $out"
pass

fresh 'status: the dashboard'
feed "$testing95"
printf '%s\n' "$board" | jq -c '.repository = "/srv/elsewhere.git"' | sh "$plugin/bin/document.sh" "build" ""
out=$(frame_of "$scratch/home" --dashboard --once)
printf '%s' "$out" | grep -q '^/srv/elsewhere.git' || fail "the dashboard misses a repository"
printf '%s' "$out" | grep -q "^$common" || fail "the dashboard misses this repository"
pass

fresh 'status: segment'
printf '%s\n' "$board" | jq -c --arg r "$common" '.repository = $r' | sh "$plugin/bin/document.sh" local
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
seg=$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh
# The copy stands alone: nothing in it names the plugin's own directory.
grep -rqF "$plugin" "$HERDR_PLUGIN_STATE_DIR/segment" && fail "the copy names the plugin's directory"
out=$(cd "$work" && env -u HERDR_PLUGIN_ROOT -u HERDR_PLUGIN_STATE_DIR sh "$seg")
[ "$out" = 'sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board 3/5 ▓▓▓▓▓▓░░░░ 2m10s/15m · #14 Herdr popup waiting · last #9 unit/sidebar blamed on 4f1c2aa' ] || fail "the segment printed: $out"
feed "$(doc '.host = "host-a"')"
out=$(cd "$work" && sh "$seg")
[ -z "$out" ] || fail "with no queue it printed: $out"
pass

fresh 'status: the segment names what waits and the last event'
feed "$(doc '.branches[0].depth = 2 | .branches[0].submissions = [
    {id: 14, state: "queued", subject: "Herdr popup", from: {agent: "herdr", worktree: "pull/41"}},
    {id: 15, state: "queued", from: {agent: "claude", worktree: "unit/toasts"}}]
  | .branches[0].recent = [{schema: "sluice/log@1", at: "2026-10-16T19:41:00Z", event: "landed", ids: [11], to: "9ecc34a1b2"}]
  | .branches[0].recent_subjects = {"11": "Fix the dates"} | .branches[0].recent_from = {"11": "unit/dates"}')"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
[ "$out" = 'sluice main · 2 submissions · #14 Herdr popup, #15 unit/toasts waiting · last #11 Fix the dates landed as 9ecc34a' ] ||
  fail "the segment printed: $out"
pass

fresh "status: another repository's document leaves the segment alone"
printf '%s\n' "$board" | jq -c --arg r "$common" '.repository = $r' | sh "$plugin/bin/document.sh" local
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
# Another repository's document that is not JSON, beside the focused one's.
echo '{"schema": "sluice/overview@1", "host": "host-a", "repo' >"$S/docs/host-a--other-1.json"
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
[ "$out" = 'sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board 3/5 ▓▓▓▓▓▓░░░░ 2m10s/15m · #14 Herdr popup waiting · last #9 unit/sidebar blamed on 4f1c2aa' ] ||
  fail "beside another repository's broken document the segment printed: $out"
pass

# The board with agents' checks: e2e-run running a command, fix-cards its
# checks with 3 of 8 tasks ended, plugin-board's ended 2 minutes ago with
# 14 passed and reused, stale's 45 minutes ago, and a worktree that only
# has main checked out; two of the host's six runs are agents'.
checks_board=$(printf '%s\n' "$board" | jq -c '.room = {bound: 6, queue: 1, sessions: 2, reserve: 0}
  | .sessions = [
      {worktree: "/work/sluice/e2e-run", branch: "main", ports: {}, last_run: "2026-10-16T19:44:40Z", removed: false,
       run: {started: "2026-10-16T19:44:40Z", command: "./scripts/e2e.sh"}},
      {worktree: "/work/sluice/fix-cards", branch: "main", ports: {web: "4310"}, last_run: "2026-10-16T19:44:00Z", removed: false,
       run: {started: "2026-10-16T19:44:00Z",
             tasks: ([range(3) | {target: "t\(.):test", state: "passed"}] + [range(5) | {target: "w\(.):test", state: "waiting"}])}},
      {worktree: "/work/sluice/plugin-board", branch: "main", ports: {}, last_run: "2026-10-16T19:40:00Z", removed: false,
       last_result: {at: "2026-10-16T19:43:10Z", head: "abc1234", kind: "checks", outcome: "pass", recorded: true, reused: true,
                     tasks: [range(14) | {target: "p\(.):test", state: "passed"}]}},
      {worktree: "/work/sluice/stale", branch: "main", ports: {},
       last_result: {at: "2026-10-16T19:00:00Z", head: "abc1234", kind: "checks", outcome: "pass", recorded: true, reused: true}},
      {worktree: "/work/sluice/held", branch: "main", holds_queued_branch: "main"}]')

fresh "status: agents' checks are load on the host"
printf '%s\n' "$checks_board" | jq -c --arg repo "$common" '.repository = $repo' | sh "$plugin/bin/document.sh" local
frame_of "$work" --once >"$t/frame"
sed "s|$common|REPO|" "$t/frame" >"$t/got"
diff -u "$here/docs/checks.frame" "$t/got" >"$t/diff" || fail "the frame differs: $(cat "$t/diff")"
grep -Eq 'testing.*e2e-run|#.*fix-cards' "$t/frame" && fail "a check is shown as a submission or as testing: $(cat "$t/frame")"
pass

fresh 'status: a finished check says what it is worth'
finished() {
  # $1 the last_result's fields, as jq
  feed "$(printf '%s\n' "$checks_board" | jq -c --arg repo "$common" ".repository = \$repo | .sessions[2].last_result |= ($1)")"
  frame_of "$work" --once | grep '^    plugin-board → main '
}
out=$(finished '.recorded = false | .reused = false')
[ "$out" = '    plugin-board → main 14 passed · not kept: uncommitted changes' ] || fail "not kept reads: $out"
out=$(finished '.reused = false')
[ "$out" = '    plugin-board → main 14 passed' ] || fail "kept, not reused, reads: $out"
out=$(finished '.outcome = "fail" | .reused = false | .tasks[0:2] |= map(.state = "failed")')
[ "$out" = '    plugin-board → main 2 of 14 failed' ] || fail "a failure reads: $out"
out=$(finished '.kind = "command" | .outcome = "fail" | .recorded = false | del(.reused, .tasks)')
[ "$out" = '    plugin-board → main failed' ] || fail "a command's failure reads: $out"
pass

fresh 'status: the band says when checks hold the queue back'
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
seg=$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh
band='sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board 3/5 ▓▓▓▓▓▓░░░░ 2m10s/15m · #14 Herdr popup waiting · last #9 unit/sidebar blamed on 4f1c2aa'
segment_with() {
  # $1 the document's changes, as jq
  feed "$(printf '%s\n' "$board" | jq -c --arg r "$common" ".repository = \$r | $1")"
  (cd "$work" && sh "$seg")
}
out=$(segment_with '.branches[0].window = 2 | .room = {bound: 2, queue: 1, sessions: 1, reserve: 0}')
[ "$out" = "$band · 1 slot to an agent's checks" ] || fail "held back by one check it printed: $out"
out=$(segment_with '.branches[0].window = 2 | .room = {bound: 3, queue: 1, sessions: 2, reserve: 0}')
[ "$out" = "$band · 2 slots to agents' checks" ] || fail "held back by two checks it printed: $out"
out=$(segment_with '.branches[0].window = 2 | .room = {bound: 3, queue: 1, sessions: 1, reserve: 0}')
[ "$out" = "$band" ] || fail "with a run free it printed: $out"
out=$(segment_with '.room = {bound: 2, queue: 1, sessions: 1, reserve: 0}')
[ "$out" = "$band" ] || fail "with the window full it printed: $out"
# #14's combination passed: it waits on no run.
out=$(segment_with '.branches[0].window = 2 | .room = {bound: 2, queue: 1, sessions: 1, reserve: 0}
  | .branches[0].submissions |= map(if .id == 14 then .state = "passed" else . end)')
[ "$out" = "$band" ] || fail "with nothing queued it printed: $out"
pass

fresh "status: branches keep the overview's order"
feed "$(printf '%s\n' "$board" | jq -c '.branches = [(.branches[0] | .branch = "release"),
  {branch: "feature/a", at: "1234567", depth: 0, submissions: [], candidates: [], recent: []}, .branches[0]]')"
out=$(frame_of "$work" --once)
[ "$(printf '%s\n' "$out" | sed -n -E 's/^(release|main) at .*/\1/p' | tr '\n' ' ')" = 'release main ' ] ||
  fail "the branches are not in the document's order: $out"
printf '%s' "$out" | grep -q '^1 other branch with nothing queued$' || fail "the idle branch is not counted: $out"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
case $out in "sluice release · "*"  |  sluice main · "*) ;; *) fail "the segment is not in the document's order: $out" ;; esac
printf '\033[B\033[Bq' | LINES=12 SLUICE_HERDR_DIR=$work sh "$plugin/bin/status.sh" >"$t/out"
third=$(awk 'BEGIN{RS="\f"} NR==3 {sub(/^\n/, ""); print}' "$t/out")
printf '%s' "$third" | grep -q '^… 2 lines above · ↑ scrolls$' || fail "it does not scroll: $third"
[ "$(printf '%s\n' "$third" | wc -l)" -le 12 ] || fail "a scrolled frame is taller than 12 lines: $third"
pass

focused_p1=$(jq -c --arg w "$work" '.focused_pane_cwd = $w | .focused_pane_id = "p1"' "$here/docs/context.json")

fresh "status: the focused pane's named branch comes first"
feed "$(printf '%s\n' "$board" | jq -c '.branches = [(.branches[0] | .branch = "release"),
  {branch: "feature/a", at: "1234567", depth: 0, submissions: [], candidates: [], recent: []}, .branches[0]]')"
named p1 feature/a
out=$(HERDR_PLUGIN_CONTEXT_JSON=$focused_p1 sh "$plugin/bin/status.sh" --once)
[ "$(printf '%s\n' "$out" | sed -n -E 's/^(release|main|feature\/a) at .*/\1/p' | tr '\n' ' ')" = 'feature/a release main ' ] ||
  fail "the named branch is not first: $out"
printf '%s' "$out" | grep -q 'with nothing queued$' && fail "the named branch is counted with nothing queued: $out"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
case $out in "sluice release · "*"  |  sluice main · "*) ;; *) fail "the segment moved: $out" ;; esac
pass

fresh 'status: a named branch with no records'
feed "$testing95"
named p1 hotfix
out=$(HERDR_PLUGIN_CONTEXT_JSON=$focused_p1 sh "$plugin/bin/status.sh" --once)
first=$(printf '%s\n' "$out" | grep -E -m1 '^(hotfix|main)( at | · )')
[ "$first" = 'hotfix · no records' ] || fail "the first branch line is '$first': $out"
pass

# main's checkout sluice/try/main/48, held after four failed releases;
# a worktree of main held after its one; the rows they draw.
held48='.branches[0].held_checkouts = [{place: "sluice/try/main/48", failed: "the release command ended exit status: 2\nusage: scripts/preview up",
    tries: 4, at: "2026-10-16T19:40:00Z", stopped: true}]'
held_unit_a='.sessions += [{worktree: "/work/sluice/unit-a", branch: "main", ports: {},
    release_failed: {failed: "a\nrm: cannot remove .stack", tries: 1, at: "2026-10-16T19:41:00Z", stopped: true}}]'
rows48='  RELEASE FAILED
    ✗ sluice/try/main/48 held · failed 4 times · usage: scripts/preview up
        sluice release sluice/try/main/48 · --accept-failure frees it without'
# The rows under main's heading, $1 of them, in the popup of the documents fed.
under_main() {
  frame_of "$work" --once | sed -n '/^main at /,$p' | sed -n "2,$(($1 + 1))p"
}

fresh 'status: a held checkout'
feed "$(doc "$held48 | .branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$work' queued)]")"
out=$(under_main 4)
[ "$out" = "$rows48
  QUEUED" ] || fail "under main the popup shows: $out"
pass

fresh 'status: a held worktree'
feed "$(doc "$held_unit_a")"
out=$(under_main 3)
[ "$out" = '  RELEASE FAILED
    ✗ worktree unit-a held · failed once · rm: cannot remove .stack
        sluice release --worktree /work/sluice/unit-a · --accept-failure frees it without' ] ||
  fail "under main the popup shows: $out"
pass

fresh "status: another host's held checkout"
feed "$(doc "$held48 | .host = \"vega\" | .branches[0].held_checkouts += [.branches[0].held_checkouts[0]
  | .place = \"sluice/try/main/50\" | .host = \"vega\"] | .branches[0].held_checkouts[0].host = \"deneb\"")"
out=$(under_main 5)
[ "$out" = '  RELEASE FAILED
    ✗ sluice/try/main/48 held on deneb · failed 4 times · usage: scripts/preview up
        on deneb: sluice release sluice/try/main/48 · --accept-failure frees it without
    ✗ sluice/try/main/50 held · failed 4 times · usage: scripts/preview up
        sluice release sluice/try/main/50 · --accept-failure frees it without' ] ||
  fail "under main the popup shows: $out"
pass

fresh 'status: an idle branch with a held place'
feed "$(doc "$held48 | $release_idle")"
out=$(frame_of "$work" --once)
[ "$(under_main 3)" = "$rows48" ] || fail "main is not shown with its held checkout: $out"
printf '%s' "$out" | grep -q '^1 other branch with nothing queued$' || fail "release is not counted as idle: $out"
printf '%s' "$out" | grep -q 'No branch has a queue' && fail "it says no branch has a queue: $out"
pass

fresh 'status: a checkout still being tried'
feed "$(doc "$held48 | .branches[0].held_checkouts[0] |= (.stopped = false | .tries = 1)
  | .branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$work' queued)]")"
out=$(frame_of "$work" --once)
printf '%s' "$out" | grep -q 'RELEASE FAILED\|sluice/try/main/48' && fail "a checkout still tried is shown: $out"
pass

fresh 'status: a document from before held places'
feed "$(doc '.branches[0].depth = 1 | .branches[0].submissions = [{id: 95, state: "queued", from: {agent: "claude", worktree: "w"}}]')"
frame_of "$work" --once >"$t/old"
feed "$(doc '.branches[0].depth = 1 | .branches[0].submissions = [{id: 95, state: "queued", from: {agent: "claude", worktree: "w"}}]
  | .branches[0].held_checkouts = [] | .sessions = [{worktree: "/w/x", branch: "main", ports: {}}]')"
frame_of "$work" --once >"$t/new"
grep -q 'RELEASE FAILED' "$t/old" && fail "an older document shows RELEASE FAILED: $(cat "$t/old")"
sed '/worktree held/d; /worktrees held/d' "$t/old" >"$t/old.rows"
sed '/worktree held/d; /worktrees held/d' "$t/new" >"$t/new.rows"
cmp -s "$t/old.rows" "$t/new.rows" || fail "an older document draws otherwise: $(diff "$t/old.rows" "$t/new.rows")"
pass

fresh 'status: the segment counts held places'
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
band='sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board 3/5 ▓▓▓▓▓▓░░░░ 2m10s/15m · #14 Herdr popup waiting · last #9 unit/sidebar blamed on 4f1c2aa'
feed "$(printf '%s\n' "$board" | jq -c --arg r "$common" ".repository = \$r | $held48")"
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
[ "$out" = "$band · 1 held: release failed" ] || fail "beside a queue the segment printed: $out"
# The same host's document, in place of the board's.
feed "$(doc "$held48 | $held_unit_a | .host = \"host-a\"")"
out=$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
[ "$out" = 'sluice main · 0 submissions · 2 held: release failed' ] || fail "with nothing queued the segment printed: $out"
pass

fresh 'status: setup shows, never edits'
before=$(find "$HERDR_PLUGIN_CONFIG_DIR" "$scratch/home" -type f | sort)
# A state directory outside the home directory is named whole.
out=$(HOME=$scratch/home sh "$plugin/bin/setup.sh" </dev/null)
for want in 'key = "prefix+m"' 'command = "agentplot.sluice.status"' '"$sluice"' '"$sluice_q"' \
  "sh '$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh'" 'interval_seconds = 5, timeout_seconds = 30 }' \
  "set up sluice's herdr plugin" 'herdr config check'; do
  printf '%s' "$out" | grep -qF -- "$want" || fail "setup does not show $want"
done
[ "$(find "$HERDR_PLUGIN_CONFIG_DIR" "$scratch/home" -type f | sort)" = "$before" ] || fail "setup wrote a file"
[ -s "$FAKE/herdr-calls" ] && fail "setup called herdr: $(calls)"
pass

fresh 'status: one tab-bar line for every host'
lines=
for h in mac box; do
  home=$t/$h
  st=$home/.local/state/herdr/plugins/agentplot.sluice
  mkdir -p "$st"
  printf '%s\n' "$board" | jq -c --arg r "$common" --arg h "$h" '.repository = $r | .branches[0].branch = "main-\($h)"' |
    HERDR_PLUGIN_STATE_DIR=$st sh "$plugin/bin/document.sh" local
  HOME=$home HERDR_PLUGIN_STATE_DIR=$st SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
  line=$(HOME=$home HERDR_PLUGIN_STATE_DIR=$st sh "$plugin/bin/setup.sh" </dev/null | grep 'type = "command"')
  lines="$lines$line
"
  cmd=$(printf '%s' "$line" | sed -n "s/.*command = '\(.*\)', interval_seconds.*/\1/p")
  [ "$cmd" = 'sh "$HOME/.local/state/herdr/plugins/agentplot.sluice/segment/bin/segment.sh"' ] || fail "on $h setup printed: $line"
  # As herdr runs it: through a login shell, in the focused pane's directory.
  out=$(cd "$work" && env -u HERDR_PLUGIN_ROOT -u HERDR_PLUGIN_STATE_DIR HOME="$home" /bin/sh -lc "$cmd")
  case $out in "sluice main-$h · 3 submissions · "*) ;; *) fail "on $h the line printed: $out" ;; esac
done
[ "$(printf '%s' "$lines" | sort -u | wc -l)" -eq 1 ] || fail "the hosts' lines differ: $lines"
pass

fresh "status: the segment is the server's"
home=$t/server
st=$home/.local/state/herdr/plugins/agentplot.sluice
mkdir -p "$st"
printf '%s\n' "$board" | jq -c --arg r "$common" '.repository = $r' |
  HERDR_PLUGIN_STATE_DIR=$st sh "$plugin/bin/document.sh" local
HOME=$home HERDR_PLUGIN_STATE_DIR=$st SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
cmd=$(HOME=$home HERDR_PLUGIN_STATE_DIR=$st sh "$plugin/bin/setup.sh" </dev/null | sed -n "s/.*command = '\(.*\)', interval_seconds.*/\1/p")
# As herdr runs it on the server, whatever host the client is on: the
# server's home and PATH, nothing else, not even its socket, so the clock
# is the host's own and the time under test is however long ago the
# board's run started.
out=$(cd "$work" && env -i HOME="$home" PATH="$PATH" /bin/sh -lc "$cmd")
case $out in 'sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board 3/5 ▓▓▓▓▓▓░░░░ '*'/15m · #14 Herdr popup waiting · last #9 unit/sidebar blamed on 4f1c2aa') ;;
*) fail "with only the server's environment it printed: $out" ;;
esac
pass

fresh 'status: the segment is the same in every server'
printf '%s\n' "$board" | jq -c --arg r "$common" '.repository = $r' | sh "$plugin/bin/document.sh" local
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 1 2>/dev/null
# Two servers with panes in the repository, the segment run in each with
# its own socket, as each server's tab bar runs it.
lines=
for s in one two; do
  serve "$s"
  panes front-none
  lines="$lines$(cd "$work" && sh "$HERDR_PLUGIN_STATE_DIR/segment/bin/segment.sh")
"
done
[ "$(printf '%s' "$lines" | sort -u | wc -l)" -eq 1 ] || fail "the servers' lines differ: $lines"
case $lines in 'sluice main · 3 submissions · testing main + #12 Batch review cards + #13 plugin-board '*) ;;
*) fail "the segment printed: $lines" ;;
esac
pass

fresh 'status: fields it does not know change nothing'
record_of 87 "$work" '["tests:t-cards"]'
extra='.later = {"anything": 1} | .branches[0].candidates |= map(.tasks = [{"name": "x", "state": "running"}])'
# The same documents read twice: as sluice prints them today, and with
# fields a later sluice may add.
for run in plain extra; do
  fresh_state=$t/$run
  mkdir -p "$fresh_state"
  : >"$FAKE/herdr-calls"
  if [ "$run" = extra ]; then e=$extra; else e=.; fi
  one=$(doc "$e | .branches[0].depth = 1 | .branches[0].submissions = [$(sub 95 '$work' testing)] | .branches[0].candidates = [{id: \"c1\", submissions: [95], revision: \"x\", state: \"testing\", started: \"2026-10-16T19:42:00Z\", limit: 900, targets: [\"#check-fast\"], executor: \"sandbox\", ran: 0, replayed: 0, failed: []}] | $e")
  two=$(doc "$e | $blamed87")
  {
    HERDR_PLUGIN_STATE_DIR=$fresh_state feed "$(doc "$e")" "$one"
    HERDR_PLUGIN_STATE_DIR=$fresh_state frame_of "$work" --once >"$t/$run.frame"
    HERDR_PLUGIN_STATE_DIR=$fresh_state tokens
    HERDR_PLUGIN_STATE_DIR=$fresh_state feed "$two"
    HERDR_PLUGIN_STATE_DIR=$fresh_state tokens
  } 2>>"$t/$run.err"
  sed 's/--seq [0-9]*//' "$FAKE/herdr-calls" >"$t/$run.calls"
done
grep -q '"later"' "$t"/extra/docs/*.json || fail "the extra fields were not kept"
cmp -s "$t/plain.frame" "$t/extra.frame" || fail "the popup differs: $(diff "$t/plain.frame" "$t/extra.frame")"
cmp -s "$t/plain.calls" "$t/extra.calls" || fail "the herdr calls differ: $(diff "$t/plain.calls" "$t/extra.calls")"
grep -q 'notification show sluice: #87 blamed' "$t/extra.calls" || fail "no toast with the extra fields"
grep -q 'token sluice=#95 testing' "$t/extra.calls" || fail "no token with the extra fields"
[ -s "$t/extra.err" ] && fail "something was logged: $(cat "$t/extra.err")"
pass

## herdr-toasts

fresh 'toasts: blamed'
record_of 87 "$work" '["tests:t-cards"]'
feed "$(doc)" "$(doc "$blamed87")"
has 'notification show sluice: #87 blamed on main --body tests:t-cards failed.'
[ "$(grep -c 'notification show' "$FAKE/herdr-calls")" = 1 ] || fail "not one toast: $(calls)"
pass

fresh 'toasts: a number another branch holds'
# #87 blamed on main; feature/a holds an #87 too, blamed on other tests:
# each blame is told with its own branch's tests.
record_of 87 "$work" '["tests:t-cards"]'
record_of 87 "$other" '["other:test"]' feature/a
on_a='.branches += [{branch: "feature/a", depth: 0, candidates: [], submissions: [], recent: []}]'
blamed87_a='.branches[1].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:10Z", event: "blamed", id: 87, combination: "a1"}]'
feed "$(doc "$on_a")" "$(doc "$on_a | $blamed87")"
has 'notification show sluice: #87 blamed on main --body tests:t-cards failed.'
hasnt 'other:test'
feed "$(doc "$on_a | $blamed87 | $blamed87_a")"
has 'notification show sluice: #87 blamed on feature/a --body other:test failed.'
grep -q status "$FAKE/sluice-calls" && fail "the reader ran sluice status: $(cat "$FAKE/sluice-calls")"
pass

fresh 'toasts: watcher restarted'
record_of 87 "$work" '["tests:t-cards"]'
feed "$(doc)" "$(doc "$blamed87")" "$(doc "$blamed87")"
[ "$(grep -c 'notification show' "$FAKE/herdr-calls")" = 1 ] || fail "told twice: $(calls)"
pass

fresh 'toasts: what is told is kept only while it is recent'
record_of 53 "$work"
feed "$(doc "$blamed87 | $landed94")" "$(doc "$landed94 | $conflict53")"
# One list for the repository, its events' keys.
set -- "$S"/told/*.json
[ "$#" = 1 ] && [ -f "$1" ] || fail "not one told list: $(ls "$S/told")"
got=$(jq -c 'sort' "$1")
[ "$got" = '["main|conflict|53|2026-10-16T19:44:30Z","main|landed|94|2026-10-16T19:33:10Z"]' ] || fail "told holds: $got"
pass

fresh 'toasts: an event on a long branch, or of many submissions'
long=$(printf 'b%.0s' $(seq 1 200))
on_long=".branches[0].branch = \"$long\""
many='.branches[0].recent += [{schema: "sluice/log@1", at: "2026-10-16T19:44:40Z", event: "landed", ids: [range(100; 161)], to: "aaaaaaa"}]'
first=$(doc "$on_long")
second=$(doc "$on_long | $blamed87 | $many")
feed "$first" "$second" 2>"$t/err"
has "notification show sluice: #87 blamed on $long"
# Kept in the list whole, however long: no file is named by an event.
[ "$(jq -r --arg b "$long" 'map(select(startswith($b + "|"))) | length' "$S"/told/*.json)" = 2 ] ||
  fail "not both events are recorded as told: $(cat "$S"/told/*.json) $(cat "$t/err")"
grep -q 'cannot' "$t/err" && fail "logged: $(cat "$t/err")"
pass

fresh 'toasts: none when what was told cannot be kept'
record_of 87 "$work" '["t"]'
record_of 53 "$work"
feed "$(doc)"
# Unwritable even to root, as the containers run it: a file where the
# folder of the told lists was.
rm -rf "$S/told" && : >"$S/told"
for i in 1 2 3; do feed "$(doc "$blamed87 | $conflict53 | .branches[0].version = $i")"; done 2>"$t/err"
hasnt 'notification show'
[ "$(grep -c 'cannot record' "$t/err")" = 1 ] || fail "not logged once: $(cat "$t/err")"
# Once it can be kept again, what follows is told.
rm -f "$S/told"
mkdir "$S/told"
feed "$(doc "$blamed87 | $conflict53")" "$(doc "$blamed87 | $conflict53 | $landed94")" 2>"$t/err"
hasnt 'notification show'
feed "$(doc "$blamed87 | $conflict53 | .branches[0].recent += [{schema: \"sluice/log@1\", at: \"2026-10-16T19:45:00Z\", event: \"blamed\", id: 88, combination: \"c\"}]")" 2>"$t/err"
has 'notification show sluice: #88 blamed on main'
pass

fresh 'toasts: first sight of a repository'
feed "$(doc "$blamed87 | $conflict53")"
hasnt 'notification show'
pass

fresh 'toasts: a conflict'
record_of 53 "$other"
feed "$(doc)" "$(doc "$conflict53")"
has 'notification show sluice: #53 conflicts on main --body a.rs'
pass

fresh 'toasts: landed, looking elsewhere'
panes p2
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$work' testing)]")" "$(doc "$landed94")"
has 'notification show sluice: #94 landed on main --body as 46cc429'
pass

fresh 'toasts: landed, looking at it'
panes p1
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$work' testing)]")" "$(doc "$landed94")"
hasnt 'notification show'
pass

fresh 'toasts: landed, no pane'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '"/elsewhere"' testing)]")" "$(doc "$landed94")"
hasnt 'notification show'
pass

fresh 'toasts: three servers'
record_of 87 "$work" '["tests:t-cards"]'
hold_lease
# Servers a and b have panes in the repository, c has none; each
# registered and its panes kept, as the watcher keeps them at its ensure.
for s in a b c; do
  serve "$s"
  if [ "$s" = c ]; then panes_at p3 "$scratch/home"; else panes front-none; fi
  sh "$plugin/bin/watch.sh" --ensure
  kept
done
feed "$(doc)" "$(doc "$blamed87")"
for s in a b; do
  [ "$(grep -c "^$s.sock: notification show sluice: #87 blamed on main --body tests:t-cards failed." "$FAKE/herdr-calls")" = 1 ] ||
    fail "not one toast in $s: $(calls)"
done
grep -q '^c.sock: notification show' "$FAKE/herdr-calls" && fail "a toast in c, which has no pane in the repository: $(calls)"
# A watcher restarted after them reads the same document: nothing is told
# again, in any server.
: >"$FAKE/herdr-calls"
feed "$(doc "$blamed87")"
hasnt 'notification show'
pass

fresh 'toasts: a repository no pane is in'
hold_lease
for s in one two; do
  serve "$s"
  panes_at p3 "$scratch/home"
  sh "$plugin/bin/watch.sh" --ensure
  kept
done
# A source of the user's prints a repository no pane of either server is
# in, then a blame in it.
app='.host = "build" | .repository = "/srv/app.git"'
for d in "$(doc "$app")" "$(doc "$app | $blamed87")"; do
  printf '%s\n' "$d" | sh "$plugin/bin/document.sh" "build machine"
done
for s in one two; do
  [ "$(grep -c "^$s.sock: notification show sluice: #87 blamed on main" "$FAKE/herdr-calls")" = 1 ] ||
    fail "not one toast in $s: $(calls)"
done
pass

fresh 'toasts: landed in two servers'
hold_lease
# #94's worktree has a pane in front in server a, and a pane in another tab
# in server b.
serve a
panes p1
sh "$plugin/bin/watch.sh" --ensure
kept
serve b
panes p2
sh "$plugin/bin/watch.sh" --ensure
kept
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$work' testing)]")" "$(doc "$landed94")"
[ "$(grep -c '^b.sock: notification show sluice: #94 landed on main --body as 46cc429' "$FAKE/herdr-calls")" = 1 ] ||
  fail "not one toast in b: $(calls)"
grep -q '^a.sock: notification show' "$FAKE/herdr-calls" && fail "a toast in a, where its pane is in front: $(calls)"
grep -q '^a.sock: tab list' "$FAKE/herdr-calls" || fail "what is in front was not read in a: $(calls)"
pass

fresh 'jump: jump'
record_of 87 "$other" '["tests:t-cards"]'
feed "$(doc)" "$(doc "$blamed87")"
: >"$FAKE/herdr-calls"
sh "$plugin/bin/jump.sh"
has 'agent focus p2'
touch "$H/no-agent"
sh "$plugin/bin/jump.sh"
has 'workspace focus w2'
has 'tab focus t2'
pass

fresh 'jump: a number another branch holds'
# #87 blamed on main, from the worktree other; feature/a holds an #87 too,
# queued from the worktree work: the jump reads main's folder for it.
record_of 87 "$other" '["tests:t-cards"]'
feed "$(doc)" "$(doc ".branches += [{branch: \"feature/a\", depth: 1, candidates: [], recent: [],
    submissions: [$(sub 87 '$work' queued)]}] | $blamed87")"
: >"$FAKE/herdr-calls"
: >"$FAKE/sluice-calls"
sh "$plugin/bin/jump.sh"
has 'agent focus p2'
hasnt 'focus p1'
[ -s "$FAKE/sluice-calls" ] && fail "the jump ran sluice: $(cat "$FAKE/sluice-calls")"
pass

fresh 'jump: no pane'
record_of 87 /elsewhere
feed "$(doc)" "$(doc "$blamed87")"
: >"$FAKE/herdr-calls"
sh "$plugin/bin/jump.sh"
has 'notification show sluice: no pane for #87'
hasnt 'focus'
pass

## herdr-sidebar

fresh 'sidebar: testing'
feed "$testing95"
tokens
has 'pane report-metadata p1 --source agentplot.sluice --seq'
has '--ttl-ms 120000 --token sluice=#95 testing 3m/15m'
pass

fresh 'sidebar: testing, with tasks'
feed "$(printf '%s' "$testing95" | jq -c '.branches[0].candidates[0].tasks = [{target: "a:test", state: "passed"}, {target: "b:test", state: "running"}]')"
tokens
has '--token sluice=#95 testing 1/2 ▓▓▓▓▓░░░░░ 3m/15m'
pass

checking='{worktree: $work, branch: "main", ports: {}, run: {started: "2026-10-16T19:43:10Z",
  tasks: [{target: "a:test", state: "passed"}, {target: "b:test", state: "running"}, {target: "c:test", state: "waiting"}]}}'

fresh 'sidebar: checking'
feed "$(doc ".sessions = [$checking]")"
tokens
has '--token sluice=checking 1/3'
hasnt 'testing'
pass

fresh 'sidebar: checking a command'
feed "$(doc ".sessions = [$checking | .run = {started: .run.started, command: \"make e2e\"}]")"
tokens
has '--token sluice=checking 2m'
pass

fresh 'sidebar: a submission comes before a check'
feed "$(printf '%s' "$testing95" | jq -c --arg work "$work" ".sessions = [$checking]")"
tokens
has '--token sluice=#95 testing 3m/15m'
pass

fresh 'sidebar: a blame comes before a check'
record_of 87 "$work" '["tests:t-cards"]'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 87 '$work' testing)]")" "$(doc "$blamed87 | .sessions = [$checking]")"
tokens
has '--token sluice=#87 BLAMED · tests:t-cards'
hasnt 'checking'
pass

fresh 'sidebar: blamed'
record_of 87 "$work" '["tests:t-cards"]'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 87 '$work' testing)]")" "$(doc "$blamed87")"
tokens
has '--token sluice=#87 BLAMED · tests:t-cards'
pass

fresh "sidebar: blamed, a command runner's task"
record_of 87 "$work" '["sleep 15; test ! -f bad"]'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 87 '$work' testing)]")" "$(doc "$blamed87")"
tokens
has '--token sluice=#87 BLAMED'
hasnt 'BLAMED ·'
pass

fresh 'sidebar: a conflict'
record_of 53 "$work"
feed "$(doc)" "$(doc "$conflict53")"
tokens
has '--token sluice=#53 CONFLICT · a.rs'
pass

fresh 'sidebar: landed, then gone'
git -C "$scratch/other" checkout -q --detach "$main"
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$other' testing)]")" "$(doc "$landed94")"
tokens
has 'pane report-metadata p2 --source agentplot.sluice'
has '--token sluice=#94 landed 12m'
: >"$FAKE/herdr-calls"
SLUICE_HERDR_NOW=$((NOW + 1200)) tokens
has 'pane report-metadata p2 --source agentplot.sluice'
has '--clear-token sluice'
# The watcher, on the clock, moves it with no document: a landing ten
# seconds short of half an hour is gone at a minute's pass.
: >"$FAKE/herdr-calls"
at=$(jq -n -r --argjson t "$(($(date +%s) - 1790))" '$t | todate')
{
  doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$other' testing)]"
  doc ".branches[0].recent += [{schema: \"sluice/log@1\", at: \"$at\", event: \"landed\", ids: [94], to: \"46cc4290c0\"}]"
} >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
SLUICE_HERDR_NOW='' SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 'grep -q "pane report-metadata p2 .*--token sluice=#94 landed 29m" "$FAKE/herdr-calls"' || fail "no landed token: $(calls)"
within 25 'grep -q "pane report-metadata p2 .*--clear-token sluice" "$FAKE/herdr-calls"' ||
  fail "the landing was not gone at a minute's pass: $(grep p2 "$FAKE/herdr-calls")"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
git -C "$scratch/other" checkout -q other
pass

fresh 'sidebar: nothing to say'
feed "$(doc)"
tokens
hasnt 'pane report-metadata p2'
hasnt 'pane report-metadata p3'
pass

fresh 'sidebar: clean'
feed "$(doc)"
forecast
tokens
has '--token sluice=3 ahead · clean'
[ "$(grep -c 'work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "not one forecast: $(cat "$FAKE/sluice-calls")"
pass

fresh 'sidebar: ahead again after a landing'
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 94 '$work' testing)]")" "$(doc "$landed94")"
forecast
tokens
[ "$(grep -c 'work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "no forecast for a worktree whose last submission landed"
has '--token sluice=3 ahead · clean'
pass

fresh 'sidebar: conflicts'
echo '{"conflict":{"paths":["docs/architecture/builds.md","b.rs"],"with":[{"id":95,"from":{"agent":"x","worktree":"y"}}]}}' >"$FAKE/check.json"
feed "$(doc)"
forecast
tokens
has '--token sluice=3 ahead · conflicts #95 in builds.md +1'
# A forecast kept before it named its files says what it did then.
for c in "$S"/checks/*; do
  jq -c 'del(.paths)' "$c" >"$c.new" && mv "$c.new" "$c"
done
: >"$FAKE/herdr-calls"
tokens
has '--token sluice=3 ahead · conflicts #95'
hasnt '3 ahead · conflicts #95 in'
pass

fresh 'sidebar: conflicts with what landed'
echo '{"conflict":{"paths":["docs/spec/identity.md"],"with":[]}}' >"$FAKE/check.json"
feed "$(doc)"
forecast
tokens
has '--token sluice=3 ahead · conflicts main in identity.md'
hasnt 'identity.md +'
pass

fresh 'sidebar: nothing changed'
feed "$(doc)"
forecast
feed "$(doc '.at = "2026-10-16T19:46:00Z"')"
forecast
[ "$(grep -c 'submit main --check' "$FAKE/sluice-calls")" = 1 ] || fail "forecast again: $(cat "$FAKE/sluice-calls")"
# A combination starting moves the branch's version, not what the forecast
# rests on.
feed "$(doc '.branches[0].version = 2 | .branches[0].candidates = [{id: "c1", submissions: [], revision: "x", state: "testing"}]')"
forecast
[ "$(grep -c 'submit main --check' "$FAKE/sluice-calls")" = 1 ] || fail "forecast on a new version alone"
# A submission waiting on the branch does.
feed "$(doc ".branches[0].version = 3 | .branches[0].depth = 1 | .branches[0].submissions = [$(sub 96 '$other' queued)]")"
forecast
[ "$(grep -c 'submit main --check' "$FAKE/sluice-calls")" = 2 ] || fail "no forecast on a new submission waiting"
pass

fresh 'sidebar: forecasts off'
echo 'ahead_check = false' >"$HERDR_PLUGIN_CONFIG_DIR/sources.toml"
feed "$(doc)"
forecast
tokens
grep -q 'submit' "$FAKE/sluice-calls" && fail "a forecast ran"
has '--token sluice=3 ahead'
hasnt '3 ahead ·'
pass

fresh 'sidebar: a named branch'
named p1 release
feed "$(doc "$release_busy")"
tokens
has '--token sluice=release · 3 queued · #12 testing 3/8 · 2 checking'
pass

fresh 'sidebar: a submission comes before a named branch'
named p1 release
feed "$(printf '%s' "$testing95" | jq -c --arg main "$main" "$release_busy")"
tokens
has '--token sluice=#95 testing 3m/15m'
hasnt 'sluice=release'
pass

fresh 'sidebar: a blame comes before a named branch'
record_of 87 "$work" '["tests:t-cards"]'
named p1 release
feed "$(doc ".branches[0].depth = 1 | .branches[0].submissions = [$(sub 87 '$work' testing)]")" "$(doc "$blamed87 | $release_busy")"
tokens
has '--token sluice=#87 BLAMED · tests:t-cards'
hasnt 'sluice=release'
pass

fresh 'sidebar: a named branch with nothing queued'
named p1 release
feed "$(doc "$release_idle")"
tokens
has '--token sluice=release · nothing queued'
pass

fresh 'sidebar: a named branch with no records'
named p1 release
feed "$(doc)"
tokens
has '--token sluice=release · no records'
pass

fresh 'sidebar: a named branch counts its checks'
named p1 release
feed "$(doc "$release_idle | .sessions = [$checking,
  {worktree: \"/elsewhere/u1\", branch: \"release\", run: {started: \"2026-10-16T19:44:00Z\"}},
  {worktree: \"/elsewhere/u2\", branch: \"release\"}]")"
tokens
has '--token sluice=release · 1 checking'
hasnt 'checking 1/3'
pass

fresh 'sidebar: a named branch with more than one under test'
named p1 release
feed "$(doc "$release_busy | .sessions = []
  | .branches[1].candidates[0] |= (del(.tasks) | .started = \"2026-10-16T19:41:10Z\")
  | .branches[1].candidates += [.branches[1].candidates[0] | .id = \"c10\" | .submissions = [14]]
  | .branches[1].submissions |= map(select(.id == 12 or .id == 14) | .state = \"testing\")")"
tokens
has '--token sluice=release · #12 testing 4m +1 more'
pass

fresh 'sidebar: no forecast for a named pane'
named p1 release
feed "$(doc "$release_idle | .sessions = [{worktree: \$work, branch: \"main\"}]")"
forecast
grep -q 'submit' "$FAKE/sluice-calls" && fail "a forecast ran for a named pane: $(cat "$FAKE/sluice-calls")"
# Another pane in the same worktree, with no branch named, has one.
jq -c --arg work "$work" '.result.panes += [{pane_id: "p4", tab_id: "t1", workspace_id: "w1", cwd: $work, foreground_cwd: $work, focused: false}]' \
  "$H/panes.json" >"$H/panes.next" && mv "$H/panes.next" "$H/panes.json"
forecast
tokens
[ "$(grep -c 'work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "no forecast for the pane with no branch named: $(cat "$FAKE/sluice-calls")"
has 'pane report-metadata p4 --source agentplot.sluice'
has '--token sluice=3 ahead · clean'
has '--token sluice=release · nothing queued'
pass

fresh 'sidebar: two servers forecast once'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# Each server has a pane in the worktree work, 3 commits ahead of main.
for s in one two; do
  serve "$s"
  panes front-none
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
done
within 20 'grep -q "^one.sock: pane report-metadata p1 .*--token sluice=3 ahead · clean" "$FAKE/herdr-calls" &&
  grep -q "^two.sock: pane report-metadata p1 .*--token sluice=3 ahead · clean" "$FAKE/herdr-calls"' ||
  fail "not both servers' panes say 3 ahead · clean: $(calls)"
settled
[ "$(grep -c '^work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "not one forecast: $(cat "$FAKE/sluice-calls")"
rm -f "$FAKE/hold"
pass

fresh 'sidebar: named in one server, not in another'
feed "$(doc)"
# The worktree work, 3 commits ahead of main: its pane in server one has
# release named, its pane in server two none.
serve one
panes front-none
named p1 release
kept
serve two
panes front-none
kept
sh "$plugin/bin/forecast.sh"
sh "$plugin/bin/forecast.sh"
[ "$(grep -c '^work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "not one forecast: $(cat "$FAKE/sluice-calls")"
tokens
grep -q '^two.sock: pane report-metadata p1 .*--token sluice=3 ahead · clean' "$FAKE/herdr-calls" || fail "server two's pane: $(calls)"
serve one
tokens
grep -q '^one.sock: pane report-metadata p1 .*--token sluice=release · no records' "$FAKE/herdr-calls" || fail "server one's pane: $(calls)"
grep -q '^one.sock: .*ahead' "$FAKE/herdr-calls" && fail "server one's named pane says ahead: $(calls)"
pass

fresh 'sidebar: queued'
release='.branches[0].depth = 3 | .branches += [.branches[0] | .branch = "release" | .depth = 1]'
feed "$(doc "$release")"
tokens
has 'workspace report-metadata w1 --source agentplot.sluice'
has '--token sluice_q=4 submissions'
: >"$FAKE/herdr-calls"
feed "$(doc)"
tokens
has 'workspace report-metadata w1 --source agentplot.sluice'
has '--clear-token sluice_q'
pass

fresh 'sidebar: queued and held'
feed "$(doc ".branches[0].depth = 3 | .branches += [.branches[0] | .branch = \"release\" | .depth = 1] | $held48")"
tokens
has '--token sluice_q=4 submissions · 1 held: release failed'
pass

fresh 'sidebar: held alone'
feed "$(doc "$held_unit_a")"
tokens
has '--token sluice_q=1 held: release failed'
pass

fresh 'sidebar: a checkout two hosts list is counted once'
for h in deneb vega; do
  feed "$(doc "$held48 | .host = \"$h\" | .branches[0].held_checkouts[0].host = \"deneb\"")"
done
[ "$(find "$S/docs" -name '*.json' | wc -l)" -eq 2 ] || fail "not two documents: $(ls "$S/docs")"
tokens
has '--token sluice_q=1 held: release failed'
pass

fresh 'sidebar: a shown token is set again once a minute'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 'grep -q "pane report-metadata p1 .*--token sluice=3 ahead · clean" "$FAKE/herdr-calls"' || fail "no token: $(calls)"
# The start's tick and the next have read the panes. A pass lists the
# panes and then sets the token, so the window opens as a minute's pass
# ends, the next four ticks away, and is read as it stood when it closed,
# a pass that has listed the panes and not yet set the token left out as
# still going: a window cut by the clock alone may hold half a pass at
# either end, however the watcher behaves.
sleep 2
: >"$FAKE/herdr-calls"
within 15 'grep -q "pane report-metadata p1 .*--ttl-ms 120000 --token sluice=3 ahead · clean" "$FAKE/herdr-calls"' || fail "no minute's pass: $(calls)"
: >"$FAKE/herdr-calls"
began=$(date +%s)
sleep 10
cp "$FAKE/herdr-calls" "$t/window"
took=$(($(date +%s) - began))
sets=$(grep -c 'pane report-metadata p1 .*--ttl-ms 120000 --token sluice=3 ahead · clean' "$t/window")
lists=$(grep -c ': pane list$' "$t/window")
tail -n 1 "$t/window" | grep -q ': pane list$' && lists=$((lists - 1))
# A minute is four ticks: one or two in ten seconds, never one a tick.
[ "$sets" -ge 1 ] || fail "the token was not set again in ${took}s: $(cat "$t/window")"
[ "$lists" -le $((took / 4 + 1)) ] || fail "the panes were listed $lists times in ${took}s"
# Each pass lists the panes once and sets each token shown.
[ "$lists" = "$sets" ] || fail "$lists pane lists for $sets tokens set: $(cat "$t/window")"
grep -q -e 'tab list' -e 'workspace list' "$FAKE/herdr-calls" && fail "tabs or workspaces were listed: $(calls)"
grep -q 'p1 .*--clear-token' "$FAKE/herdr-calls" && fail "the token was cleared"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
pass

fresh 'sidebar: a commit is forecast within a tick'
doc >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ticks 60 2>"$t/log" &
w=$!
within 15 'grep -q "pane report-metadata p1 .*--token sluice=3 ahead · clean" "$FAKE/herdr-calls"' || fail "no token: $(calls)"
sleep 2
: >"$FAKE/sluice-calls"
# One more commit, and no document or herdr event follows.
git -C "$work" commit -q --allow-empty -m w4
within 5 'grep -q "pane report-metadata p1 .*--token sluice=4 ahead · clean" "$FAKE/herdr-calls"' || fail "the token did not follow the commit: $(calls)"
# Forecast once: the ticks after see the head no newer than its stamp.
sleep 3
[ "$(grep -c '^work: submit main --check --json' "$FAKE/sluice-calls")" = 1 ] || fail "not one forecast: $(cat "$FAKE/sluice-calls")"
rm -f "$FAKE/hold"
kill "$w" 2>/dev/null
wait "$w" 2>/dev/null
git -C "$work" reset -q --hard HEAD~1
pass

fresh 'sidebar: two servers, one worktree'
printf '%s\n' "$testing95" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# Each server has a pane in the worktree that made #95.
for s in one two; do
  serve "$s"
  panes front-none
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
done
within 20 'grep -q "^one.sock: pane report-metadata p1 .*--token sluice=#95 testing 3m/15m" "$FAKE/herdr-calls" &&
  grep -q "^two.sock: pane report-metadata p1 .*--token sluice=#95 testing 3m/15m" "$FAKE/herdr-calls"' ||
  fail "not both servers' panes say #95 testing 3m/15m: $(calls)"
[ "$(grep -c 'overview --json --watch --stdin' "$FAKE/sluice-calls")" = 1 ] || fail "watch ran other than once: $(cat "$FAKE/sluice-calls")"
# What each server was set is its own.
for s in one two; do
  [ "$(jq -r .panes.p1 "$(folder_of "$s")/tokens.json" 2>/dev/null)" = '#95 testing 3m/15m' ] ||
    fail "server $s keeps: $(cat "$(folder_of "$s")/tokens.json")"
done
rm -f "$FAKE/hold"
pass

fresh 'sidebar: a server stops'
printf '%s\n' "$testing95" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
for s in one two; do
  serve "$s"
  panes front-none
  SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
done
within 20 'grep -q "^one.sock: pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls" &&
  grep -q "^two.sock: pane report-metadata p1 .*--token sluice=#95 testing" "$FAKE/herdr-calls"' ||
  fail "not both servers' panes say #95 testing: $(calls)"
# Server one stops, as its panes show #95 testing.
rm -f "$t/one.sock"
within 10 '[ ! -e "$(folder_of one)/socket" ]' || fail "the stopped server is still served"
: >"$FAKE/herdr-calls"
# The queue moves: server two's pane follows it, and the minute's pass
# sets it again; server one is called for neither.
printf '%s\n' "$testing95" | jq -c '.branches[0].candidates[0].tasks = [{target: "a:test", state: "passed"}, {target: "b:test", state: "running"}]' >"$FAKE/next-work.tmp" &&
  mv "$FAKE/next-work.tmp" "$FAKE/next-work.jsonl"
within 10 'grep -q "^two.sock: pane report-metadata p1 .*--token sluice=#95 testing 1/2" "$FAKE/herdr-calls"' ||
  fail "server two's pane did not follow the queue: $(calls)"
within 10 'grep -q "^two.sock: pane list$" "$FAKE/herdr-calls"' || fail "server two had no minute's pass: $(calls)"
grep -q '^one.sock:' "$FAKE/herdr-calls" && fail "the stopped server was called: $(calls)"
rm -f "$FAKE/hold"
pass

fresh 'sidebar: named just after it opens'
doc "$release_idle" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
# At first the server's one pane is outside git.
panes_at p3 "$scratch/home"
SLUICE_HERDR_TICK=2 sh "$plugin/bin/watch.sh" --ensure
within 10 'grep -q "^herdr.sock: pane list$" "$FAKE/herdr-calls"' || fail "the server's panes were not read"
settled
# A pane opens in the repository, and its branch is named a second after
# the event, with none following.
panes_at p3 "$scratch/home" p1 "$work"
sh "$plugin/bin/watch.sh" --ensure
sleep 1
named p1 release
within 5 'grep -q "pane report-metadata p1 .*--token sluice=release · nothing queued" "$FAKE/herdr-calls"' ||
  fail "the pane's named branch was not read within a tick: $(calls)"
rm -f "$FAKE/hold"
pass

fresh 'sidebar: named later'
doc "$release_idle" >"$FAKE/watch-work.jsonl"
touch "$FAKE/hold"
named p1 release
SLUICE_HERDR_TICK=1 sh "$plugin/bin/watch.sh" --ensure
within 15 'grep -q "pane report-metadata p1 .*--token sluice=release · nothing queued" "$FAKE/herdr-calls"' ||
  fail "no token for the named branch: $(calls)"
# Past the ticks that read the panes again after the ensure.
sleep 3
# Another branch is named for the pane, and no herdr event follows: the
# minute's pass, four ticks here, reads it.
named p1 main
within 8 'grep -q "pane report-metadata p1 .*--token sluice=main" "$FAKE/herdr-calls"' ||
  fail "the pane's new branch was not read within a minute and a tick: $(calls)"
rm -f "$FAKE/hold"
pass

fresh 'sidebar: tokens fade without the watcher'
feed "$testing95"
tokens
: >"$FAKE/herdr-calls"
tokens
hasnt 'report-metadata'
tokens --refresh
has '--ttl-ms 120000 --token sluice=#95 testing'
pass

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures failed of $((count - skips)), $skips skipped"
  exit 1
fi
echo "all $((count - skips)) passed, $skips skipped"
