# Tasks

Work in this change's place on `change/sluices-actions-pool-is-proven-live`,
cut from `bolt/sluices-pools-run-a-repositorys-own-steps`, which is its base
and its gate (S66). Read the proposal, the design and the delta specs first,
then record 0119 with its amendments of 2026-11-16 and
`docs/architecture/core.md`. Task 1.1 waits on ops's trial (D1): every task
after it reads what the trial made. Each task leaves the tree building and its
named check passing. A unit test runs directly with `cargo test`; a project's
task runs through sluice against the base, as CLAUDE.md's "Running checks" has
it: `sluice exec bolt/sluices-pools-run-a-repositorys-own-steps -- moon run
…`. No test reaches GitHub: the tests read the kept files alone. Commit by
pathspec, Conventional Commits scoped to the part of the tree (`records`,
`devenv`, `docs`), citing the clauses, record 0119, rules by id and the
design's decisions by D-number.

## 1. The trial

- [ ] 1.1 The GitHub Actions pool's trial has run as D2 has it, on a released sluice: a public scratch repository whose `main` names the pool for `linux-x64` on `ubuntu-24.04` and `macos-arm64` on `macos-15`, one submission landed on it from a signed run on each, each bundle added to `refs/sluice/runs/forge` (78, 88, 107; record 0119; D1, D2). ops runs it by hand. Where this stage's prompt names no trial's repository, stop short with `flume needs --to ops`, saying that this change's code waits on the trial D2 describes, that the user is asked on one card for sluice's app's Actions permission and for the public repository's owner and name, and that a defect the trial meets goes to `main` as a fix and the trial runs again on the release that carries it (D6); go on once ops answers with the repository, its release and what the trial found. The gate cannot run this check, since it reaches GitHub: true when `git ls-remote https://github.com/<owner>/<name> refs/heads/main refs/sluice/runs/forge` prints both refs.
- [ ] 1.2 `crates/sluice/tests/trials/actions/` keeps the trial (D3): `linux-x64.sigstore.json` and `macos-arm64.sigstore.json`, the two records the trial's sign jobs added under `refs/sluice/runs/forge` at `<combination>/<system>/<digest>.json`, written with `git cat-file blob` from a scratch clone outside the tree so their bytes are the records'; `policy.toml`, the trial's `.config/sluice.toml` at the revision its gate was read at; and `trial.toml`, a comment naming record 0119 and this change, then `repository = "<owner>/<name>"`, `release`, `revision`, and a `[[runs]]` table for each run naming its `system`, `runner` and `record` file. `treefmt.nix` excludes `crates/sluice/tests/trials/**/*.sigstore.json` beside the recorded bundles' exclude, its comment saying they are kept as the pool's sign jobs wrote them (core.21). True when `sluice exec bolt/sluices-pools-run-a-repositorys-own-steps -- moon run root:format` passes.

## 2. Every gate reads the trial

- [ ] 2.1 `the_trials_runs_count_for_its_gate`, a unit test in `crates/sluice/src/profiles/remote_refs.rs`'s tests: the kept policy parsed by `sluice_core::policy::Policy::parse`, `trial.toml` read with `toml`, each record by `include_str!`; the identity built by `keyless::Identity::workflow` from `trial.toml`'s repository and the policy's `[signers.forge] github` workflow and ref; each record read through `verify("forge", &SignerKey::Keyless(Some(identity)), …)` gives, for every check the policy's `main` requires, a line of kind ran, outcome pass, on that record's own system, `linux-x64` from the one and `macos-arm64` from the other, its signer `forge`, its host the claim `pool/forge/<n>` the statement names, its branch `main`, the two records' lines of one combination; and the policy's `main` rules count each line, `trusts_signed("forge", <its confinement>, <its system>)` true (78, 86, 88; record 0119; D4). True when `cargo test -p sluice --lib profiles::remote_refs::tests::the_trials_runs_count_for_its_gate` passes.
- [ ] 2.2 `the_trials_runs_count_only_from_its_signers_workflow`, a unit test beside it: each kept record read with the trial's workflow on `refs/pull/1/merge`, and with the trial's workflow on `main` in another repository, is refused, the error naming the identity the record's certificate carried, `https://github.com/<owner>/<name>/.github/workflows/sluice.yml@refs/heads/main` (78, 79; rule core.22; D4). True when `cargo test -p sluice --lib profiles::remote_refs::tests::the_trials_runs_count_only_from_its_signers_workflow` passes.

## 3. The record

- [ ] 3.1 Record 0119 gains an amendment, "the GitHub Actions pool's trial", dated when this lands: the trial's repository, release and runners; each run's time from its claim to its close; what `sluice forge check main` read; each run job's token as GitHub gave it; what the trial found and where each finding went (D6); what is kept and the two tests that read it (D3, D4); and that the kept trial is never edited to make a test pass, a change to what a pool signs reading the records signed before it and keeping its own trial beside (D5); each with the path not taken, and that no clause and no rule changes (D7; core.12). No gate task reads a record (core.21; record 0127), so this check is by hand: true when the amendment, read through once, names each item this task lists.
