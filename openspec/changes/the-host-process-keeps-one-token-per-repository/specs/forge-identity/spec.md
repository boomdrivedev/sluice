## ADDED Requirements

### Requirement: The host process holds one token for each repository
sluice's host process SHALL hold the app's token for a repository in one
store for its life, which every binding it makes, on any thread, SHALL
hold its tokens in, so no binding of its gets a repository's token while
one it holds lasts. Two bindings that need a repository's token at once
SHALL get it once. A token held SHALL be taken by no app of another id
(134; record 0108 as amended 2026-11-19).

#### Scenario: Bindings on several threads share one token
- **WHEN** apps on eight threads holding one store ask at once for `o/r`'s token, which the credential command gives with an end an hour away
- **THEN** the command ran once, GitHub was asked once what the token reaches, every app took `token-1`, and an app of another id holding the store got its own
  (`apps_holding_one_store_get_a_repositorys_token_once` in
  `crates/sluice/tests/profiles/forge.rs`)

#### Scenario: The host process's looks buy one token
- **WHEN** a host process started afresh is held open with nothing in play, and looks at the idle repository four times, each look fetching main's records from GitHub as the app
- **THEN** one token for `o/r` was bought
  (`the_host_process_buys_a_repositorys_token_once_across_its_looks` in
  `crates/sluice/tests/profiles/pull_requests.rs`)

### Requirement: A token got afresh by one binding is the one another refused takes
Where GitHub refused a token and the token held is another, got since by
another call, sluice SHALL take the token held in place of the refused
one, and SHALL neither run the credential command with `erase` nor ask it
for another (134; rule core.29; record 0108 as amended 2026-11-19).

#### Scenario: Two bindings refused one token
- **WHEN** two apps hold one store and `token-1`, GitHub refuses `token-1` to the first, which gets `token-2`, and the second is then renewed in place of `token-1`
- **THEN** the second takes `token-2`, and the command ran with `get`, `erase` for `token-1` and `get`, nothing more
  (`a_token_got_afresh_is_the_one_another_refused_takes` in
  `crates/sluice/tests/profiles/forge.rs`)
