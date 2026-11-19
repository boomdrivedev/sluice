## Why

On sluice 0.97.4, in boomdrivedev/sluice-actions-trial, both pool runs
passed `#gate` on GitHub's runners, and neither sign job saved its
record, so nothing landed. In the step `Record the run's end`,
`sluice job record` failed: `remote: Permission to
boomdrivedev/sluice-actions-trial.git denied to github-actions[bot]. ...
The requested URL returned error: 403`, then `origin refused app
5719968's token ... scoped to ... contents: write`, five tries, exit 3.

`actions/checkout` keeps the job's own token, which may only read, in
the repository's git config, as a header for `https://github.com/`. git
sends a stored header with every request, so GitHub never asked for
credentials and git never asked sluice's helper for the App's token.
sluice empties the user's helpers for its own calls and leaves stored
headers in place. Then it read the 403 as a refusal of a token it never
handed over, and bought a fresh one four times.

## What Changes

- **git sends no stored header.** Every fetch and push sluice makes as
  its identity empties the headers git's configuration holds for the
  remote's address, on the call's command line alone.
- **A refusal names what git sent.** sluice's helper marks a file the
  call made once it hands git the token. A refusal on a try git never
  asked for the token is read once as the remote's answer, with no
  token bought afresh. It is named as a credential git sent of its own:
  the user name in the address git reached the remote by, as a rewrite
  in git's configuration may carry one, the header stored for the
  address a rewrite reached, the entry `.netrc` holds for the host, or
  none from the helper. It is never named as the App's token.
- **Recorded.** Record 0019 is amended 2026-11-18 with the choice and the
  paths not taken: `persist-credentials: false` in the workflow, and
  holding the address against rewrites, which would send the App's token
  past a host's mirror or proxy and the tests' own copy. Rule core.29's
  details rule out both failures.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-identity`: git sends no header its configuration stores in
  place of the App's token, and a refusal of git's own credential names
  it.

## Out of scope

- **The workflow `sluice init actions` writes.** It is unchanged. The
  sign job's `sluice job sign`, and `sign-head`'s fetch of a pull
  request's head, read the repository with the job's own token, which a
  private repository needs.
- **The runner's own git calls.** boomdrivedev/boomdrive's runner hands
  the App's token through a helper as sluice does, emptying the host's
  helpers alone, and has the same gap where a host's configuration
  stores a header for the forge. That is the runner's to change.

## Impact

- `crates/sluice/src/profiles/pushed_refs.rs`: `git_as` empties stored
  headers and makes the file the helper marks; `call` tells a refusal
  git never asked for the token from one of the identity's;
  `own_credential` names what git sent.
- `crates/sluice/src/identity.rs`: `ASKED_VAR`, and `helper` marking it.
- Tests: `crates/sluice/tests/pushed_refs.rs`
  (`a_header_gits_configuration_holds_is_never_sent_as_the_identity`,
  `a_credential_git_sends_of_its_own_is_named_and_never_blamed_on_the_identity`);
  `crates/sluice/src/profiles/pushed_refs.rs`
  (`a_credential_git_sends_of_its_own_is_named_by_where_it_is`).
- Records and rules: record 0019 as amended 2026-11-18; rule core.29.
- Sources: clauses 133, 134 and C.4's pushed refs row; records 0019 and
  0108.

## Touches

```
crates/sluice/src/identity.rs
crates/sluice/src/profiles/pushed_refs.rs
crates/sluice/tests/pushed_refs.rs
docs/adr/0019-sluice-pushes-as-a-github-app-whose-token-reaches-only-its-own-git.md
docs/architecture/core.md
openspec/changes/the-sign-job-pushes-its-record-as-the-app/**
```
