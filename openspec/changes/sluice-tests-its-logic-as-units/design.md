# Design

## Context

sluice's tests are in four layers today:

| Layer | Where | Tests | Starts |
|---|---|---|---|
| Units in the source | `crates/*/src/**` | 580 | nothing, but some start git |
| The CLI over the fakes | `crates/sluice/tests/cli.rs` and seven smaller files | 330 | nothing, but five tests start the binary |
| The conformance scenarios over the fakes | `crates/sluice-conformance/tests/` | 430 | nothing |
| The profiles and the binary against real tools | the other files of `crates/sluice/tests/` | 953 | the binary, a host process, git, wt, moon, a fake HTTP server |

The fourth layer is the problem. Its tests prove logic through the whole binary. For example, `pull_requests.rs` has 171 tests. Each makes a bare repository, starts a fake of GitHub's API on a port, and runs `sluice step` or `wt submit` several times. Each such run starts a host process and git. The logic that these tests prove is in `Pulls`, which `sluice::run` already takes through the `PullRequests` trait.

## Goals

- Each behaviour is proven by exactly one test.
- A unit test starts no process, no git and no network.
- The end-to-end set is a few dozen tests, and each ends the processes that it starts.
- The gate's wall time and its peak count of processes fall.

## Non-goals

- A change to what sluice does. Where a seam changes, the production code uses the seam in the same way that the test does.
- Coverage of behaviour that no test proves today.

## Decisions

### D1. Where a test goes

Each test that starts a process today goes to one of four places, in this order:

1. **It goes.** Another test already proves its behaviour. `moves.md` names that test.
2. **A unit in the source.** The behaviour is a function of its inputs: a parse, a choice, a text. The test moves to the `tests` module of the file that holds the function. Where the function is inside a profile with git or a socket around it, the function moves out into a pure function first.
3. **An in-process test over the fakes.** The behaviour is a command or a round. The test runs `sluice::run` over `sluice_fakes::Fakes`, with `support::FakeHost` for the host process, as `cli.rs` does today. Where a wait or a pace matters, it runs on the fakes' clock or on tokio's paused clock.
4. **The end-to-end set.** The behaviour is the binary's own: a command's main path, the host process's life, or a seam's contract with a real tool. D2 names each test of the set.

A moved test keeps its name. A spec scenario or a Boom claim that names a test still finds it. Where a spec cites a test's file, this change's delta spec changes the citation in the same step.

### D2. The end-to-end set

The set has 47 tests. Each is a test that exists today. A test that checks several steps of one main path stays one test.

**Each command's main path (21):**

| Command | Test | File |
|---|---|---|
| `wt submit` | `wt_submit_lands_or_fails_by_its_exit` | `profiles/worktrunk.rs` |
| `sluice submit --no-wait`, `status` | `no_wait_returns_and_status_reads_the_outcome` | `profiles/worktrunk.rs` |
| `sluice inbox`, `confirm` | `inbox_hands_a_landing_over_until_it_is_confirmed` | `profiles/worktrunk.rs` |
| `sluice wait` | `sluice_wait_removes_nothing` | `profiles/worktrunk.rs` |
| `sluice withdraw` | `a_withdrawal_starts_the_host_process` | `host_process.rs` |
| `sluice amend` | `an_amend_and_a_confirmation_run_no_git` | `host_process.rs` |
| `sluice step` | `a_step_is_the_host_processs_round` | `host_process.rs` |
| `sluice overview` | `the_overview_names_the_host_process` | `host_process.rs` |
| `sluice overview --watch` | `a_watch_of_one_folder_prints_as_before` | `overview_watch.rs` |
| `sluice run` | `sluice_run_prints_the_passes_of_its_subscription` | `host_process.rs` |
| `sluice daemon` | `a_daemon_lands_a_submission_with_no_other_host_process` | `host_process.rs` |
| `sluice exec -- <command>` | `a_command_gets_the_worktrees_environment_and_its_exit` | `exec.rs` |
| `sluice exec` | `a_clean_run_of_the_checks_is_recorded_and_a_dirty_one_is_not` | `exec.rs` |
| `sluice release` | `an_unheld_worktrees_release_runs_its_release_command` | `exec.rs` |
| `sluice preview`, `previews` | `the_previews_are_answered_by_sluice_host` | `previews_tools.rs` |
| `sluice equip` | `exec_equips_before_its_command` | `equip_tools.rs` |
| `sluice protect` and its hook | `sluice_protect_is_made_by_sluice_host` | `protect.rs` |
| `sluice branch` | `a_bolt_is_made_landed_on_and_deleted_on_its_remote` | `remote_branch.rs` |
| `sluice forge check` | `forge_check_all_set` | `profiles/forge.rs` |
| `sluice submit` to a branch that takes pull requests | `submit_opens_then_updates_a_pull_request` | `profiles/pull_requests.rs` |
| A round's intake of a pull request | `landed_and_merged_by_landing` | `profiles/pull_requests.rs` |

**The host process's life (9):**

| Part | Test | File |
|---|---|---|
| One host process for many reaches | `ten_reaches_at_once_start_one_host_process` | `host_process.rs` |
| A kill loses nothing | `a_killed_host_process_loses_nothing` | `host_process.rs` |
| A hand-over to a newer binary | `a_newer_client_has_an_older_host_process_hand_over` | `host_process.rs` |
| A stop ends the runs | `a_stopped_host_process_ends_its_runs` | `host_process.rs` |
| An idle host process ends | `the_host_process_ends_once_its_queues_are_empty` | `host_process.rs` |
| The service manager keeps the daemon | `a_command_starts_no_host_process_where_the_service_manager_keeps_the_daemon` | `host_process.rs` |
| A write reaches a watch by its file event | `a_records_write_reaches_a_watch_by_its_file_event` | `host_process.rs` |
| The test guard fails a test that leaves a run | `a_test_that_leaves_a_run_fails` | `host_process.rs` |
| The test guard finds a detached process | `the_guard_finds_a_detached_host_process` | `host_process.rs` |

**Each seam with a real tool (17):**

| Seam and tool | Test | File |
|---|---|---|
| Branch, git in this host's refs | `form_a_chain_of_three` | `local_refs.rs` |
| Branch, git on a remote | `a_push_lands_and_leaves_the_copy` | `pushed_refs.rs` |
| State, records on a remote | `two_hosts_write_one_version_and_one_is_refused` | `remote_refs.rs` |
| State, the lock of this host's files | `the_lock_is_one_holder_and_dies_with_it` | `local_files.rs` |
| Checkout, git worktree places | `a_git_worktree_place_is_the_candidate_and_leaves_nothing` | `profiles/places.rs` |
| Tests, moon's plan | `the_tasks_a_range_affects` | `profiles/moon.rs` |
| Tests, moon's run and proof | `a_failure_and_its_dependent` | `profiles/moon.rs` |
| Tests, the command runner | `a_command_passes_or_fails_by_its_exit` | `checks.rs` |
| Tests, the sandbox | `the_sandbox_holds_writes_and_the_network` | `checks.rs` |
| A run's supervisor | `a_run_ends_everything_it_started` | `runs.rs` |
| The container engine's socket | `the_places_containers_are_removed_and_no_other` | `checks.rs` |
| A stack beside a place | `two_stacks_side_by_side_and_a_time_limit_leaves_nothing` | `stack.rs` |
| Driver, a herdr pane | `a_landing_is_pushed_into_the_pane_it_was_submitted_from` | `profiles/worktrunk.rs` |
| Forge, GitHub's REST over HTTP | `every_read_is_conditional` | `profiles/forge.rs` |
| Forge, a credential command | `a_commands_credentials_say_what_their_token_is` | `profiles/forge.rs` |
| Telemetry, OTLP over HTTP | `telemetry_goes_to_the_endpoint_as_otlp_over_http` | `telemetry_logs.rs` |
| Two hosts on one remote | `a_submission_lands_by_another_host` | `peers.rs` |

The conformance scenarios over the real profiles, in `profiles/conformance.rs`, go. `e2e/check.sh` drives S1 to S4 through the real tools. The conformance crate proves each scenario over the fakes.

### D3. The pull request driver takes git through a seam

`Pulls` reads the remote's address through `LocalRefs`, and fetches a request's head and pushes a submit's branch through `PushedRefs`. These calls are outside the seams, so no fake can stand for them. So each test of the driver needs real git. `Pulls` takes them through a trait of its own, `RequestRefs`. It has one method for each call: the remote's address, the fetch of a head, and the push of a branch. `PushedRefs` implements it. `sluice_fakes::World` implements it over its own commits. Then each scenario of `pull_requests.rs` runs in-process: `sluice::run` over the fakes, with `Pulls` over `FileForge` and the fakes' `RequestRefs`.

The forge's HTTP behaviour moves to units of `rest.rs`: the conditional reads, the pauses on a spent allowance, the 503 tries. In these units, a test gives a transport in place of ureq's agent. One end-to-end test, `every_read_is_conditional`, runs the real agent against the fake server.

### D4. The host process's logic runs in-process

Most tests of `host_process.rs` prove what the host process decides. Examples: ten statuses share one read, and an older client receives the host process's version. The host process's code is a library, `sluice::host_process`, and `support::FakeProcess` already runs a pass in the test's own process. These tests run the host process's handler in-process, on tokio's paused clock. A client connects to it through an in-memory stream. The tests of the process's own life stay in the end-to-end set (D2).

A test that proves that a command runs no git becomes one end-to-end assertion. The main-path tests count git's starts through `CountingGit`, and a command's client starts none.

### D5. The end-to-end set has a bound

The `tools` task runs the end-to-end set alone. In `.config/nextest.toml`, a test group `e2e` holds each test of the set, with `max-threads = 4`. So the set starts at most 4 binaries and their host processes at one time, whatever the host's cores.

### D6. Seams with one profile

A profile's logic that moves out into pure functions stays in the profile's module (rule core.1). A new seam, such as `RequestRefs`, has one production implementation. It is the pull request profile's own trait, inside the profile. So it is no seam of Part B, and rule core.3 asks no record for it.

## Measurements

Measured on deneb, with no other suite on the host, before the first step and after each step. The suite is `moon run cli:test cli:tools conformance:test core:test`. Peak processes are the most `sluice` and `git` processes that `ps` saw at one time, sampled each 200 milliseconds.

| Step | Tests | End-to-end | Wall time | Peak sluice | Peak git |
|---|---|---|---|---|---|
| Before | 2,293 | 953 | | | |

## Risks

- **A moved test proves less.** A test over the fakes cannot find a fault in the real profile. The contract tests of D2 cover each profile against its tool, and `e2e/check.sh` covers the whole path.
- **The fakes drift from the profiles.** The conformance crate runs the same scenarios over the fakes and the profiles. The fakes' own units hold their behaviour.
