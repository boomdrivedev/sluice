#!/bin/sh
# The tag of the release the commit checked out makes, from the tags the
# repository holds when it runs: git-cliff's next version from the
# commits since the newest v* tag (cliff.toml). A release's tag names a
# release commit off main, its one parent the commit it released, so
# `--unreleased` starts its range at that tag. It prints nothing, and says
# why on standard error, where git-cliff's version is tagged already:
# where no commit since the newest tag bumps it, and where the commit
# checked out is in that tag's release, which a later push made, when the
# range is empty and git-cliff gives that tag. The version job runs it
# in the workflow's one release group, after the release before it pushed
# its tag, so two pushes never reckon one tag (.github/workflows/ci.yml;
# record 0111; rule core.55). root:next-version runs it on scratch repositories
# (check-next-version.sh).
#
#   sh next-version.sh
set -eu

tag=$(git-cliff --unreleased --bumped-version)
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "nothing to release: $tag is already released" >&2
  exit 0
fi
printf '%s\n' "$tag"
