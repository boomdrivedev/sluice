# Tasks

Each check runs through sluice: `sluice exec main -- <command>`.

## 1. Two pushes never reckon one tag

- [x] 1.1 Put the runs of pushes to main in the workflow's group `release`, with `cancel-in-progress: false`, and take the release job's own group out, in `.github/workflows/ci.yml`. Check: `sluice exec main -- moon run root:release`.
- [x] 1.2 Write `next-version.sh`, and run it in the version job. Check: `sluice exec main -- moon run root:next-version`.
- [x] 1.3 Write `check-next-version.sh` and the task `root:next-version` in `moon.yml`. Check that it fails where `next-version.sh` prints a version that has a tag.
- [x] 1.4 Hold the group in `check-release.sh`. Check that `root:release` fails where the group cancels, names no `release`, or a job names a group.

## 2. The change

- [x] 2.1 Run `openspec validate two-pushes-never-reckon-one-tag --strict`. Check: `sluice exec main -- moon run root:specs root:changes`.
- [x] 2.2 Archive the change with `openspec archive two-pushes-never-reckon-one-tag`.
