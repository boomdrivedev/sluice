#!/bin/sh
# shellcheck disable=SC1091,SC2154,SC2016 # jq programs; common.sh, sourced, sets root, lib, state, server and config.
# The sidebar of the herdr server in the environment (D9): the `sluice`
# token on each of its panes whose worktree has something to say,
# `sluice_q` on each of its workspaces with a queue, from its panes as kept
# (D6) and the host's documents. Reports only what changed, unless
# --refresh, which reports every token again so its time to live starts
# over. --read reads the server's panes and keeps them first, as the
# minute's pass does.
set -u
. "$(dirname "$0")/../lib/common.sh"
. "$lib/panes.sh"

refresh=false
read=false
for a in "$@"; do
  case $a in
  --refresh) refresh=true ;;
  --read) read=true ;;
  esac
done
ttl=${SLUICE_HERDR_TTL_MS:-120000}
t=$(now)
# What was last set in this server, its sequence and its panes are its own
# (D3).
mkdir -p "$server"
# A server that does not list its panes is set nothing, and what was set in
# it stays as kept; 3 tells the watcher it did not answer (D2).
if [ "$read" = true ]; then keep_panes "$server" || exit 3; fi
panes=
if [ -f "$server/panes.json" ]; then { IFS= read -r panes <"$server/panes.json"; } 2>/dev/null; fi
[ -n "$panes" ] || panes='[]'
docs=$(all_docs)

# Each document's learned submissions, by repository.
extra='{}'
for f in "$state"/docs/*.json; do
  [ -e "$f" ] || continue
  k=$(basename "$f" .json)
  p=$(jq -c 'objects' "$state/docs/$k.paths" 2>/dev/null)
  [ -n "$p" ] || p='{}'
  extra=$(printf '%s' "$extra" | jq -c --arg r "$(jq -r .repository "$f")" --argjson p "$p" \
    '.[$r] = {paths: ((.[$r].paths // {}) + $p)}')
done

checks='{}'
for c in "$state"/checks/*; do
  # A head's stamp beside its forecast is the watcher's (D10).
  case $c in *.stamp) continue ;; esac
  [ -e "$c" ] || continue
  checks=$(printf '%s' "$checks" | jq -c --slurpfile v "$c" '. + {($v[0].path): $v[0]}')
done

want=$(jqw -n -c --argjson panes "$panes" --argjson docs "$docs" --argjson extra "$extra" \
  --argjson checks "$checks" --argjson now "$t" 'include "words";
  { panes: [ $panes[] | select(.top) | . as $p
      | ($docs | map(select(.repository == $p.repo))) as $d
      | {id: .pane_id, v: (if ($d | length) == 0 then ""
          else $d | pane_token($p.top; $extra[$p.repo].paths // {}; $checks[$p.top]; $now; $p.named) end)} ],
    spaces: [ $panes | group_by(.workspace_id)[] | .[0].workspace_id as $w
      | (map(.repo) | unique) as $repos
      | {id: $w, v: ($docs | map(select(.repository as $r | $repos | index($r))) | queue_token)} ] }')

last=$server/tokens.json
[ -f "$last" ] || echo '{"panes":{},"spaces":{}}' >"$last"
seq_file=$server/seq
n=$(($(cat "$seq_file" 2>/dev/null || echo 0) + 1))
echo "$n" >"$seq_file"

report() {
  # $1 pane|workspace, $2 id, $3 token name, $4 value
  if [ -n "$4" ]; then
    hd "$1" report-metadata "$2" --source agentplot.sluice --seq "$n" --ttl-ms "$ttl" --token "$3=$4" >/dev/null 2>&1
  else
    hd "$1" report-metadata "$2" --source agentplot.sluice --seq "$n" --clear-token "$3" >/dev/null 2>&1
  fi
}

changes=$(jq -n -c -r --argjson want "$want" --slurpfile last "$last" --argjson all "$refresh" '
  ($last[0]) as $l
  | ($want.panes[] | select(($l.panes[.id] // "") != .v or ($all and .v != "")) | ["pane", .id, "sluice", .v]),
    ($want.spaces[] | select(($l.spaces[.id] // "") != .v or ($all and .v != "")) | ["workspace", .id, "sluice_q", .v])
  | @tsv')
old_ifs=$IFS
IFS='
'
for line in $changes; do
  IFS='	' read -r kind id name value <<EOF
$line
EOF
  report "$kind" "$id" "$name" "$value"
done
IFS=$old_ifs
jq -n -c --argjson want "$want" '{panes: ($want.panes | map({(.id): .v}) | add // {}), spaces: ($want.spaces | map({(.id): .v}) | add // {})}' | put "$last"
