#!/usr/bin/env bash
# PreToolUse on Bash: refuse a push, merge or ref update onto a branch
# sluice protects, naming `sluice submit` instead (rule core.4). sluice's
# own git hook refuses local updates already; this says why before git
# does, and covers a push to the remote and `gh pr merge`, which the local
# hook never sees. Anything it cannot read is let through: the git hook
# and the forge's ruleset still stand.
#
# bash 3.2. Nothing of the command is evaluated: it is split on
# whitespace and shell operators and read as words.

set -u
command -v jq >/dev/null 2>&1 || exit 0

input=$(cat)
cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
dir=$(printf '%s' "$input" | jq -r '.cwd // empty' 2>/dev/null)
[ -n "$cmd" ] || exit 0
[ -n "$dir" ] || dir=$PWD

# One pattern match on every other command, and out.
printf '%s' "$cmd" | grep -Eq '(^|[^[:alnum:]_-])(git[[:space:]].*(push|merge|update-ref)|gh[[:space:]]+pr[[:space:]]+merge)' || exit 0

# shellcheck disable=SC1091 # beside this script
. "$(dirname "$0")/protected.sh"

deny() {
  local reason
  reason="sluice protects $1: only sluice lands work on it. Submit instead, from your worktree with everything committed: \`sluice submit $1\` (or \`wt submit $1\` under worktrunk). See the using-sluice skill."
  jq -n --arg r "$reason" '{hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}'
  exit 0
}

unquote() {
  local w=$1
  w=${w#\"}
  w=${w%\"}
  w=${w#\'}
  w=${w%\'}
  printf '%s' "$w"
}

current_branch() {
  git -C "$1" symbolic-ref -q --short HEAD 2>/dev/null
}

# The protected branch a refspec's destination names, or nothing.
destination() {
  local spec=$1 here=$2 dst
  spec=${spec#+}
  case $spec in
  *:*) dst=${spec#*:} ;;
  *) dst=$spec ;;
  esac
  [ "$dst" = HEAD ] && dst=$(current_branch "$here")
  dst=${dst#refs/heads/}
  case $dst in refs/*) dst= ;; esac
  printf '%s' "$dst"
}

check() {
  local branch=$1 here=$2 patterns
  [ -n "$branch" ] || return 0
  patterns=$(sluice_patterns "$(sluice_policy "$here")")
  [ -n "$patterns" ] || return 0
  if sluice_matches "$branch" "$patterns"; then
    deny "$branch"
  fi
}

segments=$(printf '%s\n' "$cmd" | awk '{ gsub(/&&|\|\||[;|]/, "\n"); print }')

while IFS= read -r seg; do
  # shellcheck disable=SC2206 # split into words on purpose, never evaluated
  read -r -a w <<<"$seg"
  n=${#w[@]}
  i=0
  # Leading VAR=value assignments.
  while [ $i -lt "$n" ] && [[ ${w[$i]} == [A-Za-z_]*=* ]]; do i=$((i + 1)); done
  [ $i -lt "$n" ] || continue
  prog=$(unquote "${w[$i]}")
  i=$((i + 1))
  here=$dir

  case $prog in
  cd)
    [ $i -lt "$n" ] || continue
    to=$(unquote "${w[$i]}")
    case $to in /*) dir=$to ;; ~*) dir=$HOME${to#\~} ;; *) dir=$dir/$to ;; esac
    continue
    ;;
  gh)
    [ "$(unquote "${w[$i]:-}")" = pr ] && [ "$(unquote "${w[$((i + 1))]:-}")" = merge ] || continue
    i=$((i + 2))
    pr=
    while [ $i -lt "$n" ]; do
      a=$(unquote "${w[$i]}")
      case $a in
      -R | --repo | -b | --body | -t | --subject | -F | --body-file | -A | --author-email | --match-head-commit) i=$((i + 1)) ;;
      -*) ;;
      *) [ -n "$pr" ] || pr=$a ;;
      esac
      i=$((i + 1))
    done
    base=$(cd "$here" 2>/dev/null && GH_PROMPT_DISABLED=1 gh pr view ${pr:+"$pr"} --json baseRefName --jq .baseRefName 2>/dev/null </dev/null)
    if [ -n "$base" ]; then
      check "$base" "$here"
    else
      # The base could not be read: refuse where the policy takes pull
      # requests at all, since merging one there is sluice's.
      pulls=$(sluice_pull_patterns "$(sluice_policy "$here")")
      [ -n "$pulls" ] && deny "$(printf '%s' "$pulls" | head -n 1)"
    fi
    continue
    ;;
  git) ;;
  *) continue ;;
  esac

  # git's own options before the subcommand.
  while [ $i -lt "$n" ]; do
    a=$(unquote "${w[$i]}")
    case $a in
    -C)
      here=$(unquote "${w[$((i + 1))]:-}")
      case $here in /*) ;; *) here=$dir/$here ;; esac
      i=$((i + 2))
      ;;
    -c | --git-dir | --work-tree | --namespace) i=$((i + 2)) ;;
    -*) i=$((i + 1)) ;;
    *) break ;;
    esac
  done
  [ $i -lt "$n" ] || continue
  sub=$(unquote "${w[$i]}")
  i=$((i + 1))

  case $sub in
  push)
    args=()
    all=
    while [ $i -lt "$n" ]; do
      a=$(unquote "${w[$i]}")
      case $a in
      --all | --mirror | --branches) all=1 ;;
      -o | --push-option | --repo | --receive-pack | --exec) i=$((i + 1)) ;;
      -*) ;;
      *) args+=("$a") ;;
      esac
      i=$((i + 1))
    done
    if [ -n "$all" ]; then
      patterns=$(sluice_patterns "$(sluice_policy "$here")")
      [ -n "$patterns" ] && deny "$(printf '%s' "$patterns" | head -n 1)"
      continue
    fi
    if [ ${#args[@]} -le 1 ]; then
      # No refspec: the current branch goes to its upstream.
      check "$(current_branch "$here")" "$here"
      continue
    fi
    for spec in "${args[@]:1}"; do
      check "$(destination "$spec" "$here")" "$here"
    done
    ;;
  merge)
    ongoing=
    for a in "${w[@]:i}"; do
      case $(unquote "$a") in --abort | --continue | --quit) ongoing=1 ;; esac
    done
    [ -n "$ongoing" ] || check "$(current_branch "$here")" "$here"
    ;;
  update-ref)
    while [ $i -lt "$n" ]; do
      a=$(unquote "${w[$i]}")
      case $a in
      --stdin) break ;;
      -m)
        i=$((i + 2))
        continue
        ;;
      -*)
        i=$((i + 1))
        continue
        ;;
      esac
      [ "$a" = HEAD ] && a=$(current_branch "$here")
      check "${a#refs/heads/}" "$here"
      break
    done
    ;;
  esac
done <<EOF
$segments
EOF

exit 0
