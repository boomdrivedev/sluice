#!/usr/bin/env bash
# The plugin's hooks on a scratch repository: the guard refuses a push,
# merge, ref update or pull request merge onto a protected branch and lets
# everything else through; the session note names the protected branches.
# git runs isolated from the user's config and credentials, and `gh` is a
# stand-in, so nothing reaches a network or asks anyone anything.

set -u
here=$(cd "$(dirname "$0")" && pwd)
guard=$here/../scripts/guard.sh
note=$here/../scripts/session-context.sh

# Under sluice's sandbox only SLUICE_TMP is writable beside the checkout.
scratch=$(mktemp -d "${SLUICE_TMP:-${TMPDIR:-/tmp}}/sluice-plugin-hooks.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
scratch=$(cd "$scratch" && pwd -P)

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@localhost
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@localhost
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_ASKPASS SSH_ASKPASS

# A stand-in gh: `gh pr view` answers the base the test sets.
mkdir -p "$scratch/bin"
cat >"$scratch/bin/gh" <<'EOF'
#!/bin/sh
[ -n "${FAKE_GH_BASE:-}" ] || exit 1
echo "$FAKE_GH_BASE"
EOF
chmod +x "$scratch/bin/gh"
export PATH="$scratch/bin:$PATH"

repo=$scratch/repo
git init -q -b main "$repo"
mkdir -p "$repo/.config"
cat >"$repo/.config/sluice.toml" <<'EOF'
[tests]
runner = "command"
command = "true"

[branches."feature/*"]
parallel = 2

[branches.main] # the trunk
remote = "origin"

[branches.main.pull_requests]
reviews = 0
EOF
git -C "$repo" add -A
git -C "$repo" commit -qm policy
git -C "$repo" branch feature/y
git -C "$repo" worktree add -q -b unit/x "$scratch/unit-x" main

plain=$scratch/plain
git init -q -b main "$plain"
git -C "$plain" commit -q --allow-empty -m root

failures=0

run_guard() { # dir command
  jq -n --arg c "$2" --arg d "$1" '{tool_name: "Bash", tool_input: {command: $c}, cwd: $d}' | "$guard"
}

denies() { # dir command
  local out
  out=$(run_guard "$1" "$2")
  if printf '%s' "$out" | jq -e '.hookSpecificOutput.permissionDecision == "deny"' >/dev/null 2>&1 &&
    printf '%s' "$out" | grep -q 'sluice submit'; then
    echo "ok    denies: $2"
  else
    echo "FAIL  should deny: $2 (in $1) -> ${out:-nothing}"
    failures=$((failures + 1))
  fi
}

allows() { # dir command
  local out
  out=$(run_guard "$1" "$2")
  if [ -z "$out" ]; then
    echo "ok    allows: $2"
  else
    echo "FAIL  should allow: $2 (in $1) -> $out"
    failures=$((failures + 1))
  fi
}

wt=$scratch/unit-x

denies "$wt" 'git push origin HEAD:main'
denies "$wt" 'git push origin main'
denies "$wt" 'git push -f origin unit/x:refs/heads/main'
denies "$wt" 'git push origin unit/x:feature/y'
denies "$wt" 'git push --all origin'
denies "$wt" "git -C $repo merge unit/x"
denies "$wt" "cd $repo && git merge --no-ff unit/x"
denies "$wt" 'git update-ref refs/heads/main HEAD'
denies "$wt" 'git status; git push origin HEAD:main'
FAKE_GH_BASE=main denies "$wt" 'gh pr merge 5 --merge'
denies "$wt" 'gh pr merge 5'

allows "$wt" 'sluice status --all'
allows "$wt" 'sluice exec main -- cargo test'
allows "$wt" 'git push origin unit/x'
allows "$wt" 'git push -u origin HEAD'
allows "$wt" 'git push'
allows "$wt" 'git merge main'
allows "$repo" 'git merge --abort'
allows "$wt" 'git update-ref refs/heads/unit/x HEAD'
FAKE_GH_BASE=feature allows "$wt" 'gh pr merge 5'
allows "$wt" 'echo "git push origin main"'
allows "$wt" 'ls -la'
allows "$plain" 'git push origin main'
allows "$plain" 'git merge topic'

# The session note.
out=$(jq -n --arg d "$wt" '{cwd: $d}' | "$note")
# shellcheck disable=SC2016 # backticks are the text
if printf '%s' "$out" | jq -er '.hookSpecificOutput.additionalContext' 2>/dev/null | grep -q '`feature/\*`, `main` only move through it.*Before anything else, run `sluice inbox` once in your worktree.*Act on each, then run `sluice confirm <key>` for it; an outcome you have not confirmed is handed over again by your next `sluice inbox`.*`sluice submit main --no-wait --json`.*start `sluice wait <id> --timeout 9m --json` as a background command.*again the same way for as long as it exits 5.*Under worktrunk, submit and wait in one command instead: start `wt submit main --timeout 9m --json` as a background command, again the same way for as long as it exits 5. It submits once.*removes your worktree once it lands.*never remove a landed worktree by hand.*Wait in the foreground only where your harness runs nothing in the background.*An agent this session starts to work in a worktree submits its own work through sluice from there, told so in the prompt it is given; never ask the user to run `sluice submit`'; then
  echo "ok    the session note names the protected branches, has sluice inbox run first and each outcome confirmed once acted on, has a submission watched from the background, under worktrunk submitted and waited on with wt submit, which removes the landed worktree, and says who submits an agent's work"
else
  echo "FAIL  session note: ${out:-nothing}"
  failures=$((failures + 1))
fi
out=$(jq -n --arg d "$plain" '{cwd: $d}' | "$note")
if [ -z "$out" ]; then
  echo "ok    no note where nothing is protected"
else
  echo "FAIL  a note in a repository with no policy: $out"
  failures=$((failures + 1))
fi

if [ "$failures" -gt 0 ]; then
  echo "$failures failed"
  exit 1
fi
echo "0 failed"
