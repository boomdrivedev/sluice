# Tasks

The base is `bolt/sluice-reports-its-work-plainly`. A page's words are
checked by Vale by hand, `devenv shell -- vale <the pages>`, since Vale
holds no gate and no gate task reads a page's prose (record 0127;
core.21); the word list comes first, so every later page check flags a
use left behind.

## 1. The word list and the records

- [x] 1.1 `styles/Sluice/Terms.yml` holds design D5's five entries after its last, under a comment naming record 0058 as amended 2026-11-18, and flags "the lead role", "the lead", "a lead", "leads the branch", "the leading hosts" and "is led by" each with D5's suggestion, and neither "called" nor "misled"; check, by hand for record 0127's reason: `devenv shell -- vale <scratch>/lead.md` on a page outside `site/` holding those phrases and words, the keys rewritten (never the suggestions) where Vale gives another suggestion
- [x] 1.2 Record 0058 carries an amendment dated 2026-11-18 under its Sources line, quoting the user ("i don't understand the "leads" part i see in the docs") and the signal `2026-11-18-boomdrive-operator-33bf2bfc/01-sluice-lead-is-unclear`, and giving boomdrive-design's ruling of that day: the pages, the skills, the output and the template say the host that runs the queue and the start role where the statement says the lead; check, by hand since no gate task reads a record (core.21; record 0127): `rg -n "Amended 2026-11-18" docs/adr/0058-*.md` shows it
- [x] 1.3 Rule core.26's details in `docs/architecture/core.md` rule out "lead", "the lead", "leads", "leading" and "led" for the host, and "lead role", on a page, in a skill or in output, where the host that runs the queue or the start role is meant, citing record 0058 as amended 2026-11-18 and the signal; core.62's details say "a start role that may start the sign project"; check, by hand for 1.2's reason: `rg -n "start role" docs/architecture/core.md` shows both

## 2. The step says elsewhere

- [x] 2.1 `sluice step` on a host while another host runs the branch's queue prints `<branch>: elsewhere: another host runs the branch's queue, and its lease has not passed`, gives `"outcome": "elsewhere"` under `--json`, and the round's `sluice.step` field says `elsewhere` (`host_process/pass.rs`, `sluice-core/src/step.rs`; `Step::Led` keeps its name); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli step_on_a_host_that_does_not_run_the_queue_says_elsewhere`, a new test the peer-lead spec names
- [x] 2.2 The commands page's `sluice step` row and `--json` row give `elsewhere`, its "Who runs the rounds" says a round on another host says `elsewhere` and leaves the branch alone, and `executors-and-systems.md`'s "Watching it" says `sluice step <branch>` says `elsewhere` on a host that does not run the branch's queue; check, by hand for record 0127's reason: `devenv shell -- vale site/src/content/docs/reference/commands.md site/src/content/docs/guides/executors-and-systems.md` raises no `Sluice.Terms` warning for those lines
- [x] 2.3 `using-sluice/SKILL.md` says `sluice step <branch>` printing `elsewhere` means another host runs the branch's queue; check: `sluice exec bolt/sluice-reports-its-work-plainly -- moon run plugin-sluice:validate`

## 3. The template names the start role

- [x] 3.1 `templates/stack.yml` names its parameter `StartPrincipal`, its role and the role's output `StartRole`, and the role's policy `start`, with design D3's comment and descriptions, and `template.rs`'s tests read `StartRole`; check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib the_start_role_may_change_nothing_a_run_build_runs`, the role's test renamed from `the_lead_may_change_nothing_a_run_build_runs`
- [x] 3.2 The rendered template holds `StartPrincipal`, `StartRole` and the policy `start`, and no `lead` in any case; check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib the_template_names_the_start_role_and_no_lead`, a new test in `template.rs`
- [x] 3.3 `sluice init codebuild`'s deploy command passes `StartPrincipal=<the ARN the hosts that may run the branch's queue reach AWS as>`, and it says to name the profile that assumes the stack's start role as `[aws] profile` on each host that may run the branch's queue, with no `lead` in what it prints (`init.rs`); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli init_codebuild_writes_the_stack`, asserting those words and no `lead`
- [ ] 3.4 `sluice init codebuild --stack` says a branch's `trust` names codebuild and the signer the hosts that may run its queue sign as (`init.rs`); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli init_codebuild_from_a_stack_appends_the_pool`, asserting that line
- [ ] 3.5 The AWS profile in `instance.rs`'s module comment and in its test is `sluice-start`; check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib an_aws_profile_in_the_settings_is_the_one_used`
- [ ] 3.6 `pools.md`'s CodeBuild setup (the `sluice init codebuild` sample, the stack's roles, `StartPrincipal`, the `[aws]` example `profile = "sluice-start"`, the `--stack` sample), the commands page's `sluice init codebuild` sample, and `configuration.md`'s `[aws]` example, its row and its prose say the start role, `StartPrincipal` and the hosts that may run the branch's queue; check, by hand for record 0127's reason: `devenv shell -- vale site/src/content/docs/guides/pools.md site/src/content/docs/reference/commands.md site/src/content/docs/reference/configuration.md` raises no `Sluice.Terms` warning for those lines
- [ ] 3.7 `setting-up-sluice/SKILL.md`'s pool row and its CodeBuild step say the hosts that may run the branch's queue, the profile that assumes the stack's start role, and the signer those hosts sign as; check: `sluice exec bolt/sluice-reports-its-work-plainly -- moon run plugin-sluice:validate`

## 4. sluice pool check says who may run the queue

- [ ] 4.1 `sluice pool check` says `<branch>'s queue is run by a host signing as <signers>` where the trust names a signer a host signs as (`pool.rs`); the fake AWS's identity is `assumed-role/sluice-start/host` (`profiles/codebuild/fake.rs`, `tests/support/aws.rs`); the CLI tests' `fleet_policy` trusts `box` in place of `lead`; check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli pool_check_says_a_fleet_tests_side_by_side`, asserting `ok      main's queue is run by a host signing as box` and the `sluice-start` identity
- [ ] 4.2 Where the trust names no signer a host signs as, `sluice pool check` fails saying `<branch>'s trust names no signer a host signs as, so no host can run its queue and it forms nothing: trust the signer the hosts that run its queue sign as beside the pool's, with systems = [] so it claims no run`; check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --test cli pool_check_names_a_branch_whose_queue_no_host_can_run`, the test renamed from `pool_check_names_a_branch_no_host_can_lead`
- [ ] 4.3 `pools.md`'s "Test side by side" (its example's signer `box`, `.config/sluice.boxes`) and its `sluice pool check` sample, the commands page's `sluice pool check` section and sample, `configuration.md`'s `[signers]` `systems` row, and `executors-and-systems.md`'s pool paragraph say the hosts that may run the branch's queue; check, by hand for record 0127's reason: `devenv shell -- vale site/src/content/docs/guides/pools.md site/src/content/docs/reference/commands.md site/src/content/docs/reference/configuration.md site/src/content/docs/guides/executors-and-systems.md` raises no `Sluice.Terms` warning for those lines

## 5. The refusals say the host that runs the queue

- [ ] 5.1 A host's settings whose `[peers] alive` is not shorter than its `lease` are refused with `[peers] alive must be shorter than lease, so a host that runs a branch's queue and is alive keeps running it` (`instance.rs`); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib an_alive_as_long_as_the_lease_is_refused`, a new test
- [ ] 5.2 A CodeBuild pool's launch on a host whose `[forge]` names no app refuses, saying the token is bought by the host that runs the branch's queue and that a host that runs the queue of a branch on a CodeBuild pool names the app in `[forge]` (`profiles/codebuild/mod.rs`); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib a_launch_on_a_host_with_no_app_names_the_host_that_runs_the_queue`, a new test
- [ ] 5.3 A reading token asked of a host whose `[forge]` names a credential command refuses, saying to name the app's key as `[forge] key` on a host that runs the queue of a branch on a CodeBuild pool (`profiles/github/app.rs`); check: `sluice exec bolt/sluice-reports-its-work-plainly -- cargo test -p sluice --lib a_credential_command_buys_no_reading_token`, a new test

## 6. The rest of the pages and skills

- [ ] 6.1 `pools.md`'s opening, its claim paragraph, "What a run does" and "With every host off", `configuration.md`'s `[peers]` `name` row, `state.md`'s `no_executor` line and `parallel-and-batch.md`'s "the ones at its front" say the host that runs the queue by design D1, and design D7's sweep over `site/src/content/docs` finds the word in no page but `decisions/` and `architecture/rules.md`; check, by hand for record 0127's reason: `devenv shell -- vale site/src/content/docs/guides/pools.md site/src/content/docs/reference/configuration.md site/src/content/docs/reference/state.md site/src/content/docs/guides/parallel-and-batch.md` raises no `Sluice.Terms` warning
- [ ] 6.2 `setting-up-sluice/references/profiles/peers.md`'s `lease` comment and "Two hosts both think they run the queue" say the host that runs the queue, and design D7's sweep over `plugins/sluice/skills` finds the word only in `read-back.md`'s other sense; check: `sluice exec bolt/sluice-reports-its-work-plainly -- moon run plugin-sluice:validate`

## Workflow follow-up

- The verify stage runs the base's whole checks once, `sluice exec
  bolt/sluice-reports-its-work-plainly`, after every task is done.
- The change is archived with `openspec archive` once verified.
