# release-version Specification

## Purpose
How sluice is released, so that a build of a release's tag reports that
release's version from its tree alone, whoever builds it. Main's tree
names `0.0.0`. `set-version.sh` writes a release's version into every
file that carries sluice's version. A release's tag names a release
commit cut from the pushed commit, and nothing reaches main. The
release's binaries are built from that tree with nothing given. CI
reckons the next release from the newest tag by version
(agentplot/blueprints `design/principles.md` 14; agentplot/boomdrive
record 0023; rule core.55; record 0111).

## Requirements

### Requirement: Main's tree names no release

On main, every file that carries sluice's version SHALL give `0.0.0`:
`Cargo.toml`'s `[workspace.package]` version, which each of sluice's
crates takes; `Cargo.lock`'s entry for each of sluice's own crates,
those with no `source`; `plugins/sluice/.claude-plugin/plugin.json`;
`plugins/sluice-board/.claude-plugin/plugin.json`; and
`plugins/herdr/herdr-plugin.toml`'s own `version`, ahead of its first
table.

#### Scenario: Main's files

- **WHEN** `root:versions` reads main's tree
- **THEN** `Cargo.toml`'s workspace version is `0.0.0`, and each of
  sluice's crates in `Cargo.lock` and each plugin manifest gives `0.0.0`

#### Scenario: A file that gives another version

- **WHEN** `plugins/sluice-board/.claude-plugin/plugin.json` gives
  `0.1.0` and the workspace `0.0.0`
- **THEN** `root:versions` fails, naming the file, the version it gives
  and the workspace's

### Requirement: The marketplace names no version

The marketplace's entries, `.claude-plugin/marketplace.json`, SHALL
give no version, so each plugin's version is said once, in its
manifest.

#### Scenario: An entry that gives a version

- **WHEN** the marketplace's entry for `sluice-board` gives a `version`
- **THEN** `root:versions` fails, naming the entry

### Requirement: Every plugin manifest is among the files the writer writes

Every plugin manifest the tree holds under `plugins/`, each
`plugins/*/.claude-plugin/plugin.json` and `plugins/*/herdr-plugin.toml`,
SHALL be among the files `set-version.sh` writes.

#### Scenario: A plugin manifest the writer leaves out

- **WHEN** the tree holds a plugin manifest under `plugins/` that
  `sh set-version.sh --files` does not list
- **THEN** `root:versions` fails, naming the manifest

### Requirement: set-version.sh writes a release's version into every file that carries sluice's version

`sh set-version.sh <x.y.z> [<root>]` SHALL write the version as the
version of each file of the first requirement, under the root, the
folder the script is in by default. It SHALL change no other line. It
SHALL refuse a version that is not `x.y.z` before it writes anything. It
SHALL refuse a file in which it finds no version to write, naming the
file. Each refusal exits non-zero. `sh set-version.sh --files` SHALL
print the files it writes, one path from the root on each line, and
write nothing.

#### Scenario: A release's version

- **WHEN** `sh set-version.sh 98.76.54 <copy>` runs on a scratch copy of
  main's files
- **THEN** each file gives `98.76.54` where main's gives `0.0.0`
- **AND** each file's difference from main's is exactly the lines that
  give the version

#### Scenario: Not a version

- **WHEN** `sh set-version.sh 0.88 <copy>` runs
- **THEN** it exits non-zero, saying `0.88` is not a version
  `major.minor.patch`, and no file of the copy changes

#### Scenario: A file with no version

- **WHEN** `sh set-version.sh 98.76.54 <copy>` runs where the copy's
  `plugins/herdr/herdr-plugin.toml` gives no `version` ahead of its
  first table
- **THEN** it exits non-zero, naming `plugins/herdr/herdr-plugin.toml`

#### Scenario: The files it writes

- **WHEN** `sh set-version.sh --files` runs
- **THEN** it prints `Cargo.toml`, `Cargo.lock`,
  `plugins/sluice/.claude-plugin/plugin.json`,
  `plugins/sluice-board/.claude-plugin/plugin.json` and
  `plugins/herdr/herdr-plugin.toml`, and no file changes

### Requirement: A build takes sluice's version from its tree alone

`sluice --version` SHALL print `sluice <version>`, where `<version>` is
the workspace's version as the tree it was built from gives it. The
flake's package SHALL take as its version `Cargo.toml`'s
`[workspace.package]` version, read from the tree. The flake SHALL read
no variable of the builder's environment for it, and SHALL hand the
build no version. A build of main, or of any commit that no release's
tag names, SHALL report `0.0.0`.

#### Scenario: The binary's version

- **WHEN** `sluice --version` is parsed by the command line built from
  this tree
- **THEN** it prints `sluice ` and the version `Cargo.toml`'s
  `[workspace.package]` gives ("the_version_is_the_workspaces" in
  `crates/sluice/src/lib.rs`)

#### Scenario: The flake's package, whatever the builder's environment

- **WHEN** the flake's outputs are evaluated with `SLUICE_VERSION` set
  to `98.76.54` in the evaluation's environment
- **THEN** each system's `packages.<system>.default.version` is
  `Cargo.toml`'s `[workspace.package]` version
- **AND** no system's package hands its build a `SLUICE_VERSION` ("The
  package's version is the tree's" in `nix/tests/version.nix`)

### Requirement: A release's tag names a release commit cut from the pushed commit

CI's release job SHALL run `set-version.sh` with the release's version
on the pushed commit, and commit the files `set-version.sh --files`
lists, by path, as `chore(release): v<x.y.z>`, so the pushed commit is
the release commit's one parent. The tag `v<x.y.z>` SHALL name that
commit.

#### Scenario: The release commit

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `release` job runs `sh set-version.sh "${TAG#v}"`
- **AND** it commits the paths `sh set-version.sh --files` prints as
  `chore(release): $TAG`, and runs `git tag "$TAG"`

### Requirement: Nothing of a release reaches main, and no tag moves

The release job SHALL push the tag alone, with no branch and no force,
and SHALL publish the GitHub release at that tag, naming no other
commit for it. Nothing SHALL be committed to main, and no tag SHALL be
moved.

#### Scenario: The tag and the release

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `release` job's one push is `refs/tags/$TAG`, with no
  `+`, no `--force` and no branch
- **AND** it runs `gh release create "$TAG" --verify-tag`, with no
  `--target`

### Requirement: A release's binaries are built from the tree its tag names, with nothing given

CI's binaries job SHALL build each platform's binaries from the tree the
release's tag names, the pushed commit with `set-version.sh`'s writes,
by `cargo build --release --locked` for the platform's target under the
toolchain `rust-toolchain.toml` names: Linux's against musl. It SHALL
install no Nix, and SHALL fail unless the built `sluice --version`
prints `sluice <x.y.z>` there. No job SHALL name `SLUICE_VERSION` or
`--impure` (record 0111 as amended 2026-11-18).

#### Scenario: The binaries job

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `binaries` job runs `sh set-version.sh "${TAG#v}"` ahead
  of `cargo build --release --locked -p sluice --target "$TARGET"`
- **AND** it fails where `target/$TARGET/release/sluice --version` is
  not `sluice ${TAG#v}`
- **AND** the workflow names neither `SLUICE_VERSION` nor `--impure`

#### Scenario: A binary runs on a runner with no Nix

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `binaries` job neither installs Nix nor runs `nix`, so
  its run of `sluice --version` is on a runner with nothing in
  `/nix/store`
- **AND** each Linux target it names is `-linux-musl`, so Linux's
  binaries are static and load nothing of the runner's

### Requirement: CI reckons the next release from the newest tag by version

A release's tag names a commit off main's history. So CI's version job
SHALL reckon the next version with git-cliff's `--unreleased` and
`--bumped-version`, whose range starts at the newest tag. Its
conventional check SHALL check the commits since the newest `v*` tag
by version, not since the newest tag main's history holds.

#### Scenario: The version and conventional jobs

- **WHEN** `root:release` reads `.github/workflows/ci.yml`
- **THEN** its `version` job runs `git-cliff --unreleased
  --bumped-version`
- **AND** its `conventional` job's range starts at the first of
  `git tag --list 'v*' --sort=-v:refname`, and it runs no
  `git describe`
