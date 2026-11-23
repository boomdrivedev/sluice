# shellcheck shell=sh
# What every script of the plugin shares: where its state is, how it calls
# herdr and sluice, and how it names a repository. Sourced, never run.

: "${HERDR_PLUGIN_ROOT:=$(cd "$(dirname "$0")/.." && pwd -P)}"
: "${HERDR_PLUGIN_STATE_DIR:=${XDG_STATE_HOME:-$HOME/.local/state}/herdr-agentplot.sluice}"
: "${HERDR_PLUGIN_CONFIG_DIR:=${XDG_CONFIG_HOME:-$HOME/.config}/herdr/plugins/config/agentplot.sluice}"
root=$HERDR_PLUGIN_ROOT
# The host's state, one for every herdr server of the user's: the lease,
# each repository's documents and learned worktrees, the events told,
# forecasts, the marks of which repositories have records, the popup's
# asks, the problem with sluice, and the tab-bar copy, at a path that
# lasts (D3).
state=$HERDR_PLUGIN_STATE_DIR
# The folder of the herdr server in the environment, told apart by its
# socket: what is that server's alone, the tokens last set in it and their
# sequence. Made only by what writes there, since a script run with no
# server, the segment's, is no server's.
# shellcheck disable=SC2034 # read by the scripts that source this
server=$state/servers/$(printf '%s' "${HERDR_SOCKET_PATH:-none}" | cksum | cut -d' ' -f1)
# shellcheck disable=SC2034 # read by the scripts that source this
config=$HERDR_PLUGIN_CONFIG_DIR
lib=$root/lib
mkdir -p "$state/docs" "$state/told" "$state/checks" "$state/marks" "$state/want" "$state/run"

# The one source of the time, so the tests can fix it.
now() {
  echo "${SLUICE_HERDR_NOW:-$(date +%s)}"
}

say() {
  printf '%s agentplot.sluice: %s\n' "$(date +%H:%M:%S)" "$*" >&2
}

hd() {
  "${HERDR_BIN_PATH:-herdr}" "$@"
}

# Run a command as the server registered in folder $1 runs one: with that
# server's socket and herdr in its environment (D2). The socket read is
# left in as_s, for what the caller says of the server.
as() {
  as_d=$1
  shift
  as_s=
  as_b=
  read -r as_s 2>/dev/null <"$as_d/socket"
  read -r as_b 2>/dev/null <"$as_d/herdr"
  (
    # The export is meant to stay in this subshell, with the call it is for.
    # shellcheck disable=SC2030
    if [ -n "$as_s" ]; then export HERDR_SOCKET_PATH="$as_s"; else unset HERDR_SOCKET_PATH; fi
    if [ -n "$as_b" ]; then export HERDR_BIN_PATH="$as_b"; else unset HERDR_BIN_PATH; fi
    "$@"
  )
}

# jq with the plugin's words and rules loaded.
jqw() {
  jq -L "$lib" "$@"
}

# The checksum of a string: a file name for a path.
sum() {
  printf '%s' "$1" | cksum | cut -d' ' -f1
}

# The repository holding a directory (its git common dir, absolute and
# without links), or nothing outside git.
repo_of() {
  d=$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || return 1
  (cd "$d" 2>/dev/null && pwd -P)
}

# The top of the worktree holding a directory.
top_of() {
  t=$(git -C "$1" rev-parse --show-toplevel 2>/dev/null) || return 1
  (cd "$t" 2>/dev/null && pwd -P)
}

# The state key of a document: its host, its repository's name and a
# checksum of the repository's path.
key_of() {
  echo "$1--$(name_of "$2")-$(sum "$2")"
}

# A repository's name: its directory's, without `.git`, and its worktree's
# when the repository is the `.git` inside one.
name_of() {
  basename "${1%/.git}" .git
}

# Replace a file in one step, from stdin; nothing in, nothing replaced.
put() {
  tmp="$1.tmp.$$"
  if cat >"$tmp" && [ -s "$tmp" ]; then mv -f "$tmp" "$1"; else
    rm -f "$tmp"
    return 1
  fi
}

# Why sluice cannot be asked here, or nothing: not on PATH, or what the
# watcher found, a sources.toml it refused.
sluice_problem() {
  if ! command -v sluice >/dev/null 2>&1; then
    echo "sluice is not on PATH: install sluice."
    return
  fi
  cat "$state/problem" 2>/dev/null
}

# Note a problem with sluice for the popup, logging it once; none clears it.
set_problem() {
  if [ -z "$1" ]; then
    rm -f "$state/problem"
  elif [ "$(cat "$state/problem" 2>/dev/null)" != "$1" ]; then
    printf '%s\n' "$1" >"$state/problem"
    say "$1"
  fi
}

# Have the host's watcher serve the herdr server in the environment, where
# a lease stands and that server is not served, or is served at a socket
# another server has since taken: the popup's or the setup action's server
# the watcher dropped, or never heard from (D2). The environment's socket is
# read as the process was given it: as() changes it only in its subshell.
# shellcheck disable=SC2031
enlist() {
  [ -n "${HERDR_SOCKET_PATH:-}" ] && [ -f "$state/watch.lease" ] || return 0
  if [ -f "$server/socket" ] && { [ ! -e "$server/link" ] || [ "$HERDR_SOCKET_PATH" -ef "$server/link" ]; }; then
    return 0
  fi
  sh "$root/bin/watch.sh" --ensure
}

# Every document the watcher holds, as one array.
all_docs() {
  set -- "$state"/docs/*.json
  if [ -e "$1" ]; then jq -s . "$@"; else echo '[]'; fi
}
