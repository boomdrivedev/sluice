## Why

sluice v0.97.1's release binaries do not start on GitHub's hosted
runners. CI's binaries job builds them by `nix build .#default`, so each
names what it loads by its path in the Nix store: Linux's interpreter is
`/nix/store/…-glibc-2.44-25/lib/ld-linux-x86-64.so.2`, and macOS's
`libiconv.2.dylib` is `/nix/store/…-libiconv-115.100.1/lib/`. A runner
with no Nix, or a Nix store that lacks those paths, holds neither.

On 2026-11-18, in the trial boomdrivedev/sluice-actions-trial, the
Actions pool's run job installed the release and `sluice job run`
failed before sluice ran: on ubuntu-24.04,
`/home/runner/work/_temp/bin/sluice: cannot execute: required file not
found`, exit 127; on macos-15, `dyld[1735]: Library not loaded:
/nix/store/fgrxz4hr8f71hkhsddbvk8nxadpg67bv-libiconv-115.100.1/lib/libiconv.2.dylib`,
exit 134. Every sign job failed the same way at `sluice job sign`.
CodeBuild's run and sign builds install the same Linux binary.

Flume's release already ships binaries that run on a host with no Nix
and no Rust. Record 0111 as amended 2026-11-18 takes its form.

## What Changes

- **The binaries job builds by cargo, as Flume's does.** Each platform's
  binaries are `cargo build --release --locked -p sluice` for the
  platform's target, by the toolchain `rust-toolchain.toml` names, which
  rustup installs: Linux's static against musl
  (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`), macOS's
  linking only the system's libraries (`aarch64-apple-darwin`). The
  tree is the one the release's tag names, as before.
- **The job installs no Nix, so its own run of the binary is the
  proof.** It fails unless `sluice --version` says the release's
  version on that runner, which holds nothing of Nix's.
- **`root:release` holds the job to it.** `check-release.sh` reads the
  binaries job's cargo build for its target, its run of the built
  `sluice --version`, no Nix installed or run in it, and no Linux
  target but musl's.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `release-version`: a release's binaries are built by cargo from the
  tree its tag names and run on a host with no Nix.

## Out of scope

- **The flake's package.** It stays the Nix build a Nix host installs,
  linked against its store, as before; CI's `build` job still builds it.
- **The pool's workflows and CodeBuild's builds.** They install the
  release's tarballs by the names they have; only what the tarballs
  hold changes.
- **timber's binaries job,** which builds by `nix build` as sluice's
  did; following Flume's form there is timber's.

## Impact

- `.github/workflows/ci.yml`: the `binaries` job.
- `check-release.sh` and `moon.yml`'s `root:release` comment.
- `flake.nix`: its header no longer names the release's build.
- Records and rules: record 0111 as amended 2026-11-18, rule core.55.
- Sources: Flume's `.github/workflows/ci.yml` `binaries` job; the trial
  boomdrivedev/sluice-actions-trial, 2026-11-18.

## Touches

```
.github/workflows/ci.yml
check-release.sh
moon.yml
flake.nix
docs/adr/0111-a-sluice-releases-tag-names-a-release-commit-that-writes-its-version-into-the-tree.md
docs/architecture/core.md
openspec/changes/release-binaries-run-on-a-plain-runner/**
```
