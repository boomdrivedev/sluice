# Tasks

Work in this change's place on its branch, cut from `main`, the base
every check below runs against. Run suites through sluice, as
CLAUDE.md's Running checks has it; a quick `cargo check` or one unit
test may run directly. Commit by pathspec, Conventional Commits,
scoped to the part of the tree, citing clauses by number and rules by
id.

## 1. A fail stands, whatever the sandbox refused

- [x] 1.1 `RunReport` carries `refused`, the sandbox's words; `after_run` keeps a run's fails where the sandbox refused something, and a switch or an engine gone still gives them no outcome (design D1). True when `sluice exec main -- cargo nextest run -p sluice-conformance --test could_not_run -E 'test(refus)'` passes.
- [x] 1.2 A check failed by a task the run was refused is failed by it. True when `sluice exec main -- cargo nextest run -p sluice-conformance --test failed_dependency -E 'test(a_refusal_leaves_a_check_failed_by_a_dependency_failed_by_it)'` passes.
- [x] 1.3 The run's `detail` line, `sluice status` and `sluice exec` say what was refused beside the fails. True when `cargo nextest run -p sluice --features tools --test exec -E 'test(exec_records_a_fail_the_sandbox_refused_with_its_words_and_its_pass_under_a_wider_grant)'` passes outside any sandbox, as `moon run cli:sandbox` runs it.
- [x] 1.4 Rules core.7, core.35 and core.37, the site's reference pages and the using-sluice skill say D1. True when `sluice exec main -- moon run root:format` passes.
- [x] 1.5 D1 proposed in Boom as the decision `a-fail-stands-whatever-the-sandbox-refused`, with a new version of `a-refusal-is-read-from-the-executors-own-report`. True when `boom show a-fail-stands-whatever-the-sandbox-refused` gives it.

## 2. The sandbox lets the tools do what each run does

- [x] 2.1 A sandboxed run held from the network starts its command with `PROTO_OFFLINE=1` (design D2). True when `sluice exec main -- cargo nextest run -p sluice --lib -E 'test(a_run_held_from_the_network_is_told_it_is_offline)'` passes.
- [x] 2.2 The sandbox opens moon's task archives where the checkout's link leads them (design D3). True when `sluice exec main -- cargo nextest run -p sluice --lib -E 'test(the_profile_opens_moons_archives_where_their_link_leads)'` passes.
- [x] 2.3 D2 and D3 proposed in Boom as a new version of `a-sandboxed-run-writes-only-where-its-profile-allows`. True when `boom show a-sandboxed-run-writes-only-where-its-profile-allows` gives it.

## 3. The flaky tests

- [x] 3.1 The held-landing tests step until a step runs a round (design D5). True when `sluice exec main -- cargo nextest run -p sluice --features tools --test remote_branch -E 'test(a_held_landing)'` passes.
- [x] 3.2 `s75_a_combination_starts_before_a_third_agents_check` holds each agent's check until the overview shows what waits for room. True when `sluice exec main -- cargo nextest run -p sluice --test host_process -E 'test(s75)'` passes.
- [ ] 3.3 The hand-over tests read the host process's pid as it is after a hand-over under load (design D5). True when `sluice exec main -- cargo nextest run -p sluice --test host_process -E 'test(hand_over) or test(an_upgrade)'` passes.
