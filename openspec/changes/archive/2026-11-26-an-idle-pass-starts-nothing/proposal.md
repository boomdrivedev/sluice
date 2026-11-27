## Why

On 2026-11-26, two tests of an idle repository failed now and then in
sluice's own gate, and passed when they ran again. Each fail failed
another agent's submission and left a key that the gate reads as flaky.

- `sluice_run_with_nothing_queued_starts_git_at_its_first_pass_alone`:
  an idle `sluice run --rounds 6` started one git, `rev-parse
  --path-format=absolute --git-common-dir`. A watch's subscription does
  not wait for its answer. So `sluice host` found the hooks folder for
  the watch at the same time as for `sluice run --rounds 1`, and each
  finding started two gits. Under load, the second git of the watch's
  finding started after the first run ended. The test counted it in the
  six passes (129).
- `the_host_processs_look_at_an_idle_repository_starts_nothing`: the
  host process logged `this host claims no run of it` two times in one
  second. A `--no-wait` submit asks the repository's task two times. Each
  ask removed the words of the last refusal. Where the second refusal
  came after the first, the host process logged it again.

## What Changes

- `sluice host` finds the hooks folder of one folder with one finding at
  a time. A request that names the folder while the finding goes waits
  for it and starts no git.
- The host process keeps the words that it logged for a worker that
  could not start apart from the refusal. A command's ask still binds
  the repository again. A refusal for the same cause logs nothing more
  until a worker binds the repository.

## Capabilities

### New Capabilities

### Modified Capabilities

- `reader-tick`: a folder's hooks folder is found once, and a refused
  worker is logged once for each cause.

## Impact

- `crates/sluice/src/host_process/protection.rs`: `Hooks::found` finds
  a folder's hooks folder one finding at a time.
- `crates/sluice/src/host_process/tasks.rs`: the words logged are kept
  in `said`, which a command's ask does not change.
- Tests: `two_requests_for_one_folder_find_its_hooks_once` and
  `a_finding_that_fails_is_found_again` in `protection.rs`. A step after
  the refusal in `the_host_processs_look_at_an_idle_repository_starts_nothing`.
