#!/bin/sh
# shellcheck disable=SC2016 # The workflow's lines name their own $TAG.
# The workflow releases as record 0111 and rule core.55 have it, read from
# .github/workflows/ci.yml's text, job by job:
#   binaries      writes the release's version (set-version.sh), builds by
#                 cargo for its target, given nothing, and fails unless the
#                 binary says that version; installs and runs no Nix, so
#                 that run is on a runner with nothing in /nix/store, and
#                 builds Linux's against musl alone (record 0111 as amended
#                 2026-11-18);
#   release       writes the version, commits the files set-version.sh lists
#                 by path as `chore(release): $TAG`, tags that commit,
#                 pushes the tag alone, with no branch and no force, and
#                 publishes the GitHub release at the tag it pushed;
#   version       reckons the next version from the newest tag, which is off
#                 main's history (git-cliff --unreleased --bumped-version);
#   conventional  checks the commits since the newest v* tag by version,
#                 never `git describe`'s;
# and no line of the workflow names SLUICE_VERSION or --impure, so no build
# is given its version. actionlint, under root:format, reads the workflow's
# keys, expressions and shell; it cannot say what a release's steps must
# be. Each failure is one line, release: <job>: <what>.
set -u
root=$(cd "$(dirname "$0")" && pwd)
cd "$root" || exit 1
workflow=.github/workflows/ci.yml
status=0

fail() {
  echo "release: $1: $2" >&2
  status=1
}

# The job $1's lines, from `  <job>:` to the next job at that indent, each
# shell line continued by `\` joined to the next, its leading space and a
# step's `- run: ` taken off.
job() {
  awk -v job="  $1:" '
    $0 == job { on = 1; next }
    on && /^  [a-z][a-z-]*:$/ { exit }
    on {
      line = $0
      sub(/^[ \t]+/, "", line)
      sub(/^- run: /, "", line)
      if (held != "") line = held " " line
      if (line ~ /\\$/) {
        sub(/[ \t]*\\$/, "", line)
        held = line
        next
      }
      held = ""
      print line
    }
  ' "$workflow"
}

# The number of the first of the lines $1 that is $2 exactly, or holds
# the fixed text $2 where $3 is `in`; empty where none is.
at() {
  if [ "${3:-}" = in ]; then
    printf '%s\n' "$1" | grep -nF -- "$2" | head -n 1 | cut -d : -f 1
  else
    printf '%s\n' "$1" | grep -nxF -- "$2" | head -n 1 | cut -d : -f 1
  fi
}

# Whether the steps $2 come in order in the job $1, each a line number,
# naming each step's words in $3 to $n; says which is missing or out of
# order.
ordered() {
  name=$1
  shift
  previous=0
  said=""
  for step in "$@"; do
    number=${step%%:*}
    words=${step#*:}
    if [ -z "$number" ]; then
      fail "$name" "runs no $words"
      return
    fi
    if [ "$number" -le "$previous" ]; then
      fail "$name" "runs $words ahead of $said, which it must follow"
      return
    fi
    previous=$number
    said=$words
  done
}

binaries=$(job binaries)
release=$(job release)
version=$(job version)
conventional=$(job conventional)
for name in binaries release version conventional; do
  [ -n "$(job "$name")" ] || fail "$name" "the workflow has no such job"
done

# binaries: the release's tree, built with nothing given, saying its
# version on a runner with nothing of Nix's, Linux's static.
ordered binaries \
  "$(at "$binaries" 'sh set-version.sh "${TAG#v}"'):sh set-version.sh \"\${TAG#v}\"" \
  "$(at "$binaries" 'cargo build --release --locked -p sluice --target "$TARGET"'):cargo build --release --locked -p sluice --target \"\$TARGET\"" \
  "$(at "$binaries" 'test "$("target/$TARGET/release/sluice" --version)" = "sluice ${TAG#v}"'):test that target/\$TARGET/release/sluice --version says sluice \${TAG#v}"
if printf '%s\n' "$binaries" | grep -qE 'install-nix-action|(^|[ ;&|(])nix( |$)'; then
  fail binaries "installs or runs Nix: the binaries it builds must run, and run there, on a runner with nothing in /nix/store"
fi
targets=$(printf '%s\n' "$binaries" | sed -n 's/^target: *//p')
[ -n "$targets" ] || fail binaries "names no target in its matrix"
for target in $targets; do
  case $target in
  *-linux-musl) ;;
  *-linux-*) fail binaries "builds $target: Linux's binaries are static, against musl" ;;
  esac
done

# release: the release commit, its tag pushed alone, the release at the tag.
commit=$(printf '%s\n' "$release" | grep -nE 'commit .*-m "chore\(release\): \$TAG" .*-- "\$\{files\[@\]\}"$' | head -n 1 | cut -d : -f 1)
ordered release \
  "$(at "$release" 'sh set-version.sh "${TAG#v}"'):sh set-version.sh \"\${TAG#v}\"" \
  "$(at "$release" 'sh set-version.sh --files' in):the files of sh set-version.sh --files" \
  "$commit:git commit -m \"chore(release): \$TAG\" -- the files set-version.sh lists" \
  "$(at "$release" 'git tag "$TAG"'):git tag \"\$TAG\"" \
  "$(at "$release" 'git push origin "refs/tags/$TAG"'):git push origin \"refs/tags/\$TAG\", the tag alone" \
  "$(at "$release" 'gh release create "$TAG" --verify-tag' in):gh release create \"\$TAG\" --verify-tag"
if printf '%s\n' "$release" | grep -E 'commit ' | grep -Eq '(^| )(-a|--all|-am)( |$)'; then
  fail release "commits with -a: it commits the files set-version.sh lists, by path"
fi
pushes=$(printf '%s\n' "$release" | grep -cE '(^|[ ;&|])git( -c [^ ]+)* push( |$)')
if [ "$pushes" -ne 1 ]; then
  fail release "pushes $pushes times: its one push is the tag, alone"
fi
printf '%s\n' "$release" | grep -E 'git( -c [^ ]+)* push( |$)' | while IFS= read -r push; do
  [ "$push" = 'git push origin "refs/tags/$TAG"' ] ||
    echo "release: release: pushes other than the tag alone, with no branch and no force: $push" >&2
done
if printf '%s\n' "$release" | grep -E 'git( -c [^ ]+)* push( |$)' | grep -vxqF 'git push origin "refs/tags/$TAG"'; then
  status=1
fi
if printf '%s\n' "$release" | grep -qF -- '--target'; then
  fail release "names a --target: the release is published at the tag it pushed"
fi

# version and conventional: reckoned from the newest tag by version.
[ -n "$(at "$version" 'git-cliff --unreleased --bumped-version' in)" ] ||
  fail version "runs no git-cliff --unreleased --bumped-version"
[ -n "$(at "$conventional" "git tag --list 'v*' --sort=-v:refname" in)" ] ||
  fail conventional "does not start at the first of git tag --list 'v*' --sort=-v:refname"
if printf '%s\n' "$conventional" | grep -qF 'git describe'; then
  fail conventional "runs git describe: the newest tag is off main's history"
fi

# The whole workflow: no build given its version.
grep -nF 'SLUICE_VERSION' "$workflow" | while IFS= read -r line; do
  echo "release: workflow: names SLUICE_VERSION, at line ${line%%:*}" >&2
done
grep -nF -- '--impure' "$workflow" | while IFS= read -r line; do
  echo "release: workflow: names --impure, at line ${line%%:*}" >&2
done
if grep -qF -e 'SLUICE_VERSION' -e '--impure' "$workflow"; then
  status=1
fi

[ "$status" -ne 0 ] || echo "release: the workflow releases as record 0111 has it"
exit $status
