## MODIFIED Requirements

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
