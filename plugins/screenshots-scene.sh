#!/usr/bin/env bash
# The scene the plugins' screenshots are taken of: a real sluice queue on a
# demo repository, with one submission blamed, one landed, an agent's own
# check (sluice exec) part done in its worktree, holding one of the host's
# two runs, so the queue tests one combination of the two its window allows,
# its tasks part done, with two submissions queued behind it.
#
#   screenshots-scene.sh [--fast] <new dir>   build it and print "ready" once
#                                             the scene holds; the driver
#                                             keeps running, its pid in
#                                             <dir>/driver.pid
#   screenshots-scene.sh --teardown <dir>     stop the driver and sluice's
#                                             host process, remove <dir>
#   screenshots-scene.sh --check              the gate's check: the sluice
#                                             built from this tree, --fast in
#                                             a scratch directory, the scene
#                                             asserted, then torn down
#
# --fast shortens the tasks that pass so the scene holds within a minute.
# Needs git, jq, moon >= 2.6 and sluice on PATH. Written for bash 3.2.
set -euo pipefail

die() {
  echo "screenshots-scene: $*" >&2
  exit 1
}

ended() { # ended <pid>: wait for the process to end, up to 30 seconds
  local n=0
  while kill -0 "$1" 2>/dev/null && [ $n -lt 300 ]; do
    sleep 0.1
    n=$((n + 1))
  done
}

teardown() {
  local dir=$1 p
  for p in driver check; do
    [ -f "$dir/$p.pid" ] || continue
    pkill -P "$(cat "$dir/$p.pid")" 2>/dev/null || true
    kill "$(cat "$dir/$p.pid")" 2>/dev/null || true
    rm -f "$dir/$p.pid"
  done
  # sluice's host process for the scene's state (135), which the commands
  # above started: while the queue has work in play it lives on, starting
  # runs and writing its reader's document under state/sluice/readers, so
  # it ends before anything is removed. Its pid is in its version file.
  local pid
  pid=$(jq -r '.pid // empty' "$dir/state/sluice/host/version" 2>/dev/null || true)
  if [ -n "$pid" ]; then
    kill "$pid" 2>/dev/null || true
    ended "$pid"
  fi
  # The candidates' runs and the agent's, each its own process group.
  for g in "$dir"/shop/.git/sluice/.runs/*/pgid "$dir"/shop/.git/sluice/.runs/*/*/pgid; do
    [ -f "$g" ] && kill -TERM -- "-$(cat "$g")" 2>/dev/null || true
  done
  # Each run's supervisor writes the run's result once its group has
  # ended: wait for it before removing where it writes.
  local lock
  for lock in "$dir"/shop/.git/sluice/.runs/*/lock "$dir"/shop/.git/sluice/.runs/*/*/lock; do
    [ -f "$lock" ] || continue
    pid=$(tr -dc 0-9 <"$lock")
    if [ -n "$pid" ]; then ended "$pid"; fi
  done
  rm -rf "$dir"
}

case ${1:-} in
--teardown)
  teardown "${2:?usage: screenshots-scene.sh --teardown <dir>}"
  exit 0
  ;;
--check)
  cd "$(dirname "$0")/.."
  cargo build -p sluice --bins
  PATH="$PWD/target/debug:$PATH"
  export PATH
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/sluice-scene.XXXXXX")
  rmdir "$scratch"
  if ! bash plugins/screenshots-scene.sh --fast "$scratch"; then
    echo "screenshots-scene: failed; the scene is in $scratch" >&2
    exit 1
  fi
  teardown "$scratch"
  exit 0
  ;;
esac

FAST=0
if [ "${1:-}" = --fast ]; then
  FAST=1
  shift
fi
ROOT=${1:?usage: screenshots-scene.sh [--fast] <new dir>}
for t in git jq moon sluice; do command -v "$t" >/dev/null || die "needs $t on PATH; run it in devenv shell"; done
case $(moon --version) in
"moon 2."[6-9]* | "moon 2."[1-9][0-9]* | "moon "[3-9]*) ;;
*) die "needs moon >= 2.6, found $(moon --version)" ;;
esac
[ -e "$ROOT" ] && die "$ROOT exists; give a new directory"
mkdir -p "$ROOT/home"
ROOT=$(cd "$ROOT" && pwd -P)
REPO=$ROOT/shop

# moon's own variables never leak in from a caller's shell: moon reads MOON_* as options.
for v in $(env | sed -n 's/^\(MOON_[A-Z0-9_]*\)=.*/\1/p'); do unset "$v"; done
# Nothing of the user's: git's config, credentials and prompts, sluice's host state.
export GIT_AUTHOR_NAME=dev GIT_AUTHOR_EMAIL=dev@example.com GIT_COMMITTER_NAME=dev GIT_COMMITTER_EMAIL=dev@example.com
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never
unset GIT_ASKPASS SSH_ASKPASS
export XDG_STATE_HOME=$ROOT/state XDG_CONFIG_HOME=$ROOT/config
# The directory standing for the user's, where each checkout's scratch is
# made (35), inside the scene: the gate's sandbox lets --check write no
# other, and --teardown removes it with the scene.
export SLUICE_USER_SCRATCH=$ROOT/scratch
# The host's bound, the same on every machine the scene is built on: two runs, one of them the agent's check.
mkdir -p "$XDG_CONFIG_HOME/sluice"
printf '[runs]\nmax = 2\n' >"$XDG_CONFIG_HOME/sluice/config.toml"

# --- the repository: three moon projects, five tasks tagged check ---------------------------------------------------
# Each task sleeps the seconds in delays/<task> and fails when fail/<task> exists.
TASKS="web:lint web:typecheck api:test ui:test web:build"
git init -q -b main "$REPO"
cd "$REPO"
mkdir -p .moon .config web api ui delays fail
cat >.moon/workspace.yml <<'EOF'
$schema: 'https://moonrepo.dev/schemas/workspace.json'
projects:
  web: 'web'
  api: 'api'
  ui: 'ui'
vcs:
  client: 'git'
  defaultBranch: 'main'
telemetry: false
EOF
project() { # project <name> <task...>
  local p=$1 t
  shift
  {
    echo "\$schema: 'https://moonrepo.dev/schemas/project.json'"
    echo "tasks:"
    for t in "$@"; do
      echo "  $t:"
      echo "    command: 'sh ../task.sh $p:$t'"
      echo "    inputs: ['/task.sh', '/delays/*', '/fail/*', '**/*']"
      echo "    tags: [check]"
    done
  } >"$p/moon.yml"
  echo "# $p" >"$p/README.md"
}
project web lint typecheck build
project api test
project ui test
cat >task.sh <<'EOF'
#!/bin/sh
# sleeps the seconds in delays/<task>, then fails if fail/<task> exists
t=$1
f=$(echo "$t" | tr : -)
sleep "$(cat "../delays/$f" 2>/dev/null || echo 1)"
if [ -e "../fail/$f" ]; then
  echo "$t: $(cat "../fail/$f")"
  exit 1
fi
echo "$t ok"
EOF
delays() { # delays <seconds for each of TASKS, in order>
  local t
  for t in $TASKS; do
    echo "$1" >"delays/$(echo "$t" | tr : -)"
    shift
  done
}
delays 1 1 1 1 1
touch fail/.keep
cat >.config/sluice.toml <<'EOF'
[tests]
runner = "moon"

[branches.main]
required = ["#check"]
parallel = 2
batch = false
timeout = "20m"
trust = ["open", "cache"]
EOF
echo '.moon/cache/' >.gitignore
git add -A
git commit -qm "shop: web, api and ui, each tested on every landing"
sluice protect >/dev/null

# --- each submission from its own worktree, as an agent's ------------------------------------------------------------
WT=$ROOT/shop-wt
submit() { # submit <name> <message> <command...>: a worktree on unit/<name>, the command run in it, committed, submitted
  local name=$1 msg=$2 dir=$WT/$1
  shift 2
  git worktree add -q -b "unit/$name" "$dir" main
  (cd "$dir" && "$@")
  git -C "$dir" add -A
  git -C "$dir" commit -qm "$msg"
  (cd "$dir" && SLUICE_AGENT="$name" sluice submit main --no-wait --json) | jq -r .id
}
state() { sluice status "$1" --json | jq -r .state; }
until_done() { # until_done <id>: rounds until the submission has landed or failed, for at most five minutes
  local i=0
  while :; do
    case $(state "$1") in landed | failed | conflicted) return 0 ;; esac
    sluice step main >/dev/null
    i=$((i + 1))
    [ $i -lt 150 ] || die "#$1 did not finish"
    sleep 2
  done
}

# History: one blamed, then one landed, which widens the window the blame narrowed back to two (13).
A=$(submit coupon-codes "api: coupon codes" sh -c 'echo "coupons" >>api/README.md; echo "expected 10.00, got 9.99" >fail/api-test')
until_done "$A"
B=$(submit cart-totals "web: cart totals round to the cent" sh -c 'echo "totals" >>web/README.md')
until_done "$B"
[ "$(state "$A")" = failed ] || die "#$A was not blamed: $(state "$A")"
[ "$(state "$B")" = landed ] || die "#$B did not land: $(state "$B")"

# Live: first an agent's own check in its worktree, a change that affects every task, one of them slow; it holds one
# of the host's two runs while the others pass.
inventory_sync() {
  echo "inventory" >>api/README.md
  if [ $FAST = 1 ]; then delays 1 1 1 1 300; else delays 1 1 1 1 900; fi
}
F=$WT/inventory-sync
git worktree add -q -b unit/inventory-sync "$F" main
(cd "$F" && inventory_sync)
git -C "$F" add -A
git -C "$F" commit -qm "api: inventory sync"
(cd "$F" && SLUICE_AGENT=inventory-sync sluice exec main) </dev/null >>"$ROOT/check.log" 2>&1 &
echo $! >"$ROOT/check.pid"
checking() { # the agent's check with three tasks passed and one running
  sluice overview --json | jq -e '
    ([.sessions[] | select(.branch == "main") | .run.tasks // empty] | first // []) as $k |
    ([$k[] | select(.state == "passed")] | length) >= 3 and
    ([$k[] | select(.state == "running")] | length) >= 1 and .room.sessions == 1' >/dev/null
}
i=0
until checking; do
  i=$((i + 1))
  [ $i -lt 150 ] || die "the agent's check did not start within five minutes; $(cat "$ROOT/check.log")"
  sleep 2
done

# Then the queue: three submissions, their tasks slow enough to photograph. The window allows two combinations under
# test, and the run the check holds leaves room for one.
if [ $FAST = 1 ]; then SLOW="1 1 1 120 120"; else SLOW="40 90 200 900 1000"; fi
checkout_flow() {
  echo checkout >>web/README.md
  # shellcheck disable=SC2086 # the delays are words on purpose
  delays $SLOW
}
C=$(submit checkout-flow "web: checkout flow" checkout_flow)
D=$(submit order-emails "api: order emails" sh -c 'echo "emails" >>api/README.md')
submit search-index "ui: search index" sh -c 'echo "search" >>ui/README.md' >/dev/null
# The driver: a round every two seconds from here on, as a waiting submission runs them; its first forms main+C
# and main+C+D and starts main+C alone, and D and E wait. A round that fails is logged and the next one runs.
(while :; do
  sluice step main || echo "round failed"
  sleep 2
done) </dev/null >>"$ROOT/driver.log" 2>&1 &
echo $! >"$ROOT/driver.pid"

# Ready: the check as above; #C under test alone with three tasks passed and one running, #D queued, so the agent's
# check holds the queue back; the blame and the landing recent. The room is read by its four counts alone, beside
# which it names the runs waiting, the turn and each repository's runs (150).
ready() {
  checking && sluice overview --json | jq -e --argjson c "$C" --argjson d "$D" --argjson a "$A" --argjson b "$B" '
    (.room | {bound, queue, sessions, reserve}) == {bound: 2, queue: 1, sessions: 1, reserve: 0} and
    (.branches[] | select(.branch == "main") |
    ([.candidates[] | select(.state == "testing" and .submissions == [$c]) | .tasks] | first // []) as $t |
    ([$t[] | select(.state == "passed")] | length) >= 3 and
    ([$t[] | select(.state == "running")] | length) >= 1 and
    ([.submissions[] | select(.id == $d and .state == "queued")] | length) == 1 and
    ([.recent[] | select(.event == "blamed") | .id] | index($a) != null) and
    ([.recent[] | select(.event == "landed") | .ids[]] | index($b) != null))' >/dev/null
}
i=0
until ready; do
  i=$((i + 1))
  [ $i -lt 300 ] || die "the scene did not hold within ten minutes; overview: $(sluice overview --json)"
  sleep 2
done
echo ready
