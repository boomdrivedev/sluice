# Tasks

Work in the place on `chore/sluice-raises-its-open-files-limit-at-start`,
cut from `bolt/sluice-runs-under-any-hosts-file-limit`, the branch it
lands on and so the base every `sluice exec` below names. Commit by
pathspec, with Conventional Commits scoped to the part of the tree,
citing clauses by number and rules by id.

## 1. sluice raises its limit

- [x] 1.1 Add `rlimit = "0.11"` to the workspace's dependencies and to `crates/sluice`'s. In `crates/sluice/src/main.rs`, add `raise_open_files()`, which calls `rlimit::increase_nofile_limit(u64::MAX)` and, on an error, says `sluice: warning: the limit on open files stays as sluice was started with: <the error>` on standard error (design D1, D2, D4). Call it in `main` right after `HandOver::take`. In `crates/sluice/tests/exec.rs`, add `a_command_runs_under_the_raised_limit_on_open_files`, which starts sluice under a soft limit of 256 and reads the limit its command runs under, as the open-files-limit spec's first scenario says. True when `sluice exec bolt/sluice-runs-under-any-hosts-file-limit -- cargo test -p sluice --features tools --test exec a_command_runs_under_the_raised_limit_on_open_files` passes.

## 2. The decision is written down

- [x] 2.1 Add record 0126 (`docs/adr/0126-every-sluice-process-raises-its-soft-limit-on-open-files-to-its-hard-limit-as-it-starts.md`) with the paths not taken. True when `sluice exec bolt/sluice-runs-under-any-hosts-file-limit -- moon run root:adrs` passes.
- [x] 2.2 Add rule core.59 and its details to `docs/architecture/core.md`, citing 135 as amended at boomdrivedev/blueprints 2a045b2 and record 0126, and write the site's rules page with `node site/scripts/rules.mjs`. True when `sluice exec bolt/sluice-runs-under-any-hosts-file-limit -- moon run site:rules` passes.

## 3. The specs archive clean

- [x] 3.1 Validate the change, and archive it into `openspec/specs/open-files-limit/spec.md`. True when, under the openspec `devenv.nix` pins, `openspec validate sluice-raises-its-open-files-limit-at-start --strict --no-interactive` passes before the archive, and `sluice exec bolt/sluice-runs-under-any-hosts-file-limit -- moon run root:specs` passes after it.
