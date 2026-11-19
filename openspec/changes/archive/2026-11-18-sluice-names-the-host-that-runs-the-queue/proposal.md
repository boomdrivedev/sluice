# Proposal

## Why

sluice's pages, `sluice step`'s output and the CodeBuild template
`sluice init codebuild` writes call the one host that moves a branch's
queue "the lead", and the AWS role that host starts builds with "the
lead role". "Lead" is the statement's word (72, 73, 137), which a reader
cannot see in the product, and the user could not follow it: "i don't
understand the "leads" part i see in the docs" (2026-11-18, signal
`2026-11-18-boomdrive-operator-33bf2bfc/01-sluice-lead-is-unclear`).
Rule core.26 already rules such a word out of a page, a skill and a
command's output; this change makes the product keep it.

## What Changes

- The site's pages and the plugin's skills call that machine "the host
  that runs the queue", or "the host that runs the branch's queue" where
  the branch needs naming, and the hosts a branch accepts for it "the
  hosts that may run its queue"; never "the lead", "leads a branch",
  "leading hosts" or "led".
- **BREAKING** `sluice step` says `elsewhere` where it said `led`, as
  its line, `<branch>: elsewhere: another host runs the branch's queue,
  and its lease has not passed`, and as `outcome` under `--json`; the
  round's `sluice.step` field says the same.
- `sluice pool check` says whose hosts may run the branch's queue, or
  that no host can run it; `sluice init codebuild` tells the operator to
  pass `StartPrincipal` and to name, on each host that may run the
  branch's queue, the profile that assumes the stack's start role; the
  refusals that said "lead" (`[peers] alive` against `lease`, a pool's
  launch on a host with no app, a credential command asked for a
  reading token) say the host that runs the queue.
- **BREAKING** The CloudFormation template `sluice init codebuild`
  writes names its parameter `StartPrincipal`, its role and that role's
  output `StartRole` and the role's policy `start`, and its comments and
  descriptions say the start role and the hosts that run the queue. A
  stack deployed from an earlier template is written again with
  `sluice init codebuild --force` and deployed again with
  `StartPrincipal`, and each host's AWS profile assumes the new role.
- The site's word list, `styles/Sluice/Terms.yml`, flags "lead" and its
  forms where a page would use them, suggesting the host that runs the
  queue or the start role.
- Rule core.26 names the word among those ruled out, record 0058 is
  amended with the ruling, and rule core.62 names the template's role
  the start role.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `codebuild-pool`: the template's role is the start role, assumed by
  `StartPrincipal`, and the template names no lead; `sluice init
  codebuild` prints `StartPrincipal` and the start role; the AWS profile
  in the example is `sluice-start`; `sluice pool check` says which
  signer's hosts may run the branch's queue, or that no host can run it.
- `peer-lead`: a step on a host that does not run a branch's queue says
  `elsewhere`: another host runs the branch's queue.

## Out of scope

- The statement, the decision records other than record 0058's
  amendment, the rules' and the specs' own use of "lead" for the host,
  and code identifiers (`Step::Led`, `Reader::Lead`, the conformance
  suite's sample signer `lead`): core.26 leaves the design's vocabulary
  to the documents written for whoever builds sluice. No later change
  waits on this.
- The `lead` field of a branch's state, `branch.json`, which the state
  page does not name and every host sharing a remote reads: renaming it
  is a change of the records' form that every host's sluice must take at
  once. It waits on no planned change; one that changes the records'
  form would carry it.
- "The host that keeps the branch", the pages' other phrase for the same
  host, stays: it is plain English and the user did not stumble on it.
  No later change waits on this.

## Impact

- `crates/sluice/src/host_process/pass.rs` and
  `crates/sluice-core/src/step.rs`: the step's words.
- `crates/sluice/src/pool.rs`, `crates/sluice/src/init.rs`,
  `crates/sluice/src/instance.rs`,
  `crates/sluice/src/profiles/codebuild/mod.rs`,
  `crates/sluice/src/profiles/github/app.rs`: output and refusals.
- `crates/sluice/src/profiles/codebuild/templates/stack.yml` and its
  tests in `template.rs`; the fake AWS identities in
  `crates/sluice/src/profiles/codebuild/fake.rs` and
  `crates/sluice/tests/support/aws.rs`; the CLI tests in
  `crates/sluice/tests/cli.rs`.
- `site/src/content/docs/guides/pools.md`,
  `guides/executors-and-systems.md`, `guides/parallel-and-batch.md`,
  `reference/commands.md`, `reference/configuration.md`,
  `reference/state.md`; `plugins/sluice/skills/`.
- `styles/Sluice/Terms.yml`; `docs/architecture/core.md` (core.26,
  core.62); `docs/adr/0058-*.md`.
- Scripts that read `sluice step --json`'s `outcome` for `led`, and any
  CodeBuild stack deployed from an earlier template.
