## Why

Record 0119, accepted by the user on 2026-11-12, sends a queue's runs to
a pool and names two: GitHub Actions, which the change
`a-runs-life-is-written-first-and-actions-runs-its-checks` built, and a
CodeBuild reserved-capacity fleet, which this change builds. The user
asked on 2026-11-14 for the fleet to run a queue's gate checks rather
than a disk of the gate box's own ("I think I'd rather do the sluice
refactor and set up code build", signal
`2026-11-14-afterthought-planner-ab274a85/01`). Statement in
boomdrivedev/blueprints, `design/sluice/requirements.md`: 71, 77, 78,
79, 87, 88, 107, 109, 111, 141, C.3's launched row, C.6, C.7, C.8; rules
core.10, core.22, core.29, core.62.

What sluice holds at 44e33d4e is the Actions pool and nothing else:

- **A pool is GitHub's.** `[pools.<name>]` takes any kind, but the policy
  refuses a pool whose signer is not of its own kind, and the binding
  hands the check profile one launcher, `github::actions::Actions`, only
  where `[forge]` names the app (`crates/sluice/src/profiles/mod.rs`).
- **A signer is an SSH key or a workflow.** `SignerKey` is `Ssh` or
  `Keyless` (`crates/sluice/src/profiles/remote_refs.rs`); nothing reads
  a key held in a key service, and nothing signs with one.
- **A pool's job leaves its stack.** `sluice job run` starts the claimed
  run and returns once its report is kept, never releasing its checkout
  (`crates/sluice/src/job.rs`), since a GitHub runner is thrown away
  after its job. A fleet's instance runs the next build on the same
  machine and engine.
- **Nothing reaches AWS.** sluice holds no AWS client, no way to find
  AWS credentials, and no setting naming them.
- **The guides know one pool.** `site/src/content/docs/guides/github-actions.md`
  takes a queue's runs onto GitHub Actions; no guide says how a branch
  on a fleet should test.

## What Changes

- **A branch whose policy names a CodeBuild pool runs on its fleet.**
  `[pools.<name>] codebuild = { region, sign }` names the region and the
  sign project, `runners` the run project for each system, each on a
  reserved-capacity fleet of that system, and `signer` a `kms` signer. A
  gate that trusts only that signer has every run of the branch go to
  the pool (107), one build per run, as many at once as the fleet's
  instances, holding no part of any host's bound (71).
- **One run is two builds.** The lead starts the run build on the
  system's run project, writing the claim first and passing only the
  claim, the branch, the combination, the system and a token that reads
  the repository for an hour. The run build holds no key: it runs
  `sluice job run` and leaves its report as an artifact. Its end starts
  the sign build, through an EventBridge rule, which runs none of the
  combination's code: `sluice job sign --build` reads the run build
  from CodeBuild, keeps its hand-over only where its digest is the one
  CodeBuild recorded, signs the keys the combination's tree bears out
  with the KMS key, and `sluice job record --build` writes the run's end
  under its claim.
- **A `kms` signer.** `[signers.<name>] kms = { key, public }` names a
  P-256 key in AWS KMS by its key ARN and lists its public half. Its
  records are DSSE envelopes signed as timber's key-service signer signs
  (timber C.7, timber ADR 0017), and every host verifies them with the
  listed half, calling no AWS service (78).
- **Each build releases its stack.** `sluice job run` releases its
  checkout before it exits, whatever its run came to, on every pool. A
  run build starts by ending whatever an earlier build on its instance
  left running, since an instance runs one build at a time.
- **sluice reaches AWS through the AWS SDK's own chain**, as timber's
  KMS signer does, with the host's `[aws] profile` naming the profile a
  lead uses, under sluice's one retry policy.
- **`sluice init codebuild`** writes a CloudFormation template of the
  fleets, projects, key, bucket, rule and roles, and, given the stack it
  was deployed as, appends `[signers.codebuild]` and `[pools.codebuild]`.
  **`sluice pool check <branch>`** reads a CodeBuild pool back: its
  projects, fleets, buildspecs, timeout and key, and whether the branch
  tests side by side at the fleet's size.
- **One guide takes a queue's runs onto a pool**, GitHub Actions or a
  CodeBuild fleet, from the policy to the first run, and says a branch
  on a fleet tests side by side, `batch = false`, with `parallel` at the
  fleet's maximum.

## Capabilities

### New Capabilities

- `codebuild-pool`: a pool on a CodeBuild fleet: the policy's table, the
  run build and what the lead may pass it, the sign build its end
  starts, the hand-over bound to its build by CodeBuild's digest, where a
  pool's job stands, the host's AWS profile, `sluice init codebuild` and
  `sluice pool check` (record 0119; 71, 77, 88, 107, 111, C.3, C.8).

### Modified Capabilities

- `run-signing`: a `kms` signer among the identities; its records are
  DSSE envelopes signed by its key in a key service; every host verifies
  one from the policy's public half alone; a pool's jobs may sign as a
  `kms` signer (78, 79, 87, 141, C.6).
- `run-claims`: the policy's pools take a `codebuild` table beside
  `github`, each kind signing as its own kind of signer (88, 107).
- `place-release`: a pool's job releases its checkout before it ends,
  and a reused machine's next job starts with nothing of an earlier
  job's running (core.10; record 0016).

## Impact

- **Code.** `crates/sluice-core/src/policy.rs` (`kms` signers,
  `codebuild` pools, the pairing of a pool's kind and its signer's).
  `crates/sluice`: a new profile `profiles/codebuild/` (the AWS calls it
  makes behind one trait, their SDK implementation and a fake, the
  launcher, where a job stands, the templates); `signing/kms.rs`;
  `profiles/remote_refs.rs` (`SignerKey::Kms`); `profiles/checks.rs` and
  `profiles/mod.rs` (a launcher per pool kind); `profiles/github/app.rs`
  (a token limited to reading one repository); `instance.rs` (`[aws]
  profile`); `job.rs` (the release, `--build`); `init.rs` (`sluice init
  codebuild`); a new `pool.rs` (`sluice pool check`); `lib.rs` (the
  commands).
- **Dependencies.** `aws-config`, `aws-sdk-codebuild`, `aws-sdk-kms`,
  `aws-sdk-s3` and `tokio`'s current-thread runtime, as timber pins
  them; `p256` for verifying; `zip` for the hand-over.
- **Tests.** Unit tests in the new profile against its fake; the
  template's own; `crates/sluice/tests/cli.rs` for `sluice job`, `sluice
  init codebuild` and `sluice pool check`;
  `crates/sluice/tests/remote_refs.rs` for a `kms` record;
  `crates/sluice-conformance/tests/run_life.rs` for a gate on a pool.
- **Gate.** cfn-lint, pinned in the devenv, lints the CloudFormation
  template under `root:format`, beside yamlfmt (core.21, core.24).
- **Docs.** `site/src/content/docs/guides/pools.md` replaces
  `github-actions.md`, with a redirect; the parallel and batch, executors
  and systems, and pull requests guides; the configuration and commands
  references; the roadmap; the setting-up skill; the README. Record 0119
  gains the build's amendment; rule core.63 joins `docs/architecture/core.md`,
  and core.22's and core.62's details name the CodeBuild pool and the
  `kms` signer.
- **Statement.** C.6 says a signer that is not a forge's job signs "with
  an SSH key whose public half the policy lists". A `kms` signer signs
  with a key in a key service, as record 0119 decided, so C.6 is
  amended to name it, as timber's C.7 does, before this change lands
  (core.12; design D7).
- **Family.** timber's key-service signer and sluice's `kms` signer are
  one design, timber's C.7 its owner (design D7, Looking across the
  family).

## Out of scope

- **A pull request's head run on the fleet**, the CodeBuild analogue of
  `sluice-pr.yml`: a change of its own, once a fleet runs a queue's
  combinations.
- **A round run as a build**, a branch led with every host off, and a
  wake that starts a build: `a-github-action-runs-a-queue-whole` designs
  the round job for a pool; until it or a change like it lands, a branch
  on the fleet lands only while a host that leads it is up (111).
- **A standing host signing with a KMS key**, `[signing] kms`: timber
  has one; sluice's hosts sign with SSH keys as before.
- **A macOS fleet.** CodeBuild runs macOS on reserved capacity with a
  day's minimum per instance; the template writes Linux fleets, and a
  policy may name a project on any fleet by hand.
- **A run's signed record of in-toto's Test Result kind** (record 0077;
  core.40): the sign build signs the statement sluice signs today.
