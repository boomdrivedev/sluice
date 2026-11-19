# Spec Delta

## ADDED Requirements

### Requirement: A commit pushed as the app names the app's bot
Each landing commit sluice writes for a branch it pushes to GitHub as
its app, and each records commit it writes on a remote it pushes to as
its app, SHALL have the app's bot as its author and committer: the
bot's login, the app's slug followed by `[bot]`, at
`<bot user id>+<login>@users.noreply.github.com` (record 0019 as
amended 2026-11-18).

#### Scenario: A landing and its records pushed as the app
- **WHEN** a request lands on `main` on GitHub, the host's `[forge]` naming the app whose bot GitHub names `sluice[bot]` with user id 900042
- **THEN** the landing commit on `main` and the commit on `refs/sluice/state/main` each have `sluice[bot] <900042+sluice[bot]@users.noreply.github.com>` as their author and committer
  (`a_landing_and_its_records_name_the_apps_bot` in
  `crates/sluice/tests/profiles/pull_requests.rs`)

#### Scenario: A records commit pushed as the identity
- **WHEN** a host that reaches the records' remote as an identity writes a branch's state
- **THEN** the commit on `refs/sluice/state/<branch>` names the identity's author as its author and committer, and a host that reaches the remote with its own credentials names `sluice <sluice@localhost>`
  (`a_records_commit_names_whoever_pushes_it` in
  `crates/sluice/tests/remote_refs.rs`)

### Requirement: A commit sluice does not push as the app names sluice
A commit sluice writes on a branch in the host's own refs, on a branch
or a records remote it pushes to with the host's own credentials, or on
a remote on no forge the app is on, SHALL name
`sluice <sluice@localhost>` as its author and committer (record 0019 as
amended 2026-11-18).

#### Scenario: A landing pushed with the host's credentials
- **WHEN** a submission lands on a branch whose remote sluice pushes to with the host's own credentials
- **THEN** the landing commit's author and committer are `sluice <sluice@localhost>`
  (`a_landing_names_whoever_pushes_it` in
  `crates/sluice/tests/pushed_refs.rs`)

#### Scenario: A landing in the host's own refs
- **WHEN** a submission lands on a branch in the host's own refs
- **THEN** the landing commit's author and committer are `sluice <sluice@localhost>`
  (S1 in `e2e/e2e.sh`)

### Requirement: A combination is one commit on every host
A combination's commit SHALL name `sluice <sluice@localhost>` at the
time 0 whatever identity the host reaches the branch's remote as, so
one combination forms one commit on every host (core.6, core.17,
core.58).

#### Scenario: Two hosts form one combination
- **WHEN** a host that pushes as an identity and a host with none each form the combination of one head on one revision
- **THEN** both form the same revision, whose author and committer are `sluice <sluice@localhost>`
  (`a_combination_is_one_commit_whatever_the_identity` in
  `crates/sluice/tests/pushed_refs.rs`)

### Requirement: The app's bot is read with its own credential
The app's bot SHALL be read from GitHub with the app's own credential.
Under a key, its login SHALL be the slug `GET /app` gives the app's
JWT, followed by `[bot]`. Under a credential command, its login SHALL
be GraphQL's `viewer` read with the repository's token. Its user id
SHALL be `GET /users/<login>` read with the repository's token (record
0019 as amended 2026-11-18).

#### Scenario: The bot under a command
- **WHEN** the host's `[forge]` names a credential command whose token GraphQL's `viewer` names `sluice-test[bot]`
- **THEN** the author is `sluice-test[bot] <900042+sluice-test[bot]@users.noreply.github.com>`, `GET /users/sluice-test%5Bbot%5D` was read with the command's token, and `GET /app` was never called
  (`the_apps_author_under_a_command_is_the_viewers_bot` in
  `crates/sluice/tests/profiles/forge.rs`)

### Requirement: The app's bot is kept on the host
The bot's login and user id SHALL be kept in the host's state for the
app, read there by every process of sluice on the host, and read from
GitHub again once a day has passed since they were read. Where that
read fails, the kept login and id SHALL stand until a read succeeds
(record 0019 as amended 2026-11-18).

#### Scenario: Read once for the host
- **WHEN** two apps on one host's state each ask for the author of app 4242, whose slug GitHub gives as `sluice-test` and whose bot's user id as 900042, and a third asks a day and a minute after the first
- **THEN** the first two answer `sluice-test[bot] <900042+sluice-test[bot]@users.noreply.github.com>` with one `GET /app` and one `GET /users/sluice-test%5Bbot%5D` between them, and the third reads both from GitHub again
  (`the_apps_author_is_its_bot_kept_on_the_host` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A kept bot stands while GitHub cannot be read
- **WHEN** a day and a minute after the author of app 4242 was kept, GitHub answers `GET /users/sluice-test%5Bbot%5D` with `500` until the retry policy's time is spent, and then answers it again
- **THEN** the author kept is given while GitHub fails, nothing kept changes, and the next asking after GitHub answers reads it and keeps it with its new time
  (`a_kept_author_stands_while_github_cannot_be_read` in
  `crates/sluice/tests/profiles/forge.rs`)

### Requirement: A bot that cannot be read writes no commit
Where the host keeps no bot for the app and it cannot be read, the
commit that needed it SHALL NOT be written, under the bot's name or
sluice's, and the failure SHALL carry GitHub's words. Nothing SHALL be
kept of a read that failed, and the next commit SHALL ask GitHub again.
A login that does not end in `[bot]` SHALL be no app's bot, and the
failure SHALL name it (record 0019 as amended 2026-11-18).

#### Scenario: GitHub does not give the bot's user
- **WHEN** GitHub answers `GET /users/sluice-test%5Bbot%5D` with `404 Not Found`
- **THEN** asking for the author fails, carrying GitHub's words, the host's state keeps no author for the app, and the next asking reads GitHub again
  (`an_author_github_did_not_give_is_kept_nowhere` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A command whose token is a person's
- **WHEN** the host's credential command gives a token GraphQL's `viewer` names `octocat`
- **THEN** asking for the author fails, naming `octocat` as no app's bot, and `GET /users/` is not called
  (`an_author_github_did_not_give_is_kept_nowhere` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: A landing whose author cannot be read
- **WHEN** a landing on a branch pushed as an identity is written while the identity cannot give its author
- **THEN** the landing commit is not written, the branch stays where it was, and the failure carries the identity's words
  (`a_landing_whose_author_cannot_be_read_is_not_written` in
  `crates/sluice/tests/pushed_refs.rs`)
