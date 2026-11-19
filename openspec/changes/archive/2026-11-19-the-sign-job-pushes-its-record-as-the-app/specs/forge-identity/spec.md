## ADDED Requirements

### Requirement: git sends no header its configuration stores
A fetch or push sluice makes as its app SHALL send the remote no header
git's configuration stores for the remote's address in place of the
app's token, as `actions/checkout` stores its job's token (record 0019
as amended 2026-11-18).

#### Scenario: A job's token the checkout stored
- **WHEN** the repository's config holds `http.https://github.com/.extraheader` with the job's own token, and `sluice job record` pushes the run's record as the app
- **THEN** the push carries the app's token alone, and GitHub records it by the app
  (`a_header_gits_configuration_holds_is_never_sent_as_the_identity` in
  `crates/sluice/tests/pushed_refs.rs`)

### Requirement: A refusal of git's own credential names it
A call the remote refused on a credential git sent of its own, never
asking sluice for the app's token, SHALL be the remote's answer, made
once. It SHALL fail with the remote's words, naming where git found what
it sent, and SHALL NOT name the app's token as refused or buy one afresh
(134; record 0019 as amended 2026-11-18).

#### Scenario: A token a rewritten address carries
- **WHEN** the host's git config rewrites the remote's address to one carrying a user name and token, git sends them, and GitHub answers `Permission to o/r.git denied to github-actions[bot].`
- **THEN** one push was made, and the failure carries GitHub's words, names the address's user name, and names no token of the app's as refused
  (`a_credential_git_sends_of_its_own_is_named_and_never_blamed_on_the_identity`
  in `crates/sluice/tests/pushed_refs.rs`)

#### Scenario: A header stored for the address a rewrite reaches
- **WHEN** the host's git config rewrites the remote's address to another and stores a header with a token for that other address, and GitHub refuses it
- **THEN** one push was made, and the failure names the header git's configuration stores for the address the rewrite reached, never its token
  (`a_credential_git_sends_of_its_own_is_named_and_never_blamed_on_the_identity`
  in `crates/sluice/tests/pushed_refs.rs`)

#### Scenario: A credential the user's .netrc holds
- **WHEN** git sends the credential the user's `.netrc` holds for `github.com`
- **THEN** the failure names that `.netrc` and `github.com`, never the secret
  (`a_credential_git_sends_of_its_own_is_named_by_where_it_is` in
  `crates/sluice/src/profiles/pushed_refs.rs`)
