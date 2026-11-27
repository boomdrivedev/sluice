#!/bin/sh
# next-version.sh, run on scratch repositories laid out as CI's releases
# lay out sluice's: each release's tag names a release commit off main,
# its one parent the commit it released. Its cases:
#   one push      a fix since v0.1.0 is v0.1.1;
#   two pushes    the second push, reckoned once the first's v0.1.1 is
#                 tagged, is v0.1.2, never v0.1.1 again;
#   a late run    the first push, reckoned again once the second's release
#                 holds it, has nothing to release;
#   no bump       chores alone since the newest tag release nothing.
# git-cliff is the one devenv.nix pins and reads the tree's cliff.toml
# (core.24). Each repository is under a scratch folder, with no
# configuration of the host's or the user's, and no remote. Each failure
# is one line, next-version: <case>: <what>.
set -u
root=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
status=0

export HOME="$scratch/home" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
mkdir -p "$HOME"
repo="$scratch/repo"
git init -q -b main "$repo"
cp "$root/cliff.toml" "$repo/cliff.toml"
cd "$repo" || exit 1
clock=1700000000

# A commit on the branch checked out, its subject $1, a minute after the
# one before it, so git-cliff orders the tags by time as CI's are.
commit() {
  clock=$((clock + 60))
  echo "$clock" >file
  git add file cliff.toml
  GIT_AUTHOR_DATE="@$clock" GIT_COMMITTER_DATE="@$clock" git commit -q -m "$1"
}

# The release of the commit $1 as $2: a release commit off it, tagged, as
# the release job makes, and main left where it was.
release() {
  git checkout -q --detach "$1"
  commit "chore(release): $2"
  git tag "$2"
  git checkout -q main
}

# What next-version.sh prints with the commit $1 checked out.
next() {
  git checkout -q --detach "$1"
  sh "$root/next-version.sh" 2>/dev/null
  git checkout -q main
}

expect() {
  [ "$3" = "$2" ] || {
    echo "next-version: $1: printed '$3', not '$2'" >&2
    status=1
  }
}

commit "feat: the first"
release main v0.1.0

commit "fix: the first push"
first=$(git rev-parse HEAD)
expect "one push" v0.1.1 "$(next "$first")"

commit "fix: the second push"
second=$(git rev-parse HEAD)
release "$first" v0.1.1
expect "two pushes" v0.1.2 "$(next "$second")"

release "$second" v0.1.2
expect "a late run" "" "$(next "$first")"

commit "chore: no release"
expect "no bump" "" "$(next main)"

[ "$status" -ne 0 ] || echo "next-version: two pushes never reckon one tag"
exit $status
