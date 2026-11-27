## ADDED Requirements

### Requirement: Two pushes never reckon one tag

The runs of CI for pushes to main SHALL go one at a time, in the order
of the pushes. They SHALL go in one concurrency group of the workflow. A push's
run SHALL NOT cancel the run that goes. No job SHALL name a group of its
own. The version job SHALL find the next version from the tags that the
repository holds when the job starts. So the version job of a run starts
after the release before it pushed its tag. A pull request's run SHALL
wait for no other run.

#### Scenario: The group of the runs

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** the workflow's group is `release` for a push and the run's
  own for a pull request, with `cancel-in-progress: false`
- **AND** no job names a group

#### Scenario: A second push after the first's release

- **WHEN** `next-version.sh` runs for a second push, with a fix since
  `v0.1.0`, after the release of the first push pushed `v0.1.1`
- **THEN** it prints `v0.1.2` ("two pushes" in
  `check-next-version.sh`)

#### Scenario: A run that a later release holds

- **WHEN** `next-version.sh` runs for the first push, after the release
  of the second push, which holds the first push's commit, pushed
  `v0.1.2`
- **THEN** it prints nothing ("a late run" in `check-next-version.sh`)

## MODIFIED Requirements

### Requirement: CI reckons the next release from the newest tag by version

A release's tag names a commit off main's history. So CI's version job
SHALL run `next-version.sh`, which reckons the next version with
git-cliff's `--unreleased` and `--bumped-version`, whose range starts at
the newest tag. It SHALL give no version where that version has a tag.
Its conventional check SHALL check the commits since the newest `v*` tag
by version, not since the newest tag main's history holds.

#### Scenario: The version and conventional jobs

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `version` job runs `sh next-version.sh`
- **AND** its `conventional` job's range starts at the first of
  `git tag --list 'v*' --sort=-v:refname`, and it runs no
  `git describe`

#### Scenario: No commit that bumps the version

- **WHEN** `next-version.sh` runs where the commits since the newest
  tag are chores alone
- **THEN** it prints nothing ("no bump" in `check-next-version.sh`)
