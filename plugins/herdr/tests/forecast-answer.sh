#!/bin/sh
# A stand-in for the forecast `sluice host` sends on a subscription asked
# `forecast <branch> <worktree>`: the line the watch prints, with the
# worktree's head, and the conflict the test left in check.json, or none.
# $1 the branch, $2 the worktree.
head=$(git -C "$2" rev-parse HEAD 2>/dev/null)
check=$(cat "$FAKE/check.json" 2>/dev/null || echo '{"conflict":null}')
jq -c -n --arg b "$1" --arg f "$2" --arg h "$head" --argjson c "$check" \
  '{branch: $b, folder: $f, forecast: ({head: $h} + $c)}'
