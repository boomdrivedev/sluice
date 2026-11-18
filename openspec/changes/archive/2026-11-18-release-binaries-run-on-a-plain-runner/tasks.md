# Tasks

Work in this change's place on its branch, cut from main. Run suites
through sluice (`sluice exec main`); a quick check may run directly.
Commit by pathspec, Conventional Commits, scoped to the part of the
tree, citing records and rules by id.

## 1. The decision recorded

- [x] 1.1 Amend record 0111 with the binaries' build, Flume's form, and add the case to rule core.55 and its details, as the proposal's What Changes says. True when `openspec validate --strict` passes on the specs and this change.

## 2. The release's binaries run on a host with no Nix

- [x] 2.1 In `.github/workflows/ci.yml`, build the `binaries` job's binaries by cargo for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin` under the toolchain rustup installs from `rust-toolchain.toml`, with no Nix in the job, and fail unless the built `sluice --version` says the release's version. True when `sluice exec main -- moon run root:format` passes, which runs actionlint.
- [x] 2.2 In `check-release.sh`, hold the binaries job to its cargo build for its target, its run of the built `sluice --version`, no Nix installed or run in it and no Linux target but musl's; say so in `moon.yml`'s comment on `root:release`. True when `sluice exec main -- moon run root:release` passes, and fails on a copy of the workflow whose binaries job installs Nix or names `x86_64-unknown-linux-gnu`.
- [x] 2.3 Build `aarch64-unknown-linux-musl` from this tree in a fresh `ubuntu:24.04` as the job does, and run the binary in containers with nothing installed. True when it prints `sluice 98.76.54` there; the gate cannot run it, since it needs a Linux host and the network, and the binaries job proves it at each release.
