# Verify

The change `sluice-names-the-host-that-runs-the-queue` on
`change/sluice-names-the-host-that-runs-the-queue` at `707d04183`,
against its base and gate, `bolt/sluice-reports-its-work-plainly` at
`b865b5368` (S66). The branch was cut at `ba58c01d4`; the base has since
landed #109, and the branch merges onto `b865b5368` with no conflict.

Verdict: the whole checks pass, every task is done, and every requirement
and scenario of the delta specs is met by code and a test that passes. The
change is ready to archive.

## The whole checks

`sluice exec bolt/sluice-reports-its-work-plainly`, run once on
2026-11-18: the bolt's gate, `#check-fast`, for the checks `707d041`
affects against `b865b53`. Exit 0, after waiting for room at the host's
bound.

| Task | Outcome |
|---|---|
| `plugin-sluice:validate` | passed |
| `root:format` | passed |
| `nix:typecheck` | passed |
| `root:changes` | passed |
| `core:test` | passed, 172 |
| `fakes:test` | passed, 17 |
| `root:lint` | passed |
| `conformance:test` | passed |
| `cli:test` | passed |

sluice said `passed: 9 tasks` and `recorded 9 results under their keys for
707d041`. `root:specs` was not among the affected checks, since the
change writes no spec under `openspec/specs/` until it is archived;
`root:changes` archived it into a scratch copy of the specs and held what
it wrote to `openspec validate --strict` (core.61). The `failed:
api:integration` and `not recorded: worktree is not clean` lines in
`cli:test`'s output are the words of CLI tests that assert them. moon
warned that it could not archive each task's outputs into the place's
`.moon/cache`, which decides nothing.

`openspec validate sluice-names-the-host-that-runs-the-queue --strict`:
valid.

`openspec instructions apply --change sluice-names-the-host-that-runs-the-queue --json`:
`all_done`, 21 of 21 tasks complete.

## Tasks

All 21 are ticked. A task whose check is a test is met where that test
passes in the gate above (`core:test` runs `--lib` tests of
`sluice-core`, `cli:test` the `sluice` crate's library and CLI tests);
a task whose check is by hand (record 0127; core.21) was checked here as
it names.

| Task | Check | Outcome |
|---|---|---|
| 1.1 `styles/Sluice/Terms.yml` | D5's five keys after the last entry, under a comment naming record 0058 as amended 2026-11-18; Vale on a scratch page outside `site/` | met: "the lead role", "the lead", "a lead", "leads the branch", "the leading hosts" and "is led by" each flagged with D5's suggestion; "called" and "misled" not flagged |
| 1.2 record 0058 | `rg -n "Amended 2026-11-18" docs/adr/0058-*.md` | met: the amendment under Sources, quoting the user and the signal, giving boomdrive-design's ruling |
| 1.3 core.26, core.62 | `rg -n "start role" docs/architecture/core.md` | met: core.26's details rule out "lead" and its forms and "lead role", citing record 0058 as amended and the signal; core.62 says "a start role that may start the sign project" |
| 2.1 `sluice step` says `elsewhere` | `step_on_a_host_that_does_not_run_the_queue_says_elsewhere` | passed (`cli:test`) |
| 2.2 commands page, executors page | Vale on both pages | met: the `sluice step` row, the `--json` row and "Who runs the rounds" say `elsewhere`; "Watching it" says it; no `Sluice.Terms` warning |
| 2.3 `using-sluice/SKILL.md` | `plugin-sluice:validate` | passed |
| 3.1 the template's names | `the_start_role_may_change_nothing_a_run_build_runs` | passed (`cli:test`) |
| 3.2 the rendered template | `the_template_names_the_start_role_and_no_lead` | passed (`cli:test`) |
| 3.3 `sluice init codebuild` | `init_codebuild_writes_the_stack` | passed (`cli:test`) |
| 3.4 `sluice init codebuild --stack` | `init_codebuild_from_a_stack_appends_the_pool` | passed (`cli:test`) |
| 3.5 `instance.rs`'s profile | `an_aws_profile_in_the_settings_is_the_one_used` | passed (`cli:test`) |
| 3.6 pools, commands, configuration pages | Vale on the three pages | met: `StartPrincipal`, the start role, `profile = "sluice-start"` and the hosts that may run the branch's queue; no `Sluice.Terms` warning |
| 3.7 `setting-up-sluice/SKILL.md` | `plugin-sluice:validate` | passed |
| 4.1 `sluice pool check` says who runs the queue | `pool_check_says_a_fleet_tests_side_by_side` | passed (`cli:test`) |
| 4.2 no host can run the queue | `pool_check_names_a_branch_whose_queue_no_host_can_run` | passed (`cli:test`) |
| 4.3 pools, commands, configuration, executors pages | Vale on the four pages | met: `box`, `.config/sluice.boxes`, `main's queue is run by a host signing as box`; no `Sluice.Terms` warning |
| 5.1 `[peers] alive` against `lease` | `an_alive_as_long_as_the_lease_is_refused` | passed (`cli:test`) |
| 5.2 a launch with no app | `a_launch_on_a_host_with_no_app_names_the_host_that_runs_the_queue` | passed (`cli:test`) |
| 5.3 a credential command's reading token | `a_credential_command_buys_no_reading_token` | passed (`cli:test`) |
| 6.1 the rest of the pages | Vale on pools, configuration, state, parallel-and-batch; D7's sweep | met: no `Sluice.Terms` warning; the sweep finds the word in no page outside `decisions/` and `architecture/rules.md` |
| 6.2 the rest of the skills | `plugin-sluice:validate`; D7's sweep | passed; the sweep finds the word only in `read-back.md`'s other sense (lines 19, 125, 157) |

The by-hand checks: `devenv shell -- vale --config .vale.ini` over
`guides/pools.md`, `reference/commands.md`, `reference/configuration.md`,
`guides/executors-and-systems.md`, `reference/state.md` and
`guides/parallel-and-batch.md` said `0 errors, 0 warnings and 58
suggestions in 6 files`. D7's sweep, `rg -n -i -w
'lead|leads|leading|led|lead.s'`, over `site/src/content/docs` (leaving
out `decisions/` and `architecture/rules.md`) found nothing, and over
`plugins/sluice/skills` found only `read-back.md`'s "leads with the
warning" and "Lead that branch's block". The same sweep over
`crates/*/src` finds the word only in identifiers (`Step::Led`,
`Reader::Lead`, `Taking::Elsewhere(lead)`), comments, test data (the
policy's and settings' refused `lead` keys, the conformance suite's
sample signer `lead`) and the template test's own assertion, none of
which a command prints.

The word list's `ignorecase: false`, as every entry before it, means
Vale flags "the lead" and "a lead" mid-sentence but not "The lead" or
"A lead" opening one; D5 gives the keys in lowercase and task 1.1's
phrases are flagged as written, so this is no shortfall of the change,
only a limit of the list a writer should know.

## Requirements and scenarios

### codebuild-pool

| Requirement / scenario | Met by | Outcome |
|---|---|---|
| RENAMED "sluice pool check names a branch no host can lead" to "sluice pool check names who may run a branch's queue" | the main spec holds the FROM name (`openspec/specs/codebuild-pool/spec.md`) | met |
| ADDED "The start role changes nothing a run build runs" / "The start role in the template" | `stack.yml`'s `StartRole`; `the_start_role_may_change_nothing_a_run_build_runs` | passed |
| ADDED, same / "The template's names" | `StartPrincipal`, `StartRole` (resource and output), policy `start`, no `lead` in any case; `the_template_names_the_start_role_and_no_lead` | passed |
| MODIFIED "The host names the AWS profile sluice reaches a pool as" / "A host naming a profile" | `instance.rs`; `an_aws_profile_in_the_settings_is_the_one_used` | passed |
| MODIFIED "sluice init codebuild writes the stack" / "A repository with no pool" | `init.rs`'s deploy command with `StartPrincipal`, the start role's profile on each host that may run the branch's queue, no `lead`; `init_codebuild_writes_the_stack` | passed |
| same / "A template already there" | unchanged; `init_codebuild_leaves_a_differing_template_unless_forced` | passed |
| MODIFIED "sluice pool check names who may run a branch's queue" / "A gate that trusts only the pool's signer" | `pool.rs`; `pool_check_names_a_branch_whose_queue_no_host_can_run` | passed |
| same / "A gate that trusts the hosts' signer beside the pool's" | `pool.rs`'s `<branch>'s queue is run by a host signing as`; `pool_check_says_a_fleet_tests_side_by_side` asserts `ok      main's queue is run by a host signing as box` | passed |
| REMOVED "The lead's identity changes nothing a run build runs" | the main spec holds it; its bounds are the ADDED requirement's, its test renamed | met |

### peer-lead

| Requirement / scenario | Met by | Outcome |
|---|---|---|
| ADDED "A step on a host that does not run a branch's queue says another host runs it" / "Host B steps a branch whose queue host A runs" | `host_process/pass.rs`'s line and `outcome`, `sluice-core/src/step.rs`'s `sluice.step` field; `step_on_a_host_that_does_not_run_the_queue_says_elsewhere`, asserting the line, `"outcome": "elsewhere"` and no `led` word in either | passed |
