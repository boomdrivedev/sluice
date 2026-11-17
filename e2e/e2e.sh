#!/usr/bin/env bash
# sluice end to end: S1 to S4 of the statement, S27 of session-runs-1 and S30 of previews-1, for real, on a scratch repository: a moon workspace of two
# projects, beta depending on alpha.
#
#   e2e.sh <new dir> fixture   build the repository, then prove each scenario's fixture with git and moon alone:
#                              the candidates sluice must form, which conflict, which pass and which fail
#   e2e.sh <new dir> sluice    build the repository, then drive S1 to S4 through wt submit and sluice, and check
#
#   MOON=<path>                the moon to use (>= 2.5)
#
# Needs git >= 2.54 and jq; moon >= 2.5; in sluice mode, sluice and wt-submit on PATH.
# Written for bash 3.2.
set -euo pipefail

ROOT=${1:?usage: e2e.sh <new dir> fixture|sluice}
MODE=${2:?usage: e2e.sh <new dir> fixture|sluice}
MOON=${MOON:-moon}

die() {
  echo "e2e: $*" >&2
  exit 1
}
need() { command -v "$1" >/dev/null || die "needs $1 on PATH"; }
need git
need jq
[ "$MODE" = sluice ] && {
  need sluice
  need wt-submit
}
[ -e "$ROOT" ] && die "$ROOT exists; give a new directory"
mkdir -p "$ROOT/places"
ROOT=$(cd "$ROOT" && pwd -P)
REPO=$ROOT/repo
# moon's own variables never leak in from a caller's shell: moon reads MOON_* as options.
for v in $(env | sed -n 's/^\(MOON_[A-Z0-9_]*\)=.*/\1/p'); do unset "$v"; done
export GIT_AUTHOR_NAME=e2e GIT_AUTHOR_EMAIL=e2e@localhost GIT_COMMITTER_NAME=e2e GIT_COMMITTER_EMAIL=e2e@localhost
# No git here, sluice's included, reaches the user's config, credential helpers or a prompt.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GCM_INTERACTIVE=never
unset GIT_ASKPASS SSH_ASKPASS
# No submit here records the caller's herdr pane, and no round pushes an outcome into it.
unset HERDR_ENV HERDR_SESSION HERDR_SOCKET_PATH HERDR_PANE_ID HERDR_TAB_ID HERDR_WORKSPACE_ID HERDR_AGENT HERDR_BIN_PATH

# indir <dir> <command...>: run a command in a directory.
indir() {
  local dir=$1
  shift
  (cd "$dir" && "$@")
}

policy() { # the walkthrough's policy for bolt branches
  cat <<EOF
[tests]
runner = "moon"

[branches."bolt/*"]
required = ["#suite"]
parallel = 4
batch = false
flaky = "fail"
timeout = "20m"
trust = ["open", "sandbox", "container"]
EOF
}

# --- the repository ------------------------------------------------------------------------------------------------
need "$MOON"
git init -q -b main "$REPO"
cd "$REPO"
mkdir -p .moon .config alpha beta
cat >.moon/workspace.yml <<'EOF'
$schema: 'https://moonrepo.dev/schemas/workspace.json'
projects:
  alpha: 'alpha'
  beta: 'beta'
vcs:
  client: 'git'
  defaultBranch: 'main'
telemetry: false
EOF
cat >alpha/moon.yml <<'EOF'
$schema: 'https://moonrepo.dev/schemas/project.json'
tasks:
  test:
    command: 'sh test.sh'
    inputs: ['test.sh', 'value.txt', 'notes.md']
    tags: [suite]
EOF
cat >alpha/test.sh <<'EOF'
#!/bin/sh
grep -qx ok value.txt || { echo "alpha: value.txt is $(cat value.txt)"; exit 1; }
echo "alpha ok"
EOF
cat >beta/moon.yml <<'EOF'
$schema: 'https://moonrepo.dev/schemas/project.json'
dependsOn: ['alpha']
tasks:
  test:
    command: 'sh test.sh'
    inputs: ['test.sh', 'value.txt', 'notes.md', '/alpha/value.txt']
    deps: ['alpha:test']
    tags: [suite]
EOF
cat >beta/test.sh <<'EOF'
#!/bin/sh
grep -qx ok value.txt || { echo "beta: value.txt is $(cat value.txt)"; exit 1; }
grep -qx ok ../alpha/value.txt || { echo "beta: alpha's value.txt is $(cat ../alpha/value.txt)"; exit 1; }
echo "beta ok"
EOF
for p in alpha beta; do
  echo ok >$p/value.txt
  printf '%s notes\n\nnothing yet\n' $p >$p/notes.md
done
echo '.moon/cache/' >.gitignore
policy >.config/sluice.toml
git add -A
git commit -qm "two moon projects, a test each tagged suite, and sluice's policy"
# What each scenario edits: a file both a and c write (S2), and a test each of a (S4) and b (S3) breaks.
A_FILE=alpha/notes.md A_LINE=3 B_FILE=beta/notes.md B_LINE=3
B_BREAKS=beta/value.txt B_FAILS=beta:test A_BREAKS=alpha/value.txt A_FAILS=alpha:test
BROKEN=broken
case $(indir "$REPO" "$MOON" --version) in
"moon 2."[5-9]* | "moon 2."[1-9][0-9]* | "moon "[3-9]*) ;;
*) die "needs moon >= 2.5, found $(indir "$REPO" "$MOON" --version)" ;;
esac
# Every scenario's branch exists before sluice protects bolt/*: made here, and from now on moved only by sluice.
for s in s1 s2 s3 s4 s27; do git branch "bolt/$s" main; done
# S27's branch gives each checkout a port and a release command that writes which checkout it released, while
# that checkout still exists, and a preview whose command does nothing (S30).
git switch -q bolt/s27
# shellcheck disable=SC2016 # $SLUICE_CHECKOUT is the release command's, expanded when it runs
sed -e 's|^runner = "moon"$|&\
release = "test -d \\"$SLUICE_CHECKOUT\\" \&\& echo \\"$SLUICE_CHECKOUT\\" >> '"$ROOT"'/released.log"|' \
  .config/sluice.toml >.config/sluice.toml.e2e
mv .config/sluice.toml.e2e .config/sluice.toml
printf '\n[checkout]\nports = ["web"]\nports_from = 42000\n\n[preview]\ncommand = "true"\naddresses = { web = "https://{name.web}.localhost" }\n' >>.config/sluice.toml
git commit -qam "a port, a release command and a preview for S27 and S30"
git switch -q main

# --- three sessions' places, each on a fresh unit branch per scenario ----------------------------------------------
place() { # place <session> <scenario>: the session's worktree on unit/<session>-<scenario>, off bolt/<scenario>
  local dir=$ROOT/places/$1
  [ -d "$dir" ] || git -C "$REPO" worktree add -q --detach "$dir" main
  git -C "$dir" switch -q -c "unit/$1-$2" "bolt/$2"
}
at() { git -C "$ROOT/places/$1" switch -q "unit/$1-$2"; } # at <session> <scenario>: the place back on that branch
edit() {                                                  # edit <session> <path> <line number> <text>: that line replaced, as one commit in the session's place
  local f=$ROOT/places/$1/$2
  awk -v n="$3" -v l="$4" 'NR==n{print l; next} {print}' "$f" >"$f.e2e" && cat "$f.e2e" >"$f" && rm "$f.e2e"
  git -C "$ROOT/places/$1" commit -qam "$1: $2"
}
breaks() { # breaks <session> <path>: the test reading that file made to fail, as one commit
  local f=$ROOT/places/$1/$2
  echo "$BROKEN" >"$f"
  git -C "$ROOT/places/$1" commit -qam "$1: breaks $2"
}

# S1: a and b touch different files; both pass and land in order.
place a s1
edit a "$A_FILE" "$A_LINE" "# a was here"
place b s1
edit b "$B_FILE" "$B_LINE" "# b was here"
# S2: a and c write the same line of one file; c conflicts with a.
place a s2
edit a "$A_FILE" "$A_LINE" "# a says one thing"
place c s2
edit c "$A_FILE" "$A_LINE" "# c says another"
# S3: a passes; b breaks a test, which fails in b's candidate.
place a s3
edit a "$A_FILE" "$A_LINE" "# a was here"
place b s3
breaks b "$B_BREAKS"
# S4: a breaks a test; b passes once formed without a.
place a s4
breaks a "$A_BREAKS"
place b s4
edit b "$B_FILE" "$B_LINE" "# b was here"

FAILED=0
expect() { # expect <what> <actual> <glob>
  # shellcheck disable=SC2254 # the expectation is a glob on purpose
  case $2 in $3) echo "ok    $1: $2" ;; *)
    echo "FAIL  $1: $2 (expected $3)"
    FAILED=1
    ;;
  esac
}

# --- fixture: what sluice must find, shown with git and moon alone -------------------------------------------------
# form <onto> <head>: the candidate commit, or "conflict <paths>" (ADR 0009: git merge-tree decides).
form() {
  local out=$ROOT/merge-tree.out rc=0
  git -C "$REPO" merge-tree --write-tree -z --name-only --no-messages "$1" "$2" >"$out" || rc=$?
  case $rc in
  0) git -C "$REPO" commit-tree "$(tr -d '\0' <"$out")" -p "$1" -p "$2" -m "sluice candidate" ;;
  # a shell variable cannot hold NUL, so the -z output is read from the file: the tree, then one path per
  # field up to the empty field that ends the conflicted-file section
  1) echo "conflict $(tr '\0' '\n' <"$out" | sed -n '2,$p' | sed '/^$/q' | tr '\n' ' ')" ;;
  *) die "merge-tree exited $rc" ;;
  esac
}
# verdict <branch> <candidate>: the candidate's affected #suite tasks run in a detached checkout; pass or fail
verdict() {
  local dir=$ROOT/check-$2 targets rc=0
  git -C "$REPO" worktree add -q --detach "$dir" "$2"
  targets=$(indir "$dir" env MOON_BASE="$1" MOON_HEAD="$2" "$MOON" query tasks --tags suite --affected --downstream deep \
    </dev/null | jq -r '.tasks[][] | .target')
  if [ -z "$targets" ]; then echo "pass (nothing affected)"; else
    # shellcheck disable=SC2086
    indir "$dir" "$MOON" exec $targets --on-failure continue </dev/null >"$dir.log" 2>&1 || rc=$?
    report=$(jq -c '[.actions[] | select(.label | startswith("RunTask(")) | {t: .node.params.target, s: .status}]' \
      "$dir/.moon/cache/runReport.json")
    if [ $rc -eq 0 ]; then echo "pass $report"; else echo "fail $report"; fi
  fi
  git -C "$REPO" worktree remove --force "$dir"
}

if [ "$MODE" = fixture ]; then
  r() { git -C "$REPO" rev-parse "$1"; }
  L=$(r bolt/s1)
  C1=$(form "$L" "$(r unit/a-s1)")
  C2=$(form "$C1" "$(r unit/b-s1)")
  expect "S1 a" "$(verdict "$L" "$C1")" "pass*"
  expect "S1 a+b" "$(verdict "$L" "$C2")" "pass*"
  L=$(r bolt/s2)
  C1=$(form "$L" "$(r unit/a-s2)")
  expect "S2 a+c forms" "$(form "$C1" "$(r unit/c-s2)")" "conflict $A_FILE*"
  expect "S2 a" "$(verdict "$L" "$C1")" "pass*"
  L=$(r bolt/s3)
  C1=$(form "$L" "$(r unit/a-s3)")
  C2=$(form "$C1" "$(r unit/b-s3)")
  expect "S3 a" "$(verdict "$L" "$C1")" "pass*"
  expect "S3 a+b" "$(verdict "$L" "$C2")" "fail*$B_FAILS\",\"s\":\"failed*"
  L=$(r bolt/s4)
  C1=$(form "$L" "$(r unit/a-s4)")
  expect "S4 a" "$(verdict "$L" "$C1")" "fail*$A_FAILS\",\"s\":\"failed*"
  expect "S4 b formed again without a" "$(verdict "$L" "$(form "$L" "$(r unit/b-s4)")")" "pass*"
  exit $FAILED
fi

# --- sluice: S1 to S4 for real -------------------------------------------------------------------------------------
# The host's held ports and session runs, kept with the scratch repository.
export XDG_STATE_HOME=$ROOT/state
# The directory standing for the user's, where each place's scratch is made (35), kept there too: under the gate's
# sandbox, which lets a check write no other, /tmp/sluice-<uid> cannot be made.
export SLUICE_USER_SCRATCH=$ROOT/scratch
COMMON=$(git -C "$REPO" rev-parse --path-format=absolute --git-common-dir)
submit() { # submit <session> <scenario> [flags...]: wt submit from the session's place, as its agent would; the
  #        place is kept on landing, since each session's place serves every scenario
  local who=$1 s=$2
  shift 2
  at "$who" "$s"
  SLUICE_AGENT="e2e-$who" indir "$ROOT/places/$who" wt-submit "bolt/$s" --no-remove "$@"
}
id_of() { submit "$1" "$2" --no-wait --json | jq -r .id; }
status() { indir "$REPO" sluice status "$@" --json --all; }
settle() { # settle <scenario>: rounds until nothing on the branch is queued or under test, for at most an hour
  local i=0
  while status "bolt/$1" | jq -e '[.submissions[] | select(.state == "queued" or .state == "testing")] | length > 0' >/dev/null; do
    indir "$REPO" sluice step "bolt/$1" >/dev/null
    i=$((i + 1))
    [ $i -lt 720 ] || die "bolt/$1 did not settle"
    sleep 5
  done
}
landed() { git -C "$REPO" log --reverse --first-parent --format='%(trailers:key=Sluice-Submission,valueonly)' "main..bolt/$1" | sed '/^$/d' | tr '\n' ' '; }

# S1: two sessions submit to one bolt branch; both candidates pass; both land, in order; neither ran a merge.
A=$(id_of a s1)
out=$(submit b s1) && rc=0 || rc=$?
expect "S1 b's exit" "$rc" 0
expect "S1 b's last line" "$(printf '%s\n' "$out" | tail -1)" "passed · landed as *"
expect "S1 a" "$(status "$A" | jq -r .state)" landed
expect "S1 landing order" "$(landed s1)" "$A $((A + 1)) "
# Newest first: b's landing, then a's; each with its session's head as its second parent.
heads="$(git -C "$REPO" rev-parse unit/b-s1) $(git -C "$REPO" rev-parse unit/a-s1)"
for c in $(git -C "$REPO" rev-list --first-parent main..bolt/s1); do
  expect "S1 $c author" "$(git -C "$REPO" log -1 --format='%an <%ae>' "$c")" "sluice <sluice@localhost>"
  expect "S1 $c second parent" "$(git -C "$REPO" rev-parse "$c^2")" "${heads%% *}"
  heads=${heads#* }
done
expect "S1 moves refused by the hook" "$(grep -c '"event":"protected"' "$COMMON/sluice/bolt/s1/log.jsonl" || true)" 0

# S2: the second head conflicts with the first; told the paths before any check runs; the first lands.
A=$(id_of a s2)
out=$(submit c s2 --check) && rc=0 || rc=$?
expect "S2 c's forecast exit" "$rc" 2
expect "S2 c's forecast" "$out" "would conflict: $A_FILE with #$A (e2e-a, unit/a-s2)"
out=$(submit c s2) && rc=0 || rc=$?
expect "S2 c's exit" "$rc" 2
expect "S2 c's conflict" "$(printf '%s\n' "$out" | sed -n 2p)" "conflict: $A_FILE"
C=$(status bolt/s2 | jq -r '[.submissions[] | select(.from.agent == "e2e-c")][0].id')
expect "S2 c was never tested" "$(status "$C" | jq -r '.tested // "none"')" none
settle s2
expect "S2 a" "$(status "$A" | jq -r .state)" landed

# S3: the first passes and the second fails; the second's session is told the failing checks; the first landed.
A=$(id_of a s3)
out=$(submit b s3) && rc=0 || rc=$?
expect "S3 b's exit" "$rc" 1
expect "S3 b's failure" "$(printf '%s\n' "$out" | grep '^failed:')" "failed: $B_FAILS"
expect "S3 a" "$(status "$A" | jq -r .state)" landed
expect "S3 a's result" "$(status "$A" | jq -r .result)" passed
expect "S3 one blame" "$(grep -c '"event":"blamed"' "$COMMON/sluice/bolt/s3/log.jsonl")" 1

# S4: the first fails and its session is told; the second is formed again without it, passes, and lands first.
A=$(id_of a s4)
B=$(id_of b s4)
settle s4
expect "S4 a" "$(status "$A" | jq -r .state)" failed
expect "S4 a's failure" "$(status "$A" | jq -r '.failed[] | select(.hash) | .task')" "$A_FAILS"
expect "S4 b" "$(status "$B" | jq -r .state)" landed
expect "S4 only b landed" "$(landed s4)" "$B "
expect "S4 b tested without a" "$(status "$B" | jq -c .tested.submissions)" "\[$B\]"
expect "S4 none of a's work on the branch" "$(git -C "$REPO" grep -c -F "$BROKEN" bolt/s4 -- "$A_BREAKS" || echo 0)" 0

# S11 in passing: sluice left every session's place alone.
for w in a b c; do expect "place $w kept" "$(git -C "$ROOT/places/$w" rev-parse --is-inside-work-tree)" true; done

# S27: a session runs its tests twice with sluice exec, with the same port and nothing released between; its
# landing releases its worktree before worktrunk removes it.
released() { cat "$ROOT/released.log" 2>/dev/null | grep -c '/places/d$' || true; }
place d s27
edit d "$A_FILE" "$A_LINE" "# d was here"
P1=$(indir "$ROOT/places/d" sluice exec bolt/s27 --json 2>/dev/null | jq -r '.ports.SLUICE_PORT_WEB')
P2=$(indir "$ROOT/places/d" sluice exec bolt/s27 --json 2>/dev/null | jq -r '.ports.SLUICE_PORT_WEB')
expect "S27 a port" "$P1" "42[0-9][0-9][0-9]"
expect "S27 the same port on the second run" "$P2" "$P1"
expect "S27 nothing released between runs" "$(released)" 0
out=$(SLUICE_AGENT=e2e-d indir "$ROOT/places/d" wt-submit bolt/s27) && rc=0 || rc=$?
expect "S27 d's exit" "$rc" 0
expect "S27 released before removal" "$(released)" 1
expect "S27 d's worktree removed" "$([ -d "$ROOT/places/d" ] && echo kept || echo removed)" removed
held() { # held: how many ports the host still holds, each `ports/held/<port>` (31; record 0016 as amended 2026-11-13)
  set -- "$XDG_STATE_HOME"/sluice/ports/held/*
  if [ -e "$1" ]; then echo $#; else echo 0; fi
}
expect "S27 d's ports free" "$(held)" 0

# S30: the revision d's submission landed is previewed: its command runs and ends, the preview stays up and listed,
# at an address made of its port's name; released at its caller's asking, its checkout goes and its port is free.
R=$(git -C "$REPO" rev-parse bolt/s27)
out=$(indir "$REPO" sluice preview bolt/s27 "$R" --wait --json) && rc=0 || rc=$?
expect "S30 exit" "$rc" 0
expect "S30 up" "$(printf '%s' "$out" | jq -r .state)" up
expect "S30 address" "$(printf '%s' "$out" | jq -r .addresses.web)" "https://web-${R:0:7}--bolt-s27--*.localhost"
N=$(printf '%s' "$out" | jq -r .name)
expect "S30 listed" "$(indir "$REPO" sluice previews --json | jq -r '.[0].name')" "$N"
expect "S30 held" "$(held)" 1
indir "$REPO" sluice release "$N" >/dev/null
expect "S30 released in its checkout" "$(grep -c '/.previews/' "$ROOT/released.log" || true)" 1
expect "S30 its port free" "$(held)" 0
expect "S30 none left" "$(indir "$REPO" sluice previews --json | jq length)" 0
exit $FAILED
