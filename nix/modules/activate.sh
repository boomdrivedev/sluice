#!/bin/sh
# The modules' activation, run as the user who runs sluice (record 0098,
# core.51):
#
#   activate.sh link <file>
#   activate.sh keep <user> <program> <argument>...
#
# links ~/.config/sluice/config.toml to the module's file, so every
# sluice command that user runs reads it. A link there is replaced. A file
# or folder there that is no link is moved once, to
# config.toml.before-nix, and both paths are named. Where
# config.toml.before-nix is there already, it changes neither, names both
# and fails. It makes ~/.config/sluice where it is missing and links the
# file alone, never the folder, which holds the host's keys. It says a
# line only where it moved or linked something. agentplot/boomdrive's
# nix/modules/activate.sh at 3f428cf, its link alone, for sluice's path.
#
# keep is NixOS's step for the daemon's unit, since NixOS restarts no
# changed user unit (record 0093 as amended 2026-11-18; design D10 of
# `sluice-runs-as-a-daemon`): run by <user>, it starts sluice.service
# where it is not running, restarts it where the running daemon's command
# line, by ps, is not <program> <argument>..., the unit's own, and does
# nothing where they are one. Run by any other user, it does nothing. It
# waits for no job, since it runs inside the user's
# nixos-activation.service, and says a line only where it started or
# restarted the unit. boomdrivedev/boomdrive's nix/modules/activate.sh's
# keep, for sluice's unit.
set -eu

usage() {
  echo "usage: activate.sh link <file>" >&2
  echo "       activate.sh keep <user> <program> <argument>..." >&2
  exit 2
}

link() {
  file=$1
  folder=$HOME/.config/sluice
  target=$folder/config.toml
  aside=$target.before-nix

  if [ -L "$target" ] && [ "$(readlink "$target")" = "$file" ]; then
    return
  fi
  if [ -e "$target" ] && [ ! -L "$target" ]; then
    if [ -e "$aside" ] || [ -L "$aside" ]; then
      echo "sluice: $target is no link and $aside is there already: left both, linked nothing; move one aside to link $file" >&2
      exit 1
    fi
    mv "$target" "$aside"
    echo "sluice: moved $target to $aside"
  fi
  mkdir -p "$folder"
  ln -sfn "$file" "$target"
  echo "sluice: linked $target to $file"
}

keep() {
  user=$1
  shift
  unit=sluice.service

  if [ "$(id -un)" != "$user" ]; then
    return
  fi
  pid=$(systemctl --user show --property=MainPID --value "$unit")
  if [ -z "$pid" ] || [ "$pid" = 0 ]; then
    systemctl --user --no-block start "$unit"
    echo "sluice: started $unit"
    return
  fi
  running=$(ps -ww -o args= -p "$pid")
  if [ "$running" != "$*" ]; then
    systemctl --user --no-block restart "$unit"
    echo "sluice: restarted $unit, which ran $running"
  fi
}

case ${1:-} in
link)
  [ $# -eq 2 ] || usage
  link "$2"
  ;;
keep)
  [ $# -ge 3 ] || usage
  shift
  keep "$@"
  ;;
*) usage ;;
esac
