## Why

The release of #200 failed with `fatal: tag 'v0.110.20' already
exists`. Two pushes to main came close together, #199's and #200's. The
version job of each run started before the release job of the other
ended. So each run found the same newest tag, and each run found
`v0.110.20` as the next version. The release jobs went one at a
time, but the version jobs did not. #199's release pushed `v0.110.20`
first, and #200's release then failed on the same tag.

flume and timber release in the same way, and they have the same race.

## What Changes

- The runs of pushes to main go one at a time, in one concurrency group
  of the workflow, `release`. A run waits for the run before it to end.
  GitHub does not cancel a run that goes. So the version job of a run
  starts after the release before it pushed its tag, and finds that
  tag.
- GitHub keeps one run that waits in a group. A push that comes while a
  run waits takes its place, and GitHub cancels the run that waited. The
  later run releases the commits of that run too, because its range
  starts at the newest tag.
- The release job names no group of its own. In the group of its own
  run, it would wait for its own run.
- `next-version.sh` gives the next version from the tags that the
  repository holds when it starts. It gives nothing where git-cliff's
  version has a tag. The version job runs it.
- `root:next-version` runs `next-version.sh` on scratch repositories.
  A second push after the tag of the first gives the next version. A
  run that a later release holds gives nothing.
- `root:release` holds the workflow to the group, and to no group of a
  job.

## Capabilities

### Modified Capabilities

- `release-version`: two pushes never give one tag. The version job
  runs `next-version.sh`.

## Impact

- `.github/workflows/ci.yml`: the group of the runs, the version job,
  and the release job.
- `next-version.sh`, `check-next-version.sh`: new.
- `check-release.sh`, `moon.yml`: `root:release` and
  `root:next-version`.
