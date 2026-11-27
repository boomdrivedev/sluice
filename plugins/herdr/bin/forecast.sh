#!/bin/sh
# shellcheck disable=SC1091,SC2154,SC2016 # jq programs; common.sh, sourced, sets root, lib, state and config.
# The ahead tokens' forecasts: for each worktree a pane of any herdr
# server is in, by the panes each server's folder keeps (D6), with commits
# ahead of the branch it would land on and nothing else to show, how many,
# and, when its head, the branch's revision or the submissions waiting on
# it moved since the last forecast, a forecast asked again. Each worktree
# once, whichever servers' panes are in it (D10). A forecast is asked on
# the subscription, never by a sluice of its own: this prints a line for
# each, its repository, branch and worktree tab-separated, which the
# watcher writes on the input of the source that serves the repository,
# and the source's reader keeps the forecast that comes back (the change
# `sluices-herdr-plugin-leaves-no-process-and-forecasts-from-its-subscription`,
# D2). Until it comes, the worktree's token says how many ahead alone.
set -u
. "$(dirname "$0")/../lib/common.sh"
. "$lib/sources.sh"

check=$(read_sources | jq -r .ahead_check)
t=$(now)
docs=$(all_docs)
# Each worktree once, and whether every pane in it, in every server, has a
# branch named: their tokens never say ahead.
set -- "$state"/servers/*/panes.json
tops=
[ -e "$1" ] && tops=$(jq -s -r 'map(arrays[]) | [.[] | objects | select(.top)] | group_by(.top)[]
  | [.[0].top, .[0].repo, (all(.named != null) | tostring)] | @tsv' "$@" 2>/dev/null)

old_ifs=$IFS
IFS='
'
for line in $tops; do
  IFS='	' read -r top repo named <<EOF
$line
EOF
  out=$state/checks/$(sum "$top")
  [ "$named" = false ] || {
    rm -f "$out"
    continue
  }
  d=$(printf '%s' "$docs" | jq -c --arg r "$repo" 'map(select(.repository == $r))')
  [ "$(printf '%s' "$d" | jq length)" -gt 0 ] || {
    rm -f "$out"
    continue
  }
  paths='{}'
  for f in "$state"/docs/*.paths; do
    [ -e "$f" ] && paths=$(jq -c --argjson a "$paths" '$a + (objects // {})' "$f" 2>/dev/null || echo "$paths")
  done
  # Nothing to forecast while the worktree's token is about a submission.
  busy=$(jqw -n -r --argjson d "$d" --argjson p "$paths" --arg w "$top" --argjson now "$t" 'include "words";
    $d | pane_token($w; $p; null; $now; null) | test("^#") and (test("^#[0-9]+ landed ") | not)')
  if [ "$busy" = true ]; then
    rm -f "$out"
    continue
  fi
  # The branch it would land on: its latest submission's, its session's,
  # else the only branch there is.
  pick=$(jq -n -c --argjson d "$d" --argjson p "$paths" --arg w "$top" '
    ([$p | to_entries[] | select(.value.path == $w)] | sort_by(.key | tonumber) | last | .value.branch)
      // ([$d[].sessions[] | select(.worktree == $w) | .branch] | first)
      // (if ([$d[].branches[]] | length) == 1 then $d[0].branches[0].branch else null end)
    | . as $b | if $b == null then null else [$d[].branches[] | select(.branch == $b)] | first | {branch, at, ids: ([(.submissions // [])[].id] | sort)} end')
  [ "$pick" != null ] || {
    rm -f "$out"
    continue
  }
  branch=$(printf '%s' "$pick" | jq -r .branch)
  at=$(printf '%s' "$pick" | jq -r '.at // empty')
  ids=$(printf '%s' "$pick" | jq -c .ids)
  head=$(git -C "$top" rev-parse --verify -q HEAD 2>/dev/null) || continue
  count=0
  [ -n "$at" ] && count=$(git -C "$top" rev-list --count "$at..HEAD" 2>/dev/null || echo 0)
  if [ "$count" -eq 0 ]; then
    rm -f "$out"
    continue
  fi
  # What the forecast rests on: a combination starting or ending moves the
  # branch's version but none of these.
  key="$head $at $ids"
  if [ -f "$out" ] && [ "$(jq -r '.key' "$out")" = "$key" ] &&
    [ "$(jq -r '.checked' "$out")" = "$check" ]; then
    continue
  fi
  jq -n -c --arg path "$top" --arg head "$head" --arg key "$key" --arg branch "$branch" \
    --argjson count "$count" --argjson checked "$check" \
    '{path: $path, head: $head, key: $key, branch: $branch, count: $count, result: "none", with: [], paths: [], checked: $checked}' | put "$out"
  [ "$check" = true ] && printf '%s\t%s\t%s\n' "$repo" "$branch" "$top"
done
IFS=$old_ifs
