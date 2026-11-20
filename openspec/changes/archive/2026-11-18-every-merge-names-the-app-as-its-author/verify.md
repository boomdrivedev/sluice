# Verify: every-merge-names-the-app-as-its-author

Verified on `change/every-merge-names-the-app-as-its-author` at
`09960e6ef`, against its base `bolt/sluice-merges-as-the-boomdrive-app`
at `bf21b3209`, on 2026-11-18.

**Verdict: the work holds.** The gate passes, every task is ticked and
true by its own check, and every requirement and scenario of the
`forge-identity` delta is met by the code and a test that passed. The
change is archived with this report.

## The whole checks

`sluice exec bolt/sluice-merges-as-the-boomdrive-app`, run once, exited
0. It ran the bolt's gate, `#check-fast`, as `.config/sluice.toml` names
it, over the checks `09960e6` affects:

- 9 tasks passed: `nix:typecheck`, `root:changes`, `root:lint`,
  `root:format`, `seams:test`, `core:test`, `fakes:test`,
  `conformance:test` and `cli:test`.
- 1443 tests passed, none failed and none was ignored.
- sluice recorded the 9 results under their keys for `09960e6`.
- moon warned that it had no internet connection and skipped its
  dependency install, and that it could not archive `cli:test`'s
  outputs into its cache. Neither touched a result.

`#check-tools` and `#check-e2e` are main's, run when the bolt merges onto
main. The `tools` tests the tasks name as their checks were run once
through `sluice exec bolt/sluice-merges-as-the-boomdrive-app -- cargo
test -p sluice --features tools --test local_refs --test pushed_refs
--test remote_refs --test pull_requests`, filtered to them. All 10
passed: `commit_as_names_its_author` and `commit_writes_the_landing`
(task 2.1); `a_landing_names_whoever_pushes_it` (2.2),
`a_landing_whose_author_cannot_be_read_is_not_written` (2.3),
`a_combination_is_one_commit_whatever_the_identity` (2.4) and
`the_router_writes_a_landing_as_its_branchs_profile` (2.8);
`a_records_commit_names_whoever_pushes_it` (2.5);
`a_landing_and_its_records_name_the_apps_bot` (2.6); and
`a_new_head_with_no_subject_keeps_the_title` and
`a_request_opened_before_titles_changed_is_retitled`, the two tests that
count `GET /app` and now keep the bot first (2.2, design D7).

`openspec validate every-merge-names-the-app-as-its-author --strict`
says the change is valid, and the gate's `root:changes` held it to
archiving.

## Tasks

All 12 tasks are ticked, and each is true by its own check:

| Task | What it asks | Found |
|---|---|---|
| 1.1 | `Author`, `Identity::author`, `App::author` under a key, `authors/<app>.json` | `identity.rs`, `app.rs`, `kept.rs`, `mod.rs`; `the_apps_author_is_its_bot_kept_on_the_host` passed in the gate |
| 1.2 | The author under a credential command | `read_bot` reads `viewer()`; `the_apps_author_under_a_command_is_the_viewers_bot` passed in the gate |
| 1.3 | A login that is no bot, a failed user read, nothing kept | `read_bot` fails before `GET /users/`; nothing is kept on a failure; `an_author_github_did_not_give_is_kept_nowhere` passed in the gate |
| 1.4 | A kept author stands while GitHub fails | `App::author` gives the kept bot on a failed read and writes nothing; `a_kept_author_stands_while_github_cannot_be_read` passed in the gate |
| 2.1 | `LocalRefs::commit_as`; a combination as sluice at time 0 | `local_refs.rs`; `NAME`/`EMAIL` gave way to `Author::sluice()`; passed by `sluice exec` |
| 2.2 | `PushedRefs::author`, its `commit` through `commit_as` | `pushed_refs.rs`, naming no forge; the fakes answer the bot; passed by `sluice exec` |
| 2.3 | A landing whose author cannot be read is not written | `PushedRefs::commit` reads the author before `commit_as`; passed by `sluice exec` |
| 2.4 | One combination, one commit, whatever the identity | `form` writes `Author::sluice()` at 0; passed by `sluice exec` |
| 2.5 | A records commit names whoever pushes it | `RemoteRefs::commit` reads `pushed.author()` before writing anything; passed by `sluice exec` |
| 2.6 | A landing and its records name the app's bot | passed by `sluice exec` |
| 2.7 | The site says who a landing names | `reference/state.md` and `guides/github-app.md` say it, naming no host or tool of ours. The task's `devenv shell -- prose` check is the writer's, by hand (rule core.21; record 0127), and was not run again here |
| 2.8 | `Branch::commit` takes its branch; the router writes through its branch's profile | `sluice-seams`, `step.rs`'s two landings, every implementer, `Branches::commit` by `route`; passed by `sluice exec` |

## Requirements and scenarios

**A commit pushed as the app names the app's bot.**
`PushedRefs::commit` and `RemoteRefs::commit` write the author their
speaker's `Identity::author` gives, and `App`'s is
`<login> <<id>+<login>@users.noreply.github.com>`.

- *A landing and its records pushed as the app*:
  `a_landing_and_its_records_name_the_apps_bot` reads both the remote's
  `main` and `refs/sluice/state/main` as
  `sluice[bot] <900042+sluice[bot]@users.noreply.github.com>`.
- *A records commit pushed as the identity*:
  `a_records_commit_names_whoever_pushes_it`.

**A commit sluice does not push as the app names sluice.**
`PushedRefs::author` gives `Author::sluice()` where there is no speaker,
and `LocalRefs::commit` writes `Author::sluice()`.

- *A landing pushed with the host's credentials*:
  `a_landing_names_whoever_pushes_it`, its second half.
- *A landing in the host's own refs*: `e2e/e2e.sh` S1 still expects
  `sluice <sluice@localhost>` (line 277). It is `#check-e2e`'s, main's
  gate, and was not run here. `commit_writes_the_landing` and the second
  half of `the_router_writes_a_landing_as_its_branchs_profile`, a
  landing on a branch with no remote, passed and cover the same path.

**A combination is one commit on every host.** `LocalRefs::form` writes
`Author::sluice()` at the time 0, and `PushedRefs::form` goes through
it.

- *Two hosts form one combination*:
  `a_combination_is_one_commit_whatever_the_identity`.

**The app's bot is read with its own credential.** `read_bot` reads
`slug()` under a key and `viewer()` under a command, both afresh, and
then `GET /users/<login>` through `get` with the repository's token,
`[` and `]` written `%5B` and `%5D`.

- *The bot under a command*:
  `the_apps_author_under_a_command_is_the_viewers_bot`.

**The app's bot is kept on the host.** `Kept::bot` and `keep_bot` keep
`forge/authors/<app>.json`. `App::author` uses one read less than a day
ago by its clock, and gives a kept one, however old, where a read fails.

- *Read once for the host*:
  `the_apps_author_is_its_bot_kept_on_the_host`.
- *A kept bot stands while GitHub cannot be read*:
  `a_kept_author_stands_while_github_cannot_be_read`.

**A bot that cannot be read writes no commit.** With nothing kept, the
read's failure is `App::author`'s, and so `PushedRefs::commit`'s and
`RemoteRefs::commit`'s, before anything is written. Nothing is kept of
it.

- *GitHub does not give the bot's user* and *A command whose token is a
  person's*: `an_author_github_did_not_give_is_kept_nowhere`.
- *A landing whose author cannot be read*:
  `a_landing_whose_author_cannot_be_read_is_not_written`.

## Rules

- **core.1, core.2.** `pushed_refs.rs` and `remote_refs.rs` ask the
  author through `Identity` and name no forge. The GitHub profile alone
  implements it. The branch seam is still reached through its one
  interface, its `commit` now taking the branch as its other calls do.
- **core.6, core.17, core.58.** A combination's commit is unchanged:
  sluice at the time 0.
- **core.43.** Amended with record 0019's choices, and the code holds
  each one the rule rules out: no landing or records commit pushed as
  the App names sluice, no author comes from git's configuration, no
  commit is written under sluice's name where the bot cannot be read,
  and the bot is kept on the host.
- **core.29, core.32.** The user read goes through `get`, under the
  retry policy and the ETag the host keeps.
