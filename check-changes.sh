#!/bin/sh
# Every open change under openspec/changes is valid as OpenSpec takes it
# (record 0128, core.61; boomdrivedev/blueprints design/principles.md 16
# as amended on 2026-11-15): each passes openspec validate --strict, and
# each would archive cleanly. A change archives cleanly where openspec
# archive, in a scratch copy of openspec/ holding the tree's specs, its
# configuration and that change alone, applies the change's deltas, and
# every spec the archive writes passes openspec validate --strict.
# OpenSpec's archive checks the specs it rebuilds without --strict, so a
# warning it lets through, such as a new capability's placeholder
# Purpose, fails here, as root:specs would fail it once the change is
# archived. Each change is applied alone to the tree's specs, as it
# archives at its own merge, and a spec it leaves as it was is
# root:specs' to check. A task left unchecked fails nothing, since a
# change is open while its tasks are worked. A change that fails is
# named, with OpenSpec's words. Boomdrive's check-changes.sh (its record
# 0050, core.61) is the pattern this one copies.
#
#   sh check-changes.sh [<root>]
#
# <root> is the folder holding openspec/, the current folder where it is
# not given.
set -u

[ $# -le 1 ] || {
  echo "usage: check-changes.sh [<root>]" >&2
  exit 2
}
root=${1:-.}
specs=$root/openspec/specs

OPENSPEC_TELEMETRY=0
OPENSPEC_NO_UPDATE_CHECK=1
export OPENSPEC_TELEMETRY OPENSPEC_NO_UPDATE_CHECK

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

status=0
fail() {
  echo "changes: $*" >&2
  status=1
}
# OpenSpec's words in the file `$1`, indented under the line that names
# what failed.
words() {
  sed 's/^/  /' "$1" >&2
}

if ! (cd "$root" && openspec validate --changes --strict --no-interactive) \
  >"$scratch/validate" 2>&1 </dev/null; then
  fail "the open changes fail openspec validate --changes --strict:"
  words "$scratch/validate"
fi

count=0
for dir in "$root"/openspec/changes/*/; do
  [ -d "$dir" ] || continue
  name=${dir%/}
  name=${name##*/}
  [ "$name" != archive ] || continue
  count=$((count + 1))

  copy=$scratch/changes/$name
  mkdir -p "$copy/openspec/changes"
  cp "$root/openspec/config.yaml" "$copy/openspec/"
  if [ -d "$specs" ]; then
    cp -R "$specs" "$copy/openspec/specs"
  fi
  cp -R "${dir%/}" "$copy/openspec/changes/$name"

  if ! (cd "$copy" && openspec archive "$name" --yes) \
    >"$scratch/archive" 2>&1 </dev/null; then
    fail "$name would not archive: openspec archive refuses it:"
    words "$scratch/archive"
    continue
  fi

  # The id of each spec the archive wrote, its folder under the specs,
  # where the tree's specs hold no such file or another one. OpenSpec
  # reads no folder whose name starts with a dot.
  if [ -d "$copy/openspec/specs" ]; then
    (cd "$copy/openspec/specs" && find . -name spec.md -type f) |
      sed -e 's|^\./||' -e 's|/spec\.md$||' | grep -v -e '^\.' -e '/\.' |
      sort >"$scratch/ids"
  else
    : >"$scratch/ids"
  fi
  while read -r id; do
    if cmp -s "$copy/openspec/specs/$id/spec.md" "$specs/$id/spec.md"; then
      continue
    fi
    if ! (cd "$copy" && openspec validate "$id" --type spec --strict --no-interactive) \
      >"$scratch/spec" 2>&1 </dev/null; then
      fail "$name would not archive cleanly: the spec $id it writes fails openspec validate --strict:"
      words "$scratch/spec"
    fi
  done <"$scratch/ids"
done

if [ "$status" -eq 0 ]; then
  echo "changes: $count open, each valid under openspec validate --strict and archiving to specs that are"
fi
exit "$status"
