#!/bin/sh
# shellcheck disable=SC1091,SC2154,SC2016 # jq programs; common.sh, sourced, sets root, lib, state and config.
# The status popup, and with --dashboard the same view in its own tab:
# the focused pane's repository's queue, read from the host's state and
# redrawn as it changes. Keys: q or Escape closes, j jumps to the culprit's
# pane, a switches between this repository and every one held, the arrows
# and Page Up and Down scroll a frame taller than the pane.
# --once prints one frame and exits.
set -u
. "$(dirname "$0")/../lib/common.sh"

all=false
once=false
for a in "$@"; do
  case $a in
  --dashboard) all=true ;;
  --once) once=true ;;
  esac
done

# The directory of the pane the popup was opened over.
ctx=${HERDR_PLUGIN_CONTEXT_JSON:-}
[ -n "$ctx" ] || ctx='{}'
dir=${SLUICE_HERDR_DIR:-$(printf '%s' "$ctx" |
  jq -r '.focused_pane_cwd // .workspace_cwd // empty' 2>/dev/null)}
repo=
[ -n "$dir" ] && repo=$(repo_of "$dir")
name=
[ -n "$repo" ] && name=$(name_of "$repo")
# The branch named for that pane by its `sluice_branch` token, which its
# repository's view shows first.
pane=$(printf '%s' "$ctx" | jq -r '.focused_pane_id // empty' 2>/dev/null)
named=
[ -n "$pane" ] && named=$(hd pane list 2>/dev/null | jq -r --arg p "$pane" '[.result.panes // [] | .[]
  | select(.pane_id == $p) | .tokens.sluice_branch | strings | gsub("[[:cntrl:]]"; "")] | first // empty' 2>/dev/null)

[ -t 1 ] && tty=true || tty=false
# Rows are OSC 8 links on a terminal (SLUICE_HERDR_LINKS=1 asks for them
# anywhere); the popup numbers its link rows for keys, the dashboard, a
# pane where herdr's Ctrl-click reaches them, does not.
links=$tty
[ "${SLUICE_HERDR_LINKS:-}" = 1 ] && links=true
numbers=true
[ "$all" = true ] && numbers=false
links_file=$state/popup-links.$$
trap 'rm -f "$links_file" "$top_file"' EXIT
# The first row drawn, moved by the scroll keys; each frame keeps it
# within its rows and says where it stopped.
top=0
top_file=$state/popup-top.$$

# The frame in $1 lines at most, its footer's two among them, or in as
# many as it takes when $1 is empty.
frame() {
  t=$(now)
  docs=$(all_docs)
  every=$all
  note=
  # A pane in no repository: every repository held, saying so.
  if [ "$all" = false ] && [ -z "$repo" ] && [ "$(printf '%s' "$docs" | jq length)" -gt 0 ]; then
    every=true
    if [ -n "$dir" ]; then
      note="$(printf '%s' "$dir" | tr -d '\000-\037\177') is in no git repository. Every repository held:"
    else
      note="The pane's folder is not known. Every repository held:"
    fi
  fi
  height=null
  [ -n "${1:-}" ] && height=$(($1 - 2))
  shown=$(printf '%s' "$docs" | jqw -c --argjson all "$every" --arg r "$repo" --arg n "$name" '
    include "words";
    if $all then . else map(select(.repository == $r or (.repository | repo_name) == $n)) end
    | sort_by(.repository, .host)')
  if [ "$(printf '%s' "$shown" | jq length)" -gt 0 ]; then
    drawn=$(printf '%s' "$shown" | jqw -c --argjson now "$t" --argjson color "$tty" --argjson numbers "$numbers" \
      --arg note "$note" --argjson height "$height" --argjson top "$top" --arg r "$repo" --arg n "$name" --arg named "$named" '
      include "words";
      (if $note == "" then [] else [{text: $note, kind: "heading"}, {text: "", kind: "blank"}] end)
      + [.[] | (if $named != "" and (.repository == $r or (.repository | repo_name) == $n) then $named else null end) as $first
          | rows($now; $color; $first)[]]
      | fit($height; $top; $color) | if $numbers then numbered else . end')
    printf '%s' "$drawn" | jq -r '.[] | select(.n) | .url' >"$links_file"
    printf '%s' "$drawn" | jq -r '[.[] | select(.kind == "above") | .top] | first // 0' >"$top_file"
    printf '%s' "$drawn" | jqw -r --argjson links "$links" 'include "words"; .[] | line($links)'
  elif [ -n "$(sluice_problem)" ]; then
    sluice_problem
  elif [ "$all" = true ]; then
    echo "No repository with sluice's records is open in a pane."
  elif [ -z "$repo" ]; then
    echo "No sluice records in this repository."
  elif [ "$(cut -d' ' -f1 "$state/marks/$(sum "$repo")" 2>/dev/null)" = none ]; then
    echo "No sluice records in this repository."
  else
    echo "Waiting for sluice to read $repo..."
  fi
  echo
  if [ "$all" = true ]; then
    echo "q close · j jump to the blamed submission's pane · a this repository"
  else
    echo "q close · j jump to the blamed submission's pane · a every repository"
  fi
}

# The terminal's height, read again at each draw since a pane is resized:
# from the terminal itself, else terminfo, else LINES, else nothing.
height() {
  h=
  if [ "$tty" = true ]; then
    h=$(stty size </dev/tty 2>/dev/null | cut -d' ' -f1)
    [ -n "$h" ] && [ "$h" -gt 0 ] 2>/dev/null || h=$(tput lines 2>/dev/null)
  fi
  [ -n "$h" ] && [ "$h" -gt 0 ] 2>/dev/null || h=${LINES:-}
  [ -n "$h" ] && [ "$h" -gt 0 ] 2>/dev/null || h=
  echo "$h"
}

# A frame taller than the terminal would scroll it, and each redraw from
# the top would leave another copy of the frame's head above: the frame is
# fitted to the height, and its last line ends without a newline.
draw() {
  if [ "$tty" = true ]; then
    esc=$(printf '\033')
    h=$(height)
    page=$((${h:-12} - 4))
    [ "$page" -gt 0 ] || page=1
    out=$(frame "$h" | sed "s/\$/${esc}[K/")
    printf '\033[H%s\033[J' "$out"
  else
    h=$(height)
    page=$((${h:-12} - 4))
    [ "$page" -gt 0 ] || page=1
    frame "$h"
    printf '\f\n'
  fi
  top=$(cat "$top_file" 2>/dev/null || echo 0)
}

# A repository the watcher has not read yet: asked for, and the watcher,
# woken, or started by this herdr server as its own child, looks for it at
# once (D13). Else, where a watcher has
# the lease and does not serve this popup's server, that server is
# registered with it (D2).
if [ -n "$repo" ] && [ "$(all_docs | jq --arg r "$repo" 'any(.repository == $r)')" = false ]; then
  printf '%s\n' "$dir" >"$state/want/$(sum "$repo")"
  sh "$root/bin/watch.sh" --register
else
  enlist
fi

if [ "$once" = true ]; then
  frame
  exit 0
fi

if [ -t 0 ]; then
  saved=$(stty -g)
  trap 'stty "$saved"; printf "\033[?1007l\033[?1049l\033[?7h\033[?25h"; rm -f "$links_file" "$top_file"' EXIT
  trap 'exit 0' INT TERM HUP
  stty -icanon -echo min 0 time 10
  # No wrapping either: a line wider than the pane is cut at its edge
  # rather than taking a second line. The alternate screen, with its
  # wheel sent as arrow keys where the terminal can, so the wheel scrolls.
  printf '\033[?1049h\033[?1007h\033[?7l\033[?25l\033[2J'
fi

draw
while :; do
  key=$(dd bs=1 count=1 2>/dev/null)
  if [ -z "$key" ]; then
    # A second without a key on a terminal; the end of input otherwise.
    [ -t 0 ] || exit 0
    draw
    continue
  fi
  case $key in
  q) exit 0 ;;
  "$(printf '\033')")
    # Escape alone closes; an arrow key's bytes follow at once. Off a
    # terminal, a key's two bytes after its escape are read as one.
    if [ -t 0 ]; then
      stty time 1
      more=$(dd bs=64 count=1 2>/dev/null)
      stty time 10
    else
      more=$(dd bs=1 count=2 2>/dev/null)
      case $more in "[5" | "[6") more=$more$(dd bs=1 count=1 2>/dev/null) ;; esac
    fi
    [ -z "$more" ] && exit 0
    # The wheel sends several arrows at once: each one is a line.
    case $more in
    *A*) top=$((top - $(printf '%s' "$more" | tr -cd A | wc -c))) ;;
    *B*) top=$((top + $(printf '%s' "$more" | tr -cd B | wc -c))) ;;
    *5~*) top=$((top - page)) ;;
    *6~*) top=$((top + page)) ;;
    esac
    [ "$top" -ge 0 ] || top=0
    ;;
  j)
    sh "$root/bin/jump.sh"
    exit 0
    ;;
  [1-9])
    # What a Ctrl-click on the row numbered so would do.
    url=$(sed -n "${key}p" "$links_file" 2>/dev/null)
    if [ -n "$url" ]; then
      sh "$root/bin/link.sh" "$url"
      exit 0
    fi
    ;;
  a)
    if [ "$all" = true ]; then all=false; else all=true; fi
    top=0
    ;;
  esac
  draw
done
