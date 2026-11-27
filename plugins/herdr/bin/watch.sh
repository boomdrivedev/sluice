#!/bin/sh
# shellcheck disable=SC1091,SC2154,SC2016 # jq programs; common.sh, sourced, sets root, lib, state, server and config.
# The plugin's one watcher for all of a user's herdr servers on a host
# (0094). `--ensure` registers the server in its environment and wakes the
# lease's watcher where it lives and runs this plugin folder and version,
# else becomes the watcher itself: the command herdr started, herdr's own
# child for its life, which ends when that herdr server ends, and asks
# another server it serves to start the next watcher (the change
# `sluices-herdr-plugin-leaves-no-process-and-forecasts-from-its-subscription`,
# D1; `nothing-detached@f807c7f5fcea`). `--register`, a popup's, registers
# and wakes the watcher, and where none runs has the herdr server start
# one by the plugin's action `watch`; it never becomes the watcher. Running, it
# finds the repositories every server's panes
# are in, and runs each source once, its command and its reader,
# document.sh (D7). A source that names its folders, the default one among
# them, is told each repository's folder on its input, `add <folder>`, and
# `remove <folder>` once no pane of any server is in it: one subscription
# for every repository (the change `the-plugins-hold-one-subscription-each`,
# D1, D2). It keeps each server's sidebar and the forecasts current, until
# it serves no server.
# Between changes it waits on its tick's one sleep, and a tick forks
# nothing unless the shell's own tests find a change (D4, D5).
# `--ticks N` stops it after N ticks, the first at its start (the tests).
set -u
. "$(dirname "$0")/../lib/common.sh"
. "$lib/panes.sh"
. "$lib/sources.sh"
. "$lib/lease.sh"
. "$lib/procs.sh"

lease=$state/watch.lease
tick=${SLUICE_HERDR_TICK:-15}
nl='
'
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/herdr-plugin.toml")

# The lease, read by the shell alone: the pid of the watcher holding it,
# that process's identity (lib/procs.sh), and the plugin folder and version
# it runs, so a reinstall replaces it (D1). Nothing renews it.
read_lease() {
  lpid=
  lid=
  lroot=
  lversion=
  [ -f "$lease" ] || return 1
  # Each line as written: an identity read from ps may end in a space.
  {
    IFS= read -r lpid
    IFS= read -r lid
    IFS= read -r lroot
    IFS= read -r lversion
  } 2>/dev/null <"$lease"
  [ -n "$lpid" ]
}

# Whether the lease's process lives and is still the one it names. Where
# no process could be told from a later one given its pid when it took the
# lease (no ps, no /proc), its pid alone is read.
held() {
  read_lease || return 1
  case $lpid in *[!0-9]*) return 1 ;; esac
  kill -0 "$lpid" 2>/dev/null || return 1
  [ "$lid" = "pid:$lpid" ] || [ "$(identity "$lpid")" = "$lid" ]
}

# Held by a watcher of this plugin folder and version.
ours() {
  held && [ "$lroot" = "$root" ] && [ "$lversion" = "$version" ]
}

# Register the server in the environment (D2): its socket's path, its
# herdr, and a hard link to its socket where the filesystem allows one,
# which tells a server started later at the same path from it. A server is
# served while its folder holds `socket`. One registered at a socket other
# than its link's is another server, which starts with nothing set, so the
# watcher sets every token in it. Under the lock, so a watcher dropping
# the server at that moment cannot remove what is written. `due`, written
# after `socket`, is the watcher's marker that the server asked to be read
# (D4).
register() {
  take_lock || return 1
  mkdir -p "$server"
  sock=${HERDR_SOCKET_PATH:-}
  if [ -n "$sock" ] && [ -e "$sock" ] && ! [ "$sock" -ef "$server/link" ]; then
    [ -e "$server/link" ] && rm -f "$server/tokens.json" "$server/panes.json"
    rm -f "$server/link"
    ln "$sock" "$server/link" 2>/dev/null
  fi
  printf '%s\n' "${HERDR_BIN_PATH:-}" >"$server/herdr"
  printf '%s\n' "$sock" >"$server/socket.tmp.$$" && mv -f "$server/socket.tmp.$$" "$server/socket"
  : >"$server/due"
  drop_lock
}

# The tab-bar segment runs from the host's state, which outlives a
# reinstall; the plugin's own directory does not. The watcher refreshes the
# copy as it starts, and an ensure leaves it (D12).
# Each file replaced in one step, since herdr may be running it.
copy_segment() {
  mkdir -p "$state/segment/bin" "$state/segment/lib"
  for f in bin/segment.sh lib/common.sh lib/words.jq; do
    cp "$root/$f" "$state/segment/$f.tmp.$$" && mv -f "$state/segment/$f.tmp.$$" "$state/segment/$f"
  done
}

case ${1:-} in
--ensure | --register)
  register
  touch "$state/nudge"
  # The lease's watcher, woken, reads this server's panes at once (D4).
  if ours && kill -USR1 "$lpid" 2>/dev/null; then exit 0; fi
  if [ "$1" = --register ]; then
    # None runs: the herdr server starts one as its own child, by the
    # plugin's action, so none is the child of a popup that closes.
    [ -n "${HERDR_SOCKET_PATH:-}" ] &&
      hd plugin action invoke watch --plugin "${HERDR_PLUGIN_ID:-agentplot.sluice}" >/dev/null 2>&1
    exit 0
  fi
  # None: this command is the watcher, herdr's child, which herdr runs
  # with no wait for its end; what it says goes to the log.
  exec </dev/null >>"$state/watch.log" 2>&1
  shift
  ;;
esac

# The process that started this watcher: the herdr server whose command it
# is. Read by the shell's `kill -0` alone, at each tick and wake.
started_by=$PPID

# An ensure's signal, trapped before the lease can name this watcher.
woke=0
trap 'woke=1' USR1

ticks=-1
[ "${1:-}" = --ticks ] && ticks=$2

# Whether this system lets a process be told apart from one given its pid
# later: not where ps cannot run and there is no /proc (sluice's sandbox on
# macOS, where ps is refused).
me=$(identity $$)
if [ "$me" = "pid:$$" ]; then knowable=false; else knowable=true; fi
unchecked=false

# The server it was started from is served, as an ensure would have it.
register

# Take the lease, unless a live watcher of this folder and version holds
# it. One of another folder or version gives way once it reads the lease
# naming another pid.
take_lock || exit 0
if ours; then
  drop_lock
  exit 0
fi
printf '%s\n%s\n%s\n%s\n' $$ "$me" "$root" "$version" >"$lease.tmp.$$" && mv -f "$lease.tmp.$$" "$lease"
drop_lock
copy_segment
say "watching (pid $$)"
[ "$knowable" = true ] ||
  say "cannot tell a process from a later one given its pid here (no ps, no /proc); the lease names this watcher by its pid alone"

# A source's subshell, this watcher's own child, kept with its identity
# (lib/procs.sh) for a later watcher to check. While this watcher lives it
# needs none: an unreaped child keeps its pid, so kill -0 cannot mistake
# another process for it.
remember() {
  printf '%s\tchild:%s\n' "$1" "$(identity "$1")" >>"$2"
}

# A pid kept alone, at its fork, given its identity; dropped if it has
# ended already, so the pid is never taken for whoever gets it next. Where
# no identity can be read it stays as it was.
settle() {
  if kill -0 "$1" 2>/dev/null; then id=$(identity "$1"); else id=; fi
  [ "$id" = "pid:$1" ] && return 0
  awk -F '\t' -v p="$1" -v id="$id" '
    $1 == p && index($2, "pid:" p ":") == 1 { if (id != "") print p "\t" id; next }
    { print }' "$2" >"$2.tmp.$$" &&
    mv -f "$2.tmp.$$" "$2"
}

# Each process in a dead watcher's pids file that is still the one it
# recorded. Where none can be checked, none is: an unknown pid is never
# ended, and that is said once.
listed() {
  [ "$knowable" = true ] || return 0
  while IFS='	' read -r p id; do
    same_process "$p" "${id#child:}" && echo "$p"
  done <"$1"
}

end_listed() {
  for p in $(listed "$1"); do kill "$p" 2>/dev/null; done
}

unknowable() {
  [ "$knowable" = false ] && [ "$unchecked" = false ] || return 0
  unchecked=true
  say "cannot tell an ended watcher's sources from other processes here (no ps, no /proc); left alone"
}

# This watcher's own: its children by kill -0 alone; a source under one of
# them by its identity, or, where none can be read, while that child lives.
own_listed() {
  child=false
  while IFS='	' read -r p id; do
    case $id in child:*) kill -0 "$p" 2>/dev/null && child=true ;; esac
  done <"$1"
  while IFS='	' read -r p id; do
    case $id in
    child:*) kill -0 "$p" 2>/dev/null && echo "$p" ;;
    *) if same_process "$p" "$id" || { [ "$knowable" = false ] && [ "$child" = true ] && kill -0 "$p" 2>/dev/null; }; then echo "$p"; fi ;;
    esac
  done <"$1"
}

# This watcher's sources, apart from any other's. A watcher killed outright
# never ran its stop: its sources are ended here, and forgotten.
runs=$state/run/$$
for d in "$state"/run/*; do
  [ -d "$d" ] || continue
  p=${d##*/}
  [ "$p" = $$ ] && continue
  kill -0 "$p" 2>/dev/null && continue
  unknowable
  for f in "$d"/*.pids; do
    [ -e "$f" ] && end_listed "$f"
  done
  rm -rf "$d"
done
mkdir -p "$runs"

# A former version's state of the host: the reads of whether a repository
# had records, and the blamed tests it learned from sluice, which the
# documents carry now (D3, D4).
rm -rf "$state/probe" "$state/failed"

# A former version's watchers, one for each server, each with its lease
# and state in its server's folder (D11). Their leases go now, each under
# its own lock, so each gives way within its second; once every holder has
# ended, or after two ticks, the sources a killed one listed are ended by
# their identity and each server's former folders go, its watch.log kept.
olds='docs told told.d checks probe want run failed problem nudge dirty no-sha256 probe.err told-unwritable watch.lease lease.lock'
former=false
formers=
for d in "$state"/servers/*; do
  for f in $olds; do [ -e "$d/$f" ] && former=true; done
  [ -f "$d/watch.lease" ] || continue
  p=
  read -r p _ 2>/dev/null <"$d/watch.lease"
  formers="$formers $p"
  host_lock=$lock
  lock=$d/lease.lock
  take_lock && taken=true || taken=false
  rm -f "$d/watch.lease"
  [ "$taken" = true ] && drop_lock
  lock=$host_lock
done

clear_formers() {
  for d in "$state"/servers/*/run/*; do
    [ -d "$d" ] || continue
    unknowable
    for f in "$d"/*.pids; do
      [ -e "$f" ] && end_listed "$f"
    done
  done
  for d in "$state"/servers/*; do
    for f in $olds; do
      [ -e "$d/$f" ] && rm -rf "${d:?}/$f"
    done
  done
  say "a former version's watchers have given way; each server's former state removed, its watch.log kept"
  former=false
}

# Whether every former holder has ended.
formers_ended() {
  for p in $formers; do
    case $p in '' | *[!0-9]*) continue ;; esac
    kill -0 "$p" 2>/dev/null && return 1
  done
  return 0
}

stop() {
  [ -n "${timer:-}" ] && kill "$timer" 2>/dev/null
  for f in "$runs"/*.pids; do
    [ -e "$f" ] || continue
    for p in $(own_listed "$f"); do kill "$p" 2>/dev/null; done
  done
  rm -rf "$runs" "$state/dirty.$$"
  if take_lock; then
    read_lease && [ "$lpid" = $$ ] && rm -f "$lease"
    drop_lock
  fi
}
trap 'stop; exit 0' INT TERM
trap stop EXIT

# Whether a server's socket is gone, or is a file other than its link's.
gone() {
  gone_s=
  read -r gone_s 2>/dev/null <"$1/socket"
  [ -n "$gone_s" ] || return 1
  [ ! -e "$gone_s" ] || { [ -e "$1/link" ] && ! [ "$gone_s" -ef "$1/link" ]; }
}

# A server no longer served: its registration goes, with what was set in it
# and its panes; a former version's watch.log stays.
unregister() {
  rm -rf "$1/socket" "$1/herdr" "$1/link" "$1/tokens.json" "$1/seq" "$1/panes.json" "$1/due"
  rmdir "$1" 2>/dev/null
  eval "missed_${1##*/}=0"
}

# What a reading of the panes of the server in folder $1, made through
# `as`, said of it, by its status $2: 3, no answer (herdr did not list
# them), else an answer. One that has not answered twice in a row is
# dropped, under the lock where it can be had, and 1 returned (D2).
answered() {
  an_k=${1##*/}
  if [ "$2" -ne 3 ]; then
    eval "missed_$an_k=0"
    return 0
  fi
  eval "missed_$an_k=\$((\${missed_$an_k:-0} + 1))"
  eval "an_m=\$missed_$an_k"
  [ "$an_m" -ge 2 ] || return 0
  say "the herdr server at $as_s does not answer; no longer served"
  take_lock && an_t=true || an_t=false
  unregister "$1"
  [ "$an_t" = true ] && drop_lock
  return 1
}

# Each server served, tested at a tick by builtins alone: its socket still
# there and still the file its link is. One whose socket is gone or
# another's is dropped under the lock, if it still is, so a server
# registered again at that moment is kept. A server with no socket's path,
# the tests' stand-in run outside herdr, is asked for its panes, since
# nothing else can tell it gone (D2).
check_servers() {
  for c in "$state"/servers/*; do
    [ -f "$c/socket" ] || continue
    if gone "$c" && take_lock; then
      if gone "$c"; then
        say "the herdr server at $gone_s has stopped; no longer served"
        unregister "$c"
        drop_lock
        continue
      fi
      drop_lock
    fi
    [ -n "$gone_s" ] && continue
    if as "$c" hd pane list >/dev/null 2>&1; then answered "$c" 0; else answered "$c" 3; fi
  done
}

# The watcher lives while it serves a server: once every server's socket
# is gone or another's, or none answers, it stops, with its sources (D2).
enough() {
  for en_f in "$state"/servers/*/socket; do
    [ -f "$en_f" ] && return 0
  done
  say "no herdr server left to serve; stopping"
  exit 0
}

# Set the tokens of the server in folder $1 by tokens.sh with its flags,
# from its panes as kept (D9). With --read it reads them first, and that
# pane list is the call that tells whether the server answers.
tokens_of() {
  tk_d=$1
  shift
  as "$tk_d" sh "$root/bin/tokens.sh" "$@"
  tk_st=$?
  case " $* " in *" --read "*) answered "$tk_d" "$tk_st" ;; esac
}

# Every server's tokens.
each_tokens() {
  for et_d in "$state"/servers/*; do
    [ -f "$et_d/socket" ] && tokens_of "$et_d"
  done
  return 0
}

# Read the panes of the server in folder $1 and keep them there (D6),
# counting a list herdr did not give.
read_of() {
  if as "$1" keep_panes "$1"; then answered "$1" 0; else answered "$1" 3; fi
}

# The one line of the panes kept for the server in folder $1, in $line_v,
# read by the shell: "" where none are kept.
line_of() {
  line_v=
  if [ -f "$1/panes.json" ]; then { IFS= read -r line_v <"$1/panes.json"; } 2>/dev/null; fi
  return 0
}

# Whether the server in folder $1 shows a token of the plugin's: a value in
# the one line of its tokens.json that is not empty, read by the shell.
shows() {
  sh_l=
  IFS= read -r sh_l 2>/dev/null <"$1/tokens.json"
  case $sh_l in *':"'[!\"]*) return 0 ;; esac
  return 1
}

# Whether the server in folder $1 keeps a pane in one of the repositories
# in $dirty_repos, by the shell on its one line of panes: a repository
# whose path JSON would escape, or none named, is taken as one it has a
# pane in (D9).
in_repos() {
  line_of "$1"
  [ -n "$line_v" ] || return 1
  ir=1
  ir_ifs=$IFS
  IFS=$nl
  set -f
  for ir_r in $dirty_repos; do
    case $ir_r in
    - | *[\"\\]* | *[[:cntrl:]]*) ir=0 ;;
    *) case $line_v in *"\"repo\":\"$ir_r\""*) ir=0 ;; *) ir=1 ;; esac ;;
    esac
    [ "$ir" = 0 ] && break
  done
  set +f
  IFS=$ir_ifs
  return "$ir"
}

# The repositories of the documents kept since the last taking, a line
# each in $dirty_repos, as their readers wrote them to `dirty`; 1 where
# none was kept. Taken by one mv, so a reader's line written as it is read
# is left for the next.
take_dirty() {
  dirty_repos=
  [ -e "$state/dirty" ] || return 1
  mv -f "$state/dirty" "$state/dirty.$$" 2>/dev/null || return 1
  while IFS= read -r td_l || [ -n "$td_l" ]; do
    dirty_repos="$dirty_repos${td_l:--}$nl"
  done <"$state/dirty.$$"
  [ -n "$dirty_repos" ] || dirty_repos=-$nl
  return 0
}

# Run one source, as one pipeline of two processes, both the watcher's
# children (D7): its command, in the plugin's config directory, and its
# reader, document.sh, which keeps each document it prints, wakes the
# watcher for each kept, and marks the source ended at its input's end, for
# the next tick to start it again (D4). The command's pid is kept as it
# starts, so a stop before its identity is read still ends it, and its
# identity is read by the watcher itself at its next tick
# (settle_started), with no process started for it; the reader's, as the
# watcher's child. A source that names its folders
# reads a named pipe beside its run file, which the watcher holds open for
# writing on the descriptor $4 while the source runs, so its input stays
# open between looks (D2); any other's input is closed. Neither process
# holds another source's descriptor.
start() {
  # $1 run file, $2 source name, $3 argv as JSON, $4 descriptor, "" for none
  run=$1
  name=$2
  argv=$3
  st_fd=$4
  st_in=/dev/null
  if [ -n "$st_fd" ]; then
    eval "exec $st_fd>&-"
    rm -f "$run.in"
    if ! mkfifo "$run.in"; then
      say "cannot make $run.in; $name is not started"
      return 1
    fi
    st_in=$run.in
  fi
  (
    exec 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
    cd "$config" || exit 1
    eval "set -- $(printf '%s' "$argv" | jq -r 'map(@sh) | join(" ")')"
    me=$(exec sh -c 'echo $PPID')
    printf '%s\tpid:%s:%s\n' "$me" "$me" $$ >>"$run.pids"
    exec "$@"
  ) <"$st_in" | sh "$root/bin/document.sh" "$name" "$run" $$ 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&- &
  remember $! "$run.pids"
  if [ -n "$st_fd" ]; then
    # Opened once the source's side has: each waits for the other.
    eval "exec $st_fd>\"\$run.in\""
    eval "fd_${run##*/}=$st_fd"
  fi
  return 0
}

# Write the line $2 on the input of the source of run file $1, in a
# subshell, so a source that ended ends that subshell and not the watcher;
# where the write fails, the source is marked ended for the next tick to
# start it again (D2).
send() {
  eval "sd_fd=\${fd_${1##*/}:-}"
  if [ -n "$sd_fd" ] && (eval "printf '%s\\n' \"\$2\" >&$sd_fd") 2>/dev/null; then return 0; fi
  { : >"$1.ended"; } 2>/dev/null
  return 1
}

# Close the input the watcher holds of every source.
close_inputs() {
  exec 3>&- 4>&- 5>&- 6>&- 7>&- 8>&- 9>&-
}

alive() {
  [ -f "$1.pids" ] || return 1
  [ -n "$(own_listed "$1.pids")" ]
}

# Whether a source is to be started: its reader marked it ended when the
# look began, or none of its processes runs.
dead() {
  case $ended in *" ${1##*/} "*) return 0 ;; esac
  ! alive "$1"
}

# Keep the git folder of the worktree at $1, for the ticks to test its head
# against its stamp, made here where it has none (D10). Read by the shell
# from the worktree's `.git`, a folder or a file naming one.
keep_head() {
  kh_g=$1/.git
  if [ -f "$kh_g" ]; then
    kh_l=
    IFS= read -r kh_l 2>/dev/null <"$kh_g"
    kh_g=${kh_l#gitdir: }
    case $kh_g in /*) ;; *) kh_g=$1/$kh_g ;; esac
  fi
  [ -d "$kh_g" ] || return 0
  kh_k=$(sum "$1")
  [ -e "$state/checks/$kh_k.stamp" ] || : >"$state/checks/$kh_k.stamp"
  heads="$heads$kh_g	$kh_k$nl"
}

# Whether a kept worktree's head moved: its HEAD or logs/HEAD no older than
# its stamp, a time equal to the stamp's counting as moved, by the shell's
# `-nt` alone; their keys in moved (D10).
heads_moved() {
  moved=
  [ -n "$heads" ] || return 1
  hm_ifs=$IFS
  IFS=$nl
  set -f
  for hm in $heads; do
    hm_g=${hm%	*}
    hm_s=$state/checks/${hm##*	}.stamp
    [ -e "$hm_g/HEAD" ] || continue
    if ! [ "$hm_s" -nt "$hm_g/HEAD" ] ||
      { [ -e "$hm_g/logs/HEAD" ] && ! [ "$hm_s" -nt "$hm_g/logs/HEAD" ]; }; then
      moved="$moved ${hm##*	}"
    fi
  done
  set +f
  IFS=$hm_ifs
  [ -n "$moved" ]
}

# What a look found no pane of any server in (D7): each repository's
# folder is removed from the source that serves it, and its documents,
# learned worktrees and told list go with it, so its return is a first
# sight; a source is never stopped for want of panes. Each worktree's head
# stamp and forecast go too.
gone_by() {
  for gb_f in "$runs"/*.folder; do
    [ -e "$gb_f" ] || continue
    gb_t=
    gb_r=
    {
      IFS= read -r gb_t
      IFS= read -r gb_r
    } 2>/dev/null <"$gb_f"
    case "$nl$found" in *"$nl$gb_r	"*) continue ;; esac
    leave "$gb_f" "$gb_t" "$gb_r"
  done
  set --
  for gb_f in "$state"/checks/*.stamp; do
    [ -e "$gb_f" ] || continue
    gb_k=${gb_f##*/}
    case $heads in *"	${gb_k%.stamp}$nl"*) continue ;; esac
    set -- "$@" "$gb_f" "${gb_f%.stamp}"
  done
  [ $# -gt 0 ] && rm -f "$@"
  return 0
}

# Remove the folder $2 of repository $3 from the source whose file of it
# is $1, and forget the repository: every host's documents of it, by the
# key document.sh keeps them under, as the segment finds them (D2).
leave() {
  lv_f=$1
  lv_t=$2
  lv_r=$3
  rm -f "$lv_f"
  lv_run=${lv_f%.*.folder}
  [ -e "$lv_run.ended" ] || send "$lv_run" "remove $lv_t"
  lv_s=$(name_of "$lv_r")-$(sum "$lv_r")
  set --
  for lv_f in "$state"/docs/*--"$lv_s".* "$state"/told/*--"$lv_s".json; do
    [ -e "$lv_f" ] && set -- "$@" "$lv_f"
  done
  [ $# -gt 0 ] && rm -f "$@"
  say "no pane is in $lv_r; its folder is off the subscription"
}

# Whether the repository $1 is to be added: unless sluice said it has no
# records there less than five minutes since, with no nudge after (D4).
due_add() {
  da_m=$state/marks/$(sum "$1")
  da_l=
  IFS= read -r da_l 2>/dev/null <"$da_m"
  case $da_l in
  "none "*)
    [ $(($(now) - ${da_l#none })) -ge 300 ] || [ "$state/nudge" -nt "$da_m" ]
    return
    ;;
  esac
  return 0
}

# Sources and repositories, and the worktrees whose heads the ticks test,
# from every server's panes as kept (D6, D7): when a server's panes read
# changed, at the popup's asking, and at a tick after a source ended.
look() {
  cfg=$(read_sources)
  # The sources whose readers marked them ended, those marks taken up.
  ended=' '
  set --
  for f in "$runs"/*.ended; do
    [ -e "$f" ] || continue
    set -- "$@" "$f"
    f=${f##*/}
    ended="$ended${f%.ended} "
  done
  [ $# -gt 0 ] && rm -f "$@"
  # Repositories every server's kept panes are in, each with a worktree to
  # run in, and those the popup asked for; each worktree's git folder kept.
  # Whether every server served has its panes kept and read whole, since
  # only then is a repository known to have no pane: a server whose list
  # failed keeps the panes it had.
  lk_all=true
  set --
  for lk_d in "$state"/servers/*; do
    [ -f "$lk_d/socket" ] || continue
    if [ -f "$lk_d/panes.json" ]; then set -- "$@" "$lk_d/panes.json"; else lk_all=false; fi
  done
  lk_n=$#
  listed=
  if [ "$lk_n" -gt 0 ]; then
    listed=$(jq -s -r 'map(arrays[]) | [.[] | objects | select(.repo)]
      | (unique_by(.repo)[] | ["r", .repo, .top]), (unique_by(.top)[] | ["h", .top]) | @tsv' "$@" 2>/dev/null) ||
      lk_all=false
  fi
  found=
  heads=
  old_ifs=$IFS
  IFS=$nl
  set -f
  for line in $listed; do
    case $line in
    "r	"*) found="$found${line#r	}$nl" ;;
    "h	"*) keep_head "${line#h	}" ;;
    esac
  done
  set +f
  IFS=$old_ifs
  for w in "$state"/want/*; do
    [ -e "$w" ] || continue
    d=$(cat "$w")
    rm -f "$w"
    r=$(repo_of "$d") && t=$(top_of "$d") && found="$found$r	$t$nl"
  done
  [ "$lk_n" -gt 0 ] && [ "$lk_all" = true ] && gone_by
  # A sources.toml refused is said in the log and the popup, and no
  # source runs from it (D5).
  problem=$(printf '%s' "$cfg" | jq -r '.refused // empty')
  n=$(printf '%s' "$cfg" | jq '.sources | length')
  i=0
  while [ "$i" -lt "$n" ]; do
    s=$(printf '%s' "$cfg" | jq -c ".sources[$i]")
    i=$((i + 1))
    name=$(printf '%s' "$s" | jq -r .name)
    argv=$(printf '%s' "$s" | jq -c .command)
    run=$runs/$(sum "$name")
    if [ "$(printf '%s' "$s" | jq -r .folders)" != true ]; then
      dead "$run" && {
        rm -f "$run.pids"
        start "$run" "$name" "$argv" ""
      }
      continue
    fi
    if ! command -v sluice >/dev/null 2>&1; then
      problem=$(sluice_problem)
      continue
    fi
    # A descriptor of the watcher's own for each source that names its
    # folders, 3 to 9.
    fd=$((i + 2))
    if [ "$fd" -gt 9 ]; then
      say "$name: no more than seven sources name their folders; not started"
      continue
    fi
    if dead "$run"; then
      rm -f "$run.pids" "$run.ended"
      start "$run" "$name" "$argv" "$fd" || continue
      # The new subscription holds no folder: each the former one served
      # is added again (D1).
      for f in "$run".*.folder; do
        [ -e "$f" ] || continue
        IFS= read -r ft 2>/dev/null <"$f"
        send "$run" "add $ft" || break
      done
    fi
    old_ifs=$IFS
    IFS=$nl
    for line in $found; do
      [ -n "$line" ] || continue
      r=${line%%	*}
      t=${line#*	}
      f=$run.$(sum "$r").folder
      [ -e "$f" ] && continue
      due_add "$r" || continue
      # Kept before the line is written, so the reader can tell the
      # folder's repository from its first document.
      printf '%s\n%s\n' "$t" "$r" >"$f"
      send "$run" "add $t" || {
        rm -f "$f"
        break
      }
    done
    IFS=$old_ifs
  done
  set_problem "$problem"
}

# Whether the server folder $1 is among the lines of $2.
among() {
  case "$nl$2" in *"$nl$1$nl"*) return 0 ;; esac
  return 1
}

# Ask each forecast of the lines $1, a repository, a branch and a
# worktree each, on the input of the source that serves the repository:
# `forecast <branch> <worktree>`, whose answer its reader keeps (D2). One
# no source serves, or whose source has ended, has its forecast's record
# removed, so the next forecast asks it again.
ask_forecasts() {
  af_ifs=$IFS
  IFS=$nl
  set -f
  for af_l in $1; do
    af_r=${af_l%%	*}
    af_l=${af_l#*	}
    af_b=${af_l%%	*}
    af_t=${af_l#*	}
    af_run=
    set +f
    for af_f in "$runs"/*."$(sum "$af_r")".folder; do
      [ -e "$af_f" ] && af_run=${af_f%.*.folder} && break
    done
    set -f
    if [ -z "$af_run" ] || [ -e "$af_run.ended" ] || ! send "$af_run" "forecast $af_b $af_t"; then
      rm -f "$state/checks/$(sum "$af_t")"
    fi
  done
  set +f
  IFS=$af_ifs
  return 0
}

# Do what the reads, the markers and the heads ask (D4, D5, D6, D9, D10).
# The panes of each server in $rd are read and kept; each server in
# $shown, the minute's pass, has its panes read and every token it shows
# set again, in one tokens.sh. A look follows where a server's panes
# changed or $ac_look asks; a forecast, once for every server, where its
# panes changed, a head in $moved moved or a document was kept; then the
# tokens of every server where panes changed or a head moved, else of each
# with a pane in a repository whose document was kept. $ac_all has it do
# all of these as if every server's panes had changed.
act() {
  ac_panes=$ac_all
  ac_ifs=$IFS
  IFS=$nl
  set -f
  # shellcheck disable=SC2086 # the folders, a line each
  set -- $rd
  set +f
  IFS=$ac_ifs
  ac_done=
  for ac_d in "$@"; do
    [ -f "$ac_d/socket" ] || continue
    among "$ac_d" "$ac_done$shown" && continue
    ac_done="$ac_done$ac_d$nl"
    line_of "$ac_d"
    ac_b=$line_v
    read_of "$ac_d"
    line_of "$ac_d"
    [ "$line_v" = "$ac_b" ] || ac_panes=true
  done
  IFS=$nl
  set -f
  # shellcheck disable=SC2086 # the folders, a line each
  set -- $shown
  set +f
  IFS=$ac_ifs
  for ac_d in "$@"; do
    [ -f "$ac_d/socket" ] || continue
    line_of "$ac_d"
    ac_b=$line_v
    tokens_of "$ac_d" --read --refresh
    line_of "$ac_d"
    [ "$line_v" = "$ac_b" ] || ac_panes=true
  done
  enough
  if [ "$ac_panes" = true ] || [ "$ac_look" = true ]; then look; fi
  ac_dirty=false
  take_dirty && ac_dirty=true
  if [ "$ac_panes" = true ] || [ -n "$moved" ] || [ "$ac_dirty" = true ]; then
    ask_forecasts "$(sh "$root/bin/forecast.sh")"
  fi
  if [ "$ac_panes" = true ] || [ -n "$moved" ]; then
    each_tokens
  elif [ "$ac_dirty" = true ]; then
    for ac_d in "$state"/servers/*; do
      [ -f "$ac_d/socket" ] && in_repos "$ac_d" && tokens_of "$ac_d"
    done
  fi
  return 0
}

# Each source's command kept by its pid alone as it started, given its
# identity by this watcher at the tick after, once it has become the
# command; no process is started for it. Where no identity can be read,
# none is tried.
settle_started() {
  [ "$knowable" = true ] || return 0
  for ss_f in "$runs"/*.pids; do
    [ -e "$ss_f" ] || continue
    ss_l=
    while IFS='	' read -r ss_p ss_id; do
      case $ss_id in "pid:$ss_p:"*) ss_l="$ss_l $ss_p" ;; esac
    done <"$ss_f"
    for ss_p in $ss_l; do settle "$ss_p" "$ss_f"; done
  done
  return 0
}

# Whether the herdr server that started this watcher still runs. Where it
# has ended, the watcher stops with its sources, and, where it serves
# another server, asks the first that answers to start the next watcher by
# the plugin's action `watch`, as that server's own child (D1).
owned() {
  kill -0 "$started_by" 2>/dev/null && return 0
  say "the herdr server that started this watcher has ended; stopping"
  stop
  trap - EXIT INT TERM
  for ow_d in "$state"/servers/*; do
    [ -f "$ow_d/socket" ] || continue
    gone "$ow_d" && continue
    if as "$ow_d" hd plugin action invoke watch --plugin "${HERDR_PLUGIN_ID:-agentplot.sluice}" >/dev/null 2>&1; then
      say "the herdr server at $as_s starts the next watcher"
      break
    fi
  done
  exit 0
}

# Whether the lease names this watcher still: one that reads another pid
# there stops with its sources (D1).
mine() {
  read_lease
  [ "$lpid" = $$ ] && return 0
  say "the lease names another watcher (${lpid:-none}); stopping"
  exit 0
}

# Whether an ensure or the popup asked for a reading since the last, their
# markers taken up: the folders of the servers whose ensure left `due`, a
# line each in $due, and whether the popup left a `want/<sum>`, in
# $ak_want, which the look reads (D4).
asked() {
  due=
  ak_want=false
  set --
  for ak_f in "$state"/servers/*/due; do
    [ -e "$ak_f" ] || continue
    set -- "$@" "$ak_f"
    due="$due${ak_f%/due}$nl"
  done
  for ak_f in "$state"/want/*; do
    [ -e "$ak_f" ] && ak_want=true && break
  done
  [ $# -gt 0 ] && rm -f "$@"
  [ -n "$due" ] || [ "$ak_want" = true ]
}

# A wake (D4). The servers whose ensure asked are read now, and again at
# each of the next two ticks, so that what herdr is told of a pane just
# after it opens is read a whole tick after the ensure however soon the
# next tick comes (D6); the popup's repository is looked for now; a
# document kept has the tokens of the servers with a pane in its
# repository set, and what moved forecast. A source's end waits for the
# tick.
wake() {
  owned
  mine
  rd=
  shown=
  moved=
  ac_look=false
  ac_all=false
  if asked; then
    rd=$due
    next1="$next1$due"
    next2="$next2$due"
    ac_look=$ak_want
  fi
  if [ -n "$rd" ] || [ "$ac_look" = true ] || [ -e "$state/dirty" ]; then act; fi
}

# A tick (D5). The shell's own tests read its folder's manifest, the
# lease, each server's socket, the sources' ends, each worktree's head and
# the markers; it forks for what they found changed, for the servers whose
# ensure asked to be read again at this tick, and, every fourth tick, for
# the minute's pass of each server that shows a token.
tock() {
  # Its folder's manifest gone or newer than its lease: the plugin was
  # reinstalled there, or a former version put back. The next server's
  # event starts the watcher of the folder installed (D1).
  if [ ! -e "$root/herdr-plugin.toml" ] || [ "$root/herdr-plugin.toml" -nt "$lease" ]; then
    say "the plugin in $root was replaced or removed; stopping"
    exit 0
  fi
  owned
  mine
  settle_started
  if [ "$former" = true ] && { formers_ended || [ "$ticked" -ge 2 ]; }; then clear_formers; fi
  check_servers
  enough
  # A server read at a tick is read again at the next; one read at a wake,
  # at the next two (D6).
  rd=$next1
  next1=$next2
  next2=
  # The first tick, at its start, takes nothing as kept: it looks, forecasts
  # and sets every server's tokens whatever its reads find.
  ac_look=false
  ac_all=false
  if [ "$ticked" -eq 0 ]; then
    ac_look=true
    ac_all=true
  fi
  if asked; then
    rd="$rd$due"
    next1="$next1$due"
    ac_look=$ak_want
  fi
  # A moved head's stamp is renewed before its forecast reads it, so a
  # commit after the forecast's read is no older than the stamp.
  if heads_moved; then
    for k in $moved; do : >"$state/checks/$k.stamp"; done
  fi
  shown=
  if [ "$ticked" -gt 0 ] && [ $((ticked % 4)) -eq 0 ]; then
    for tk_d in "$state"/servers/*; do
      [ -f "$tk_d/socket" ] && shows "$tk_d" && shown="$shown$tk_d$nl"
    done
  fi
  for f in "$runs"/*.ended; do
    [ -e "$f" ] && ac_look=true && break
  done
  if [ -n "$rd$shown$moved" ] || [ "$ac_look" = true ] || [ -e "$state/dirty" ]; then act; fi
}

ticked=0
# The first tick, at its start, reads every server registered, as their
# ensures would have it, and the next reads them again.
for d in "$state"/servers/*; do
  [ -f "$d/socket" ] && : >"$d/due"
done
next1=
next2=
timer=
heads=
moved=
while :; do
  tock
  enough
  ticked=$((ticked + 1))
  [ "$ticks" -gt 0 ] && ticks=$((ticks - 1))
  if [ "$ticks" -eq 0 ]; then
    # A bounded run lets its sources finish before it stops, their inputs
    # closed; a signal ends the shell's wait, which is taken up again.
    close_inputs
    until wait; do :; done
    each_tokens
    break
  fi
  # The tick's one sleep, waited on until it ends: a signal ends the wait
  # at once, the markers say what changed, and the wait is taken up again
  # on the same sleep (D4).
  sleep "$tick" &
  timer=$!
  while :; do
    if [ "$woke" = 1 ]; then
      woke=0
      wake
      enough
      continue
    fi
    wait "$timer"
    [ "$woke" = 1 ] || break
  done
  timer=
done
