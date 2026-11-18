# Design

## Context

Record 0119, accepted by the user on 2026-11-12, decided the pool and
left its build to a change: every run of a queue on a pool goes to the
pool; a CodeBuild reserved-capacity fleet runs as many at once as it
holds; the lead starts one build per run, its start the claim and its
end the report (88); no candidate's code and no session reaches the
signing key, which is a key in AWS KMS only the signing step's role may
use, listed in the policy by its public half (`[signers.<name>] kms`);
a warm instance keeps the image layers, the toolchain's shell and the
task runner's cache; a build leaves its instance clean; a branch on a
fleet tests side by side with `parallel` at the fleet's maximum. Its
amendment of 2026-11-16 records how the GitHub Actions pool was built:
one workflow run of a run job and a sign job, `sluice job run`, `sign`
and `record`, the pool's seam `Tests::job`, and record 0122's run life,
which a pool's run follows as any run does (rules core.61, core.62).

The statement holds as written for everything but C.6 (D7): 71 keeps a
pool's runs out of a host's bound; 88 makes a launched job's start its
claim and its end its report; 107 sends to a pool only what no live
standing executor may take, so a gate that trusts only the pool's
signer runs everything there; 111 lets a wake be lost; C.3's launched
row is the profile this change adds a second of.

What the code does at 44e33d4e:

- **Pools.** `Policy::pools` takes any kind and refuses a pool whose
  signer is not of its own kind (`crates/sluice-core/src/policy.rs`).
  `Checks` holds one `Launcher`, bound to `github::actions::Actions`
  where the host has the app (`crates/sluice/src/profiles/mod.rs`);
  `Checks::job` reads a job as `Queued` where no launcher is bound.
- **Signers.** `SignerKey` is `Ssh` (allowed-signers lines, SSHSIG over
  the envelope's pre-authentication encoding) or `Keyless` (a Sigstore
  bundle verified against carried roots)
  (`crates/sluice/src/profiles/remote_refs.rs`).
- **Jobs.** `sluice job run` starts the claimed run through
  `Sluice::start_claimed`, keeps its report and its proof in `--out` and
  returns; nothing releases the checkout it made. `sluice job sign`
  reads the run job's directory, keeps the keys `proven_from` bears out
  and writes subjects for `actions/attest`; `sluice job record` adds a
  bundle read by `keyless::statement_of` (`crates/sluice/src/job.rs`).
- **AWS.** None. HTTP calls are blocking, over `ureq`, under `backon`'s
  policy (ADR 0062) and weir-bounded's try limits (record 0109).

## Goals / Non-Goals

**Goals:**

- A branch on a CodeBuild pool runs every run as one build on its
  system's fleet, as many at once as the fleet holds, nothing of any
  host's bound held.
- The build that runs a combination's checks holds no key, and the step
  that signs runs none of the combination's code.
- A `kms` signer's records verify on every host from the policy alone.
- No build leaves anything running on the instance the next build uses.
- One guide from the policy to the first run, on either pool.

**Non-Goals:**

- A pull request's head run on the fleet; a round run as a build; a
  standing host signing with KMS; a macOS fleet; in-toto's Test Result
  kind (proposal, Out of scope).

## Decisions

### D1. The policy names a CodeBuild pool and a `kms` signer

```toml
[signers.lead]
ssh = ".config/sluice.leads"
systems = []

[signers.codebuild]
kms = { key = "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab", public = "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE…" }
systems = ["linux-x64"]

[pools.codebuild]
codebuild = { region = "us-east-1", sign = "sluice-sign" }
signer = "codebuild"
runners = { linux-x64 = "sluice-run-linux-x64" }
```

- `kms.key` is the key's ARN, never an alias's or a bare id, which
  timber's key-service signer takes as the signer's one name (timber ADR
  0017); `kms.public` is the key's public half as KMS's `GetPublicKey`
  answers it, the base64 of its DER SubjectPublicKeyInfo, a P-256 key.
  A `kms` signer missing either, naming an alias, or listing a key of
  another curve, is refused naming `signers.<name>` and the field.
- `codebuild.region` is where the projects are; `codebuild.sign` names
  the sign project. `runners` names, for each system, the run project,
  which names its fleet.
- A pool's kind decides its signer's: a `github` pool signs as a
  `github` signer, a `codebuild` pool as a `kms` signer. The policy's
  rule that a pool signs as a signer of its own kind becomes that
  pairing; any other pairing is refused naming both.

A branch on the fleet trusts `codebuild` and `lead`, the signer its
leading hosts sign as, which may claim no system: `trust = ["lead",
"codebuild"]`. A branch whose trust names only pools' signers has no
host that can lead it, since a host leads a branch held on its remote
only signing as a signer its gate trusts (rule core.60, clause 137;
record 0093 as amended), and forms nothing. `lead`'s signature is
accepted, so its hosts lead; its `systems = []` claims no run and counts
for no result, whatever the run's system, one naming none included, so
every run goes to the pool (107) and the pool's results are the only
ones that land anything, whatever executors the leading hosts have. The
guide says to set `batch = false` and `parallel` to the fleet's maximum
(D10, D11).

*Not taken:* a host the policy names leading the branch untrusted,
which loosens record 0093 as amended ("A machine whose signature the
branch does not accept leads none of its rounds and runs none of its
checks") where the empty list reaches the same within it; trusting the
leading hosts' signer for its systems, so a host that gained an executor
would land code on its own results.

*Not taken:* the fleet's ARN in `runners`, with the lead passing it as
`fleetOverride` to one project for every system. A project per system
names its fleet itself, so the lead overrides nothing that chooses a
machine (D3).

### D2. One run, two builds

| | run build | sign build |
|---|---|---|
| project | the system's run project, on its reserved-capacity fleet, instance mode (`LINUX_EC2`, `ARM_EC2`) | the sign project, on demand, a small Linux container |
| started by | the lead, after writing the run `claimed` (D3) | an EventBridge rule on a run project's build reaching `SUCCEEDED`, `FAILED`, `FAULT` or `TIMED_OUT`, passing the run build's ARN as `SLUICE_RUN_BUILD` |
| buildspec | inline in the project, as `sluice init codebuild` writes it; no source | inline, likewise; no source |
| role may | write its artifact to the hand-over bucket and its logs | `BatchGetBuilds` on the run projects, read the hand-over bucket, `kms:Sign` with the key (`ECDSA_SHA_256`, `DIGEST`), read the app's key secret, write its logs |
| runs | ends what an earlier build left (D8), installs sluice and Nix where the instance holds none, clones with the read token (D4), `sluice job run --claim --branch --out`; its artifact, `ZIP`, is the out directory | `sluice job sign --build "$SLUICE_RUN_BUILD" --out sign`, then `sluice job record --build "$SLUICE_RUN_BUILD" --from sign`, with `SLUICE_APP_ID` and `SLUICE_APP_KEY` |

A run build CodeBuild stopped starts no sign build, as a cancelled
workflow run's sign job is skipped: the lead reads it as a job that
ended with no record (D5). The run build's `UPLOAD_ARTIFACTS` is tried
whatever its build phase came to, so a run that kept no report hands
over why it kept none, and the sign build records the run ended with no
outcome, as the Actions sign job does.

*Not taken:* one build with the sign as its last phase. Every process in
a build reaches its role's credentials from the container endpoint, so
any check could sign. The lead starting the sign build: the lead's
identity would choose what is signed, and the run's end would wait for a
lead's round. CodeBuild's batch builds: every build of a batch runs
under one role.

### D3. The lead starts a run build and changes nothing it runs

`decide` writes the run `claimed` by `pool/<pool>/<n>`, as for any pool;
`perform` asks `Tests::job` and starts a build only where it answers
`None` (record 0122; D3 and D12 of the Actions change). The start is
`StartBuild` on the project `runners` names for the run's system, with
`environmentVariablesOverride` of `SLUICE_CLAIM`, `SLUICE_BRANCH`,
`SLUICE_COMBINATION`, `SLUICE_SYSTEM` and `SLUICE_READ_TOKEN` (D4), and
`idempotencyToken` the claim, and nothing else. A start whose answer was
not read is settled by reading (core.29): the next round's `Tests::job`
finds the build that bears the claim or none; a start sent again within
five minutes under the same token starts no second build.

The lead role the template writes may `StartBuild` the run projects
only, and is denied it wherever the request carries any override
CodeBuild gives a condition key for, by `Null` on
`codebuild:source.buildspec`, `codebuild:environment.image`,
`codebuild:serviceRole`, `codebuild:environment.privilegedMode`,
`codebuild:environment.fleet.fleetArn`, `codebuild:environment.computeType`,
`codebuild:environment.type`, `codebuild:environment.imagePullCredentialsType`,
`codebuild:source.location` and the `artifacts`, `cache`, `logsConfig`
and `secondarySources` keys, and wherever
`codebuild:environment.environmentVariables.name` names a variable but
those five. An override with no key changes nothing a run build reads:
it has no source to version, and a timeout only bounds it. The role may
also `BatchGetBuilds` and `ListBuildsForProject` on the run and sign
projects, and nothing else: no key, no secret, no sign project's start
(record 0119, C.8).

### D4. The run build reads the repository with a token of an hour

The lead mints, for each start, an installation token of sluice's app
limited to `contents: read` on the branch's repository (`App::reading`
in `profiles/github/app.rs`), and passes it as `SLUICE_READ_TOKEN`. The
run build clones and fetches with it, and `sluice job run` reads the
branch's records and fetches the combination by its own ref (82;
record 0118) with nothing more. The token lapses within GitHub's hour;
a build queued past it fails to fetch, keeps no report saying so, and
the run ends with no outcome, its follow-up started afresh (record
0122). A lead with no app starts nothing, its launch failing naming
`[forge]`.

*Not taken:* a deploy key in Secrets Manager the run role reads, which
every check reaches for good; the project's own source credential
through CodeBuild's `git-credential-helper`, whose reach is the
connection's app's, not read-only.

### D5. Where a pool's job stands

`Tests::job(kind, claim, remote)` for a `codebuild` kind reads the run
build that bears the claim in `SLUICE_CLAIM`, and the sign build that
bears that run build in `SLUICE_RUN_BUILD`:

| CodeBuild says | `Job` |
|---|---|
| no run build bears the claim | `None` |
| the run build is `SUBMITTED` or `QUEUED` | `Queued` |
| the run build has left `QUEUED`, and no sign build of it has completed | `Running { since }`, since the `QUEUED` phase's end |
| the run build ended `STOPPED`, or ended and no sign build bears it within `[peers] alive` of its end | `Ended { recorded: false }` |
| the sign build completed | `Ended { recorded }`, `recorded` where it `SUCCEEDED` |

The lead's lapse of a job ended with no record says which: a build that
was stopped, or a run build no sign build took, which names the rule
the template writes (record 0122's D5 and D12 of the Actions change).

The host keeps each claim's run build and sign build ids, the start's
answer and a list's find, in its own state beside the forge's kept
answers, deciding nothing (core.32's way); where it keeps none, as on a
lead that did not start the build, it lists the project's builds newest
first and reads them in one `BatchGetBuilds`. A read AWS refuses is
logged on the branch once per cause and host with AWS's words, and the
run is read as it stood (core.28).

`Checks` holds a launcher per pool kind the host can reach: GitHub
Actions where `[forge]` names the app, CodeBuild always, its credentials
found at each call (D9), and `launch`, `job` and `wake` go to the
launcher of the pool's kind. `wake` starts nothing for CodeBuild: no
build runs a round (111).

### D6. The hand-over is CodeBuild's word

The run projects' builds share one role, so a check could write another
build's object in the hand-over bucket. CodeBuild records a `ZIP`
artifact's SHA-256 as it uploads it, and `BatchGetBuilds` answers it.
`sluice job sign --build <arn>` reads the run build: of a project the
branch's pool names for a system, completed, its `SLUICE_CLAIM` and
`SLUICE_BRANCH`; downloads its artifact from where CodeBuild says it put
it; and keeps it only where its SHA-256 is the one CodeBuild recorded.
From there it signs as the Actions sign job does: the run under the
claim read from the branch's records, never the build's word, the
combination fetched by its own ref, and only the keys its tree bears out
by the proof the run build kept and the runner the policy names (78,
109; core.62). A hand-over whose digest differs is one with no report,
and the run ends with no outcome, saying the run build's hand-over is
not what CodeBuild recorded.

`sluice job record --build <arn> --from sign` reads the claim and the
branch from the run build again, adds the signed envelope `sign` holds,
where it holds one, among the pool's signer's runs, and writes the run
`ended` and `closed` under its claim in one write, refusing with nothing
written where the claim no longer holds the run, as `--claim` does.

### D7. A `kms` signer signs as timber's key-service signer does

The family has one design for a signer whose key is in a key service:
timber's C.7 and its record 0017. sluice's `kms` signer is that design:

- an `ECC_NIST_P256` key with the usage `SIGN_VERIFY`, made in KMS;
- `ECDSA_SHA_256` over the SHA-256 of the DSSE envelope's
  pre-authentication encoding, sent as `MessageType: DIGEST`, the
  signature KMS's DER bytes as answered, the envelope's `keyid` the
  key's ARN;
- verified on the host with the public half the policy lists at the
  branch's revision, the signature's S normalized low first.

`crates/sluice/src/signing/kms.rs` signs through the AWS calls of D9 and
verifies with `p256`. `SignerKey` gains `Kms { key, public }`, read from
`[signers.<name>] kms`; `verify` checks a record's envelope by it, a
record whose `keyid` is not the listed ARN or whose signature the half
does not verify counting for nothing, with why in the reading process's
own warning, as a keyless record that does not verify (D13 of the
Actions change). A `kms` signer's one listed key is its key in KMS, so
`SignerKey::lists` lists no host's SSH key for it: a host signing as one
is not accepted, leads nothing and claims nothing (rule core.60). The
statement is sluice's run statement, as every signer signs today.

sluice never asks KMS for a public half: 78 reads a run from the records
alone, so a `kms` signer with no `public` is refused (D1), where timber's
C.7 asks the key service only where its policy carries none.

C.6 says each signer that is no forge's job signs "with an SSH key whose
public half the policy lists", which no longer holds once a key in a key
service signs. It is amended to say such a signer signs with an SSH key
or a key in a key service its holder may use and never export, timber's
C.7, either listed in the policy by its public half. The amendment is
the statement's, in boomdrivedev/blueprints, made before this change
lands (core.12); this change cites it.

*Not taken:* SSHSIG made by a KMS Ed25519 key, which KMS signs since
2025-11 and sluice's SSH verifier would read unchanged, and which C.6
holds as written. It is a second form beside timber's for one kind of
signer, and fewer tools read it than cosign's and Sigstore's P-256
envelopes (141).

### D8. Each build releases its stack

`sluice job run` releases the checkout it made before it exits, whatever
its run came to, and when it is stopped, by the release the policy names
in the checkout, freeing its ports and scratch (record 0016; 123; rule
core.10). It does so on every pool: a thrown-away runner loses nothing by
it. A release that fails is said in its output and its hand-over, and
holds nothing of the run's outcome.

CodeBuild does not say whether a build it stops or times out lets its
processes finish, so a build may end before its release. An instance
runs one build at a time, so the run buildspec's first step ends every
container the instance's engine runs and removes the host user's sluice
scratch directory and the state an earlier build kept: nothing of an
earlier build runs beside the next, whatever ended it. Images stay, as
the warm instance's caches do (record 0119).

*Not taken:* reading a stopped build's places from a clone kept between
builds and releasing each by its own command. A fresh clone per build
holds no earlier place, and the instance's engine is the one thing an
earlier build can leave running.

### D9. sluice reaches AWS through the SDK's own chain

The CodeBuild profile reaches CodeBuild, S3 and KMS through
`aws-config` and the services' SDK crates, as timber's KMS signer does,
finding credentials as the AWS tools find them at each call and keeping
none (timber record 0017). On a host, the host's own settings may name
`[aws] profile`, which every call uses, so a lead's identity is the
profile the operator gave sluice's user and never the environment of
whichever command started the host process (core.31); unset, the chain
reads the environment. In a build, the chain finds the build's role.

The profile's calls are one trait, `Aws` in
`profiles/codebuild/aws.rs`, implemented by the SDK in `sdk.rs` and by a
fake in `fake.rs` for tests. The SDK's own retries are off and its
timeouts are weir-bounded's; each call runs under sluice's one retry
policy (core.29), a start of a build settled by reading (D3). The SDK's
calls run on a current-thread runtime the profile enters for each call,
so every caller stays blocking (ADR 0062).

*Not taken:* SigV4 by hand over `ureq` with a credential command, as
`[forge] credential` is: a second way to find AWS credentials in the
family beside timber's.

### D10. Setting it up: `sluice init codebuild`

`sluice init codebuild [--systems <s>…]` writes
`.config/sluice-codebuild.yml`, a CloudFormation template, leaving a
differing file unless `--force`, as `sluice init actions` does with its
workflows:

- for each system CodeBuild runs on Linux (`linux-x64`, `linux-arm64`), a
  reserved-capacity fleet in instance mode, its base and maximum
  capacity and compute type parameters, overflowing to its queue, scaled
  by its utilization; and a run project on it, its buildspec inline and
  its timeout a parameter, keeping Docker's layers and the toolchain's
  store in the instance's local caches;
- the sign project, on demand;
- the hand-over bucket, its objects expiring;
- the KMS key, `ECC_NIST_P256`, `SIGN_VERIFY`, aliased `alias/<stack>-sign`,
  whose key policy lets only the sign role sign;
- the secret for the app's key, empty, for the operator to fill;
- the EventBridge rule of D2 and the role it starts the sign build as;
- the run, sign and lead roles of D2 and D3, the lead role assumable by
  the principal the operator names as a parameter.

The template uses CloudFormation's long-form intrinsic functions, so it
reads as plain YAML, and names sluice's release for the builds to
install, as the workflows do. It prints the deploy command, the secret
to fill and the next step.

`sluice init codebuild --stack <name> [--region <r>] [--profile <p>]`
reads the key by its alias with `GetPublicKey`, using the operator's own
chain, and appends `[signers.codebuild]` and `[pools.codebuild]` as text
where the policy names neither, the key named by the ARN KMS answers and
the runners by the projects the template names, the file's own lines
untouched.

### D11. `sluice pool check <branch>`

It names a branch whose trust names no signer a host can sign as, which
no host can lead (D1), so that misconfiguration is told and never idles
silently. For each CodeBuild pool the policy at the branch's revision
names, it reports, with the host's chain: the identity the host reaches AWS as;
each runner project on a reserved-capacity fleet of its system's
environment, its buildspec as `sluice init codebuild` writes it for this
release, and its timeout longer than the branch's `timeout`; the sign
project's buildspec likewise; the key's spec, usage and public half
equal to the policy's; and, saying so without failing, where the branch
tests with `batch = true` or a `parallel` other than the fleets'
maximum. It exits non-zero naming each that fails. A GitHub pool it
names as `sluice forge check`'s.

### D12. Records and rules

Record 0119 gains a build amendment for the CodeBuild pool, dated when
this lands (D1 to D11). `docs/architecture/core.md` gains core.63: a
pool's job releases its checkout before it ends, and a reused machine's
next job starts with nothing of an earlier job's running. core.62's
details name the run build, which holds no key, and the sign build,
which runs none of the combination's code; core.22's details name a
`kms` signer's record verified by the policy's public half with nothing
asked of AWS.

## Looking across the family

- **timber** owns the key-service signer's design (its C.7, its record
  0017): sluice's `kms` signer takes its key spec, its signature and its
  key's name, and cites it. timber's policy carries the public half
  "beside the key's name" in a form it has not yet built; sluice's
  `kms = { key, public }` is offered as that form, so one table names a
  key-service signer in either tool. The orchestrator is told.
- **timber** also reaches AWS through the SDK's chain at each call; D9
  takes the same way, and adds the host's `[aws] profile` for a lead
  that must not take a session's environment.
- **weir** bounds every try (core.29); the AWS calls take its limits.
- **flume and crew** read a pool's run as any run; nothing they read is
  new.
- **The statement**: C.6 is amended (D7), which the orchestrator is told
  as the statement's.

## Risks / Trade-offs

- [The read token is visible] `BatchGetBuilds`, the console and the
  build's event show a run build's variables. → It reads one repository
  and lapses within the hour; only the account's own principals read
  the projects' builds.
- [A reused instance is not confined] A check can leave something in a
  cache the next build reads. → The record claims no confinement (79,
  C.8); the task runner's cache decides no verdict (108); image layers
  are fetched by digest; the guide says so.
- [A heavier build] The AWS SDK crates and a runtime join the binary. →
  timber pins the same; the profile alone enters the runtime.
- [A rule missing or broken] A run build no sign build takes holds its
  claim until `[peers] alive` passes. → The lapse names the rule, and
  `sluice pool check` reads the projects the rule starts.
- [Fleet cost] Instances bill while they run, each for an hour at least.
  → The base capacity is a parameter, sized from the trial's run times
  and waiting (principle 15); the guide says so.
- [The first run is a trial] No test reaches AWS. → The trial of
  record 0119 runs the template on a scratch account, and its recorded
  envelope is kept as a fixture for the test that reads a `kms` signer's
  pass for a gate.

## Migration Plan

Install the release; run `sluice init codebuild`; deploy the template;
fill the secret; name `[aws] profile` on each host that leads the branch;
run `sluice init codebuild --stack`; set the branch's `trust`, `batch`
and `parallel`; `sluice pool check`; land the policy. Rollback: a policy
without the pool, and the stack deleted.

## Open Questions

None.
