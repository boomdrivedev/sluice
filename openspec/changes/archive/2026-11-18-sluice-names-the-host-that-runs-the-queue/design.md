# Design

## Context

See proposal.md, Why. The word reaches a reader in four places:

- the site's pages, `pools.md` above all (25 uses), then
  `reference/configuration.md`, `reference/commands.md`,
  `guides/executors-and-systems.md` and `reference/state.md`;
- the plugin's skills: `setting-up-sluice/SKILL.md`, its
  `references/profiles/peers.md`, and `using-sluice/SKILL.md`;
- what commands print: `sluice step`'s `led` (`host_process/pass.rs`,
  and the round's `sluice.step` field in `sluice-core/src/step.rs`),
  `sluice pool check` (`pool.rs`), `sluice init codebuild` (`init.rs`),
  and three refusals (`instance.rs`'s `[peers]` check,
  `profiles/codebuild/mod.rs`'s launch with no app,
  `profiles/github/app.rs`'s reading token);
- the CloudFormation template, `profiles/codebuild/templates/stack.yml`:
  `LeadPrincipal`, `LeadRole` (a resource and an output), the policy
  `lead`, and its comments and descriptions.

Vale holds no gate (record 0127): `styles/Sluice/Terms.yml` is a
writer's tool, run as `devenv shell -- prose`, and a page's words are
checked by hand. The skills are under no Vale run at all. No other tool
of the family names the step's `led`, `LeadPrincipal`, `LeadRole` or
`sluice-lead` (searched on the mains of flume, crew, boomdrive, weir,
timber, boom and swancloud), and the statement pins none of them
(boomdrivedev/blueprints main at 863a511).

## Goals / Non-Goals

**Goals:**

- One set of words for the host and the role, used the same way on
  every page, in every skill, in output and in the template.
- Each changed output pinned by a test, so the word cannot return to
  what a command prints unnoticed.

**Non-Goals:**

- Renaming code identifiers: `Step::Led`, `Reader::Lead`, the records'
  `lead` field and the conformance suite's sample signer `lead` keep the
  design's word, as `candidate` does beside the pages' "combination".
- Rewording "the host that keeps the branch" where a page says it.

## Decisions

### D1. The words

| Where a page, skill or output says | It says |
|---|---|
| the lead, a lead (the host) | the host that runs the queue, or the host that runs the branch's queue where the branch needs naming |
| leads a branch, leads it | runs a branch's queue, runs its queue |
| the hosts that lead the branch, leading hosts | the hosts that may run the branch's queue (several may; one runs it at a time) |
| is led by a host signing as X | its queue is run by a host signing as X |
| the lead role | the start role |

The singular is for the one host running the queue now; the plural "may
run" is for the hosts a branch's trust accepts, any one of which takes
it through the lease. Not taken: "the host that keeps the branch",
already on some pages, for every use, since the brief names "runs the
queue", which says what the host does, and "the keeper", a word of the
statement's that record 0058 already kept off the pages.

### D2. The step says `elsewhere`

`sluice step`'s line becomes `<branch>: elsewhere: another host runs
the branch's queue, and its lease has not passed`, `outcome` under
`--json` becomes `elsewhere`, and the round's `sluice.step` field says
`elsewhere`, so a trace and the line agree. `elsewhere` is the word the
code already gives the same case on intake (`Taking::Elsewhere` in
`profiles/github/pulls.rs`), and it reads after the branch's name. Not
taken: keeping `led` under `--json` for scripts, which keeps the word
the user could not follow in output and bridges a value no tool of the
family reads; `another-host` and `away`, which say less of where the
queue runs.

### D3. The template's names

The parameter is `StartPrincipal`, the role and its output `StartRole`,
the role's policy `start`; the header comment says "The start role may
start the run projects with a claim and nothing else", the parameter's
description "The ARN of the principal the hosts that may run a branch's
queue on the pool reach AWS as, which may assume the start role", the
output's "The role the hosts that may run a branch's queue on the pool
assume, as their [aws] profile names it". `template.rs`'s tests read
`StartRole`, its test of the role is renamed
`the_start_role_may_change_nothing_a_run_build_runs`, and a new test,
`the_template_names_the_start_role_and_no_lead`, holds the rendered
template to those names and to no `lead` in any case. Not taken:
keeping the logical ids and changing only the descriptions, which
spares a deployed stack a new role but leaves `LeadPrincipal=` in the
deploy command every page shows and `LeadRole` in the output the
operator copies into an AWS profile.

### D4. The examples follow the words

The AWS profile in the pages, `instance.rs`'s module comment and test,
and the codebuild-pool spec is `sluice-start`; the fake AWS's caller
identity (`profiles/codebuild/fake.rs`, `tests/support/aws.rs`) is
`arn:aws:sts::111122223333:assumed-role/sluice-start/host`, so the
sample output `pools.md` and `commands.md` show is the output the CLI
tests assert. The hosts' signer in `pools.md`'s example is `box`, its
keys in `.config/sluice.boxes`, as `configuration.md` and
`pull-requests.md` name it, and the CLI tests' `fleet_policy` trusts
`box` in place of `lead`, so `sluice pool check` prints `main's queue
is run by a host signing as box`.

### D5. The word list

`styles/Sluice/Terms.yml` gains, after its last entry and with a
comment naming record 0058 as amended 2026-11-18:

```yaml
  'the lead role': 'the start role'
  'lead role': 'start role'
  'the lead': 'the host that runs the queue'
  'a lead': 'a host that runs the queue'
  '(?:leads|leading|led)': 'runs the queue'
```

The last key is grouped so the word boundaries Vale puts around a key
hold for each form, and "called" or "misled" is never caught. Vale reads
no code span or block, so `[signers.box]` and the outcome
values are untouched. The last key also catches the one other sense on
the pages, `parallel-and-batch.md`'s "the leading ones" of an act, which
is reworded "the ones at its front" so the pages read clean. The build
checks on a scratch page outside `site/` that "the lead role", "the
lead", "a lead", "leads the branch", "the leading hosts" and "is led
by" are each flagged with the suggestion above; where Vale prefers a
shorter key for "the lead role", it rewrites the keys, never the
suggestions. Not taken: a rule file of its own, since the brief names
the word list, and an `existence` rule, which flags without saying what
to write.

### D6. The records

Record 0058 gains a dated amendment: on the user's words of 2026-11-18,
boomdrive-design's ruling that day names the host that runs the queue
and the start role for what the statement calls the lead and the
template called the lead role. Rule core.26's details add "lead" and its
forms, and "lead role", to what is ruled out on a page, in a skill or in
output, citing the amendment and the signal; core.62's "a lead role that
may start the sign project" reads "a start role that may start the sign
project". The statement keeps its word (core.26, not ruled out), so no
clause changes.

### D7. The sweep

A page is swept with `rg -n -i -w 'lead|leads|leading|led|lead.s'` over
`site/src/content/docs` (leaving out `decisions/` and
`architecture/rules.md`, which are made from the records and the rules)
and over `plugins/sluice/skills`, and each use in the host's or the
role's sense is rewritten by D1. `read-back.md`'s "leads with the
warning" and "Lead that branch's block" are another sense and stay. The
code's own output is swept with the same search over `crates/*/src`,
leaving comments, identifiers and test data alone.

## Risks / Trade-offs

- [A script reads `outcome` for `led`] → The commands page's `sluice
  step` rows give `elsewhere`; no tool of the family reads the value.
- [A stack deployed from an earlier template] → Deploying the new one
  makes `StartRole` and deletes `LeadRole`; a host whose profile still
  assumes the old role is refused by AWS, and `sluice pool check` names
  the identity it reached AWS as, and a refused start is logged on the
  branch (core.28). The migration below is the operator's.
- [Two phrases for one host, "keeps the branch" and "runs the queue"]
  → Both are plain; a page that says both in one paragraph uses one.
- [A Vale key catches a word of another sense] → It is a warning a
  writer sees when running `prose`; the page is reworded, as D5 does
  once.

## Migration Plan

A stack deployed from a template before this change:

1. `sluice init codebuild --force` writes the template again.
2. `aws cloudformation deploy --template-file .config/sluice-codebuild.yml
   --stack-name <stack> --capabilities CAPABILITY_IAM
   --parameter-overrides StartPrincipal=<the ARN the hosts reach AWS as>
   AppId=<sluice's app id> MaxCapacity=<the branch's parallel>`.
3. On each host that may run the branch's queue, the AWS profile's
   `role_arn` names the stack's `StartRole` output.
4. `sluice pool check <branch>` reads the setup back.

Rollback is the earlier release's template deployed with
`LeadPrincipal`. Nothing in a branch's state or records moves.
