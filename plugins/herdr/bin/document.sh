#!/bin/sh
# shellcheck disable=SC1091,SC2154,SC2016 # jq programs; common.sh, sourced, sets root, lib, state and config.
# The reader of one source's sluice/overview@1 documents, a line each on
# stdin, from the source named $1. Each line is read whole by one jq
# (lib/keep.jq), with its repository's told list and learned worktrees: it
# is kept as its repository's latest document, its submissions' worktrees
# are learned, and a toast is raised for each landing, blame or conflict
# not yet told (D8), in each herdr server it is told in (D9). A blame's
# tests and a submission's worktree come from the document; the reader
# runs no sluice (the change `the-plugins-hold-one-subscription-each`,
# D3). The rest it forks is the files it replaces and herdr to tell.
# Run by the watcher as a source's reader, with the source's run file $2
# and the watcher's pid $3: it wakes the watcher for each document kept,
# and marks its source ended at its input's end (D4, D7). A line of a
# source that names its folders carries `folder`: the reader marks the
# folder's repository as having records or none, by the folder the watcher
# added for it, and keeps no document of one with none (D4).
set -u
. "$(dirname "$0")/../lib/common.sh"
. "$lib/panes.sh"

source_name=${1:-local}
run=${2:-}
watcher=${3:-}
tab='	'
nl='
'

# Wake the watcher, only while the lease names it, so a reader a killed
# watcher left signals no process given its pid since.
rouse() {
  [ -n "$watcher" ] || return 0
  ro_p=
  IFS= read -r ro_p 2>/dev/null <"$state/watch.lease"
  [ "$ro_p" = "$watcher" ] && kill -USR1 "$watcher" 2>/dev/null
  return 0
}

# Replace file $1 with the line $2 in one step: the shell's redirection
# to a file beside it, and one mv.
write() {
  if printf '%s\n' "$2" 2>/dev/null >"$1.tmp.$$" && mv -f "$1.tmp.$$" "$1" 2>/dev/null; then return 0; fi
  [ -e "$1.tmp.$$" ] && rm -f "$1.tmp.$$"
  return 1
}

# The first line of file $1, by the shell, in $sl_v: "" where there is none.
slurp() {
  sl_v=
  if [ -f "$1" ]; then { IFS= read -r sl_v <"$1" || :; } 2>/dev/null; fi
  return 0
}

# The files of the repository whose key is $key: its told list and
# learned worktrees, read by the shell for the jq.
inputs() {
  slurp "$state/told/$key.json"
  told=$sl_v
  slurp "$state/docs/$key.paths"
  paths=$sl_v
}

# The one jq of a line, for the repository the reader takes it to be.
read_line() {
  out=$(printf '%s\n' "$1" | jqw -R -r --arg hr "$hr" --arg told "$told" --arg paths "$paths" \
    -f "$lib/keep.jq" 2>/dev/null)
}

# The repository the watcher added the folder $1 for, from the files it
# keeps beside the source's run file, a folder and its repository each,
# in $mk_r, and that file in $mk_f; 1 where it added none.
added() {
  mk_r=
  mk_f=
  [ -n "$run" ] || return 1
  for mk_g in "$run".*.folder; do
    [ -e "$mk_g" ] || continue
    mk_a=
    mk_b=
    {
      IFS= read -r mk_a
      IFS= read -r mk_b
    } 2>/dev/null <"$mk_g"
    [ "$mk_a" = "$1" ] || continue
    mk_r=$mk_b
    mk_f=$mk_g
    return 0
  done
  return 1
}

# Mark the repository of folder $1 by how it stands, $2: `records`, or
# `none` with no document of it kept; `unset`, where sluice is not set up
# there or the repository is gone, is `none`, and the folder is off the
# subscription, so the watcher adds it again at a later look (D4).
mark() {
  added "$1" || return 0
  mk_m=$state/marks/$(sum "$mk_r")
  slurp "$mk_m"
  if [ "$2" = records ]; then
    case $sl_v in records\ *) ;; *) write "$mk_m" "records $(now)" ;; esac
    return 0
  fi
  [ "$2" = unset ] && rm -f "$mk_f"
  case $sl_v in none\ *) [ "$2" = unset ] || return 0 ;; esac
  write "$mk_m" "none $(now)"
  # Its documents, of any host, and what was told of them go: a later
  # document with records is a first sight.
  mk_s=$(name_of "$mk_r")-$(sum "$mk_r")
  set --
  for mk_g in "$state"/docs/*--"$mk_s".* "$state"/told/*--"$mk_s".json; do
    [ -e "$mk_g" ] && set -- "$@" "$mk_g"
  done
  [ $# -gt 0 ] && rm -f "$@"
  hr=
  { printf '%s\n' "$mk_r" >>"$state/dirty"; } 2>/dev/null
  rouse
}

# The fields of a tab-separated line $1 into f1, f2, ..., by the shell.
fields() {
  fs_l=$1
  fs_n=0
  while :; do
    fs_n=$((fs_n + 1))
    case $fs_l in
    *"$tab"*)
      eval "f$fs_n=\${fs_l%%\"\$tab\"*}"
      fs_l=${fs_l#*"$tab"}
      ;;
    *)
      eval "f$fs_n=\$fs_l"
      return 0
      ;;
    esac
  done
}

# Keep the forecast of a line `forecast` in $out, from a forecast the
# watcher asked on the source's input, on the record forecast.sh wrote for
# its worktree, where that record still asks it: the same branch and head.
# The servers' tokens are then set again (D2).
forecast_kept() {
  fields "$out"
  fk_f=$state/checks/$(sum "$f2")
  [ -f "$fk_f" ] || return 0
  jq -c --arg b "$f3" --arg r "$f4" --arg h "$f5" --argjson w "$f6" --argjson p "$f7" \
    'select(.branch == $b and .head == $h) | .result = $r | .with = $w | .paths = $p' "$fk_f" 2>/dev/null |
    put "$fk_f" || return 0
  { printf '\n' >>"$state/dirty"; } 2>/dev/null
  rouse
}

# Keep one line a source printed.
keep() {
  read_line "$1"
  case $out in
  forecast"$tab"*)
    forecast_kept
    return 0
    ;;
  folder"$tab"*)
    fd_l=${out%%"$nl"*}
    fields "$fd_l"
    mark "$f2" "$f3"
    [ "$f3" = records ] || return 0
    out=${out#*"$nl"}
    ;;
  key"$tab"*) ;;
  *)
    say "$source_name printed a line it cannot read as an overview document; keeping the last good one"
    return 0
    ;;
  esac
  # A repository other than the one taken: its key and its files, and the
  # line read again with them. A source of one repository reads each of
  # its lines once in a row.
  at=${out%%"$nl"*}
  at=${at#key"$tab"}
  if [ "$at" != "$hr" ]; then
    hr=$at
    key=$(key_of "${at%%"$tab"*}" "${at#*"$tab"}")
    inputs
    read_line "$1"
  fi
  doc=
  learned=
  told_now=
  events=
  old_ifs=$IFS
  IFS=$nl
  set -f
  for l in $out; do
    case $l in
    doc"$tab"*) doc=${l#doc"$tab"} ;;
    paths"$tab"*) learned=${l#paths"$tab"} ;;
    told"$tab"*) told_now=${l#told"$tab"} ;;
    blamed"$tab"* | conflict"$tab"* | landed"$tab"*) events="$events$l$nl" ;;
    esac
  done
  set +f
  IFS=$old_ifs
  [ -n "$doc" ] || return 0
  if ! write "$state/docs/$key.json" "$doc"; then
    say "cannot keep a document in $state/docs"
    return 0
  fi
  write "$state/docs/$key.paths" "$learned" && paths=$learned
  # Nothing is told when it cannot be kept as told, or every document would
  # tell it again: a toast missed is better than a stream of them. Said
  # once, until it can be kept again.
  if write "$state/told/$key.json" "$told_now"; then
    told=$told_now
    [ -e "$state/told-unwritable" ] && rm -f "$state/told-unwritable"
  else
    told=
    events=
    if [ ! -f "$state/told-unwritable" ]; then
      : >"$state/told-unwritable"
      say "cannot record events as told in $state/told: no toasts until it can be written"
    fi
  fi
  # What the toasts say, the worktrees learned first, so the sidebar reads
  # them when the watcher wakes.
  toasts=
  landings=
  old_ifs=$IFS
  IFS=$nl
  set -f
  for e in $events; do
    fields "$e"
    case $f1 in
    blamed)
      toasts="${toasts}sluice: #$f2 blamed on $f3$tab${f4:+$f4 failed. }Run jump to go to its pane.$nl"
      ;;
    conflict)
      toasts="${toasts}sluice: #$f2 conflicts on $f3$tab$f4$nl"
      ;;
    landed) landings="$landings$f2$tab$f3$tab$f4$tab$f5$nl" ;;
    esac
  done
  # IFS stays a newline and globbing off until the toasts are told.
  # The repository, a line in `dirty`, so the watcher sets the tokens of the
  # servers with a pane in it (D9).
  { printf '%s\n' "${hr#*"$tab"}" >>"$state/dirty"; } 2>/dev/null
  rouse
  [ -n "$toasts$landings" ] || {
    set +f
    IFS=$old_ifs
    return 0
  }
  servers
  # A blame or a conflict is told in each server with a pane in the
  # repository, or in every server where none has one (D9).
  if [ -n "$toasts" ]; then
    to=
    for d in $sv; do
      keeps "$d" repo "${hr#*"$tab"}" && to="$to$d$nl"
    done
    tell "${to:-$sv}" "$toasts"
  fi
  # A landing is told in each server where its pane is open and is not the
  # one in front, read in that server alone; asked only of a server that
  # keeps a pane in a landed worktree, or keeps no panes.
  for d in $sv; do
    [ -n "$landings" ] || break
    near=false
    for l in $landings; do
      fields "$l"
      keeps "$d" top "$f3"
      [ $? = 1 ] || {
        near=true
        break
      }
    done
    [ "$near" = true ] || continue
    if [ "$d" = - ]; then f=$server; else f=$d; fi
    front=$(on "$d" panes_json --front "$f")
    told_here=$(jq -n -r --argjson p "$front" --arg l "$landings" '
      $l | splits("\n") | select(length > 0) | split("\t") | . as [$id, $b, $path, $to]
      | select([$p[] | select(.top == $path)] | length > 0 and (any(.front) | not))
      | "sluice: #\($id) landed on \($b)\tas \($to)"' 2>/dev/null)
    [ -n "$told_here" ] && tell "$d" "$told_here"
  done
  set +f
  IFS=$old_ifs
}

# The folders of the herdr servers the watcher serves, a line each in $sv;
# where it serves none, `-`, the server in the environment, as a reader
# run by hand tells in (D9). Called with globbing off, which it turns on
# for its own glob alone.
servers() {
  sv=
  set +f
  for sv_d in "$state"/servers/*; do
    [ -f "$sv_d/socket" ] && sv="$sv$sv_d$nl"
  done
  set -f
  [ -n "$sv" ] || sv=-$nl
}

# Whether the server of folder $1 keeps a pane whose field $2 is $3, read by
# the shell from its one line of panes (D6): 0 it does, 1 it does not, 2 it
# keeps none. A value JSON would escape is taken as one it keeps.
keeps() {
  [ "$1" != - ] || return 2
  slurp "$1/panes.json"
  [ -n "$sl_v" ] || return 2
  case $3 in *[\"\\]* | *[[:cntrl:]]*) return 0 ;; esac
  case $sl_v in *"\"$2\":\"$3\""*) return 0 ;; esac
  return 1
}

# Run a command as the server of folder $1 runs one, `-` the environment's.
on() {
  on_d=$1
  shift
  if [ "$on_d" = - ]; then "$@"; else as "$on_d" "$@"; fi
}

# Raise each toast of $2, a line each, its title and body tab-separated, in
# each server of $1, a folder a line. Called with IFS a newline and
# globbing off.
tell() {
  for tl_d in $1; do
    for tl_e in $2; do
      on "$tl_d" hd notification show "${tl_e%%"$tab"*}" --body "${tl_e#*"$tab"}" >/dev/null 2>&1
    done
  done
}

hr=
key=
told=
paths=
while IFS= read -r line || [ -n "$line" ]; do
  keep "$line"
  line=
done
if [ -n "$run" ]; then
  { : >"$run.ended"; } 2>/dev/null
  rouse
fi
exit 0
