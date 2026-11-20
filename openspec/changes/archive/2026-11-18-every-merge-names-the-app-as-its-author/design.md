## Context

See proposal.md for why. Record 0019 as amended 2026-11-18, point 1,
says what holds; its section "The build's choices within the amendment
of 2026-11-18" records this design's choices with the paths not taken,
and rule core.43 rules on them. The statement's C.4 names no author for
a commit sluice writes, so nothing there changes.

What the code has today:

- `LocalRefs` (`profiles/local_refs.rs`) writes every commit with gix
  as `NAME` and `EMAIL`, `sluice` and `sluice@localhost`, fixed:
  `write_commit` for a landing (`Branch::commit`, the time now) and for
  a combination (`Branch::form`, the time 0, "so one combination forms
  one commit whenever it is formed"). `LocalRefs::git` gives every git
  command it runs the same identity in its environment.
- `PushedRefs` (`profiles/pushed_refs.rs`) forms and commits through
  its `LocalRefs`. It knows sluice's identity on the remote only call
  by call: `speaker()` reads the remote's address as the host's
  configuration holds it now and asks the identity for its URL there,
  `None` for the host's own credentials or a remote on no forge the
  identity is on (129).
- `RemoteRefs::commit` (`profiles/remote_refs.rs`) writes every records
  commit, on `refs/sluice/state/<branch>`, the runs, ids, logs,
  set-asides and heads refs, by `git commit-tree` with the same fixed
  identity in its environment, through the `PushedRefs` it pushes with.
- `Identity` (`identity.rs`) is the interface the pushed refs profile
  reaches sluice's identity by: `name`, `url`, `credentials`, `renew`.
  `App` (`profiles/github/`) implements it.
- `App::bot` reads the app's bot login once in a process, for the pull
  requests sluice opened: under a key `slug()`, `GET /app` with the
  JWT, then `[bot]`; under a credential command `viewer()`, GraphQL's
  `viewer { login }` with the repository's token (record 0108).
- `Kept` (`profiles/github/kept.rs`) keeps the host's answers, pauses
  and readings under `forge/` in the host's state, each file written
  whole and renamed over the last, one that does not parse read as
  absent.

**The family.** No other tool of the family reads a commit's author as
a signal: crew's stub of `wt submit` writes `sluice <sluice@localhost>`
for its own tests, on no App. weir-branch writes its store's commits
under a name its caller gives, and timber its record of roots as
`timber <timber@localhost>`, each on refs of its own that sluice
neither writes nor pushes. Boomdrive's record 0007 holds the one App
whose bot this change names; it says nothing of a commit's author
beyond expecting GitHub to show the product on every landing.

## Goals / Non-Goals

**Goals:**

- A landing and a records commit sluice pushes as a GitHub App name the
  App's bot as author and committer.
- Every other commit sluice writes names `sluice <sluice@localhost>`,
  a combination's on every host.
- The bot is read with the App's own credential and kept on the host,
  so a host asks GitHub for it about once a day.

**Non-Goals:**

- `sluice forge check` reporting the author or the commit statuses
  permission.
- The pull requests' author, `App::bot`, read as before.
- A commit sluice wrote before this change, which stays as written.

## Decisions

### D1. A commit names the identity that pushes it

The author is read where a commit is written, from the identity its
remote is reached as at that moment, exactly as a fetch or a push reads
its identity (129). A landing commit is the branch's `PushedRefs`'s; a
records commit is the records' `PushedRefs`'s. So a remote pointed at
GitHub since the profile opened names the App at its next commit, and
one pointed away names sluice.

Path not taken: an author fixed when the profile opens, which would
name an App the remote no longer reaches.

### D2. The identity names its author, and the profiles ask it

`identity.rs` gains `Author`, a name and an address, with
`Author::sluice()` giving `sluice <sluice@localhost>`, and
`Identity::author(&self, url) -> Result<Author>`, the author a commit
pushed as the identity to `url` names, `url` one `Identity::url` gave.
`App` implements it by reading its bot for the repository `url` names
(D4). The tests' fake identity names an author of its own.

`PushedRefs::author()` gives `Identity::author` of its speaker, or
`Author::sluice()` where it has none. `LocalRefs` gains
`commit_as(&LandingCommit, &Author)`, `write_commit` taking the author;
its `Branch::commit` writes `Author::sluice()`. `PushedRefs`'s
`Branch::commit` writes `commit_as` with its `author()`.
`RemoteRefs::commit` gives `git commit-tree` its `PushedRefs`'s
`author()` in place of the fixed name. `LocalRefs::git` keeps sluice's
name in its environment: no command it runs writes a commit.

The branch seam's `Branch::commit` takes the branch it writes for,
`commit(&self, branch, &LandingCommit)`, as `read`, `move_to` and
`make` take theirs, and the core's two landings in `step.rs`, the one
beside the chain and the chain's, give it the branch they land on. The
`Branches` router writes a landing through the profile that branch is
routed to, as it moves the branch: a landing on a branch on a remote
reaches that remote's `PushedRefs`, and one in the host's own refs its
`LocalRefs`. The branch a commit goes to is where it is written, not
part of the commit, so `LandingCommit` stays what is written, and who
commits stays the profile's to know, not the core's. The seam is still
reached through its one interface, and nothing else of it changes. The
pushed refs profile names no forge and asks the identity's own
interface (rules core.1, core.2).

Paths not taken: an author on `LandingCommit`, decided by the core,
which knows no identity; the branch on `LandingCommit`, which would
carry where a commit goes beside what it is, and name a branch one way
there and another in the seam's other methods.

### D3. A combination's commit stays sluice's

`Branch::form` keeps writing the combination as `Author::sluice()` at
the time 0, whatever the profile's identity. One combination then
forms one commit on every host, a host with the App and one without
alike, as rules core.6, core.17 and core.58 need: a combination is
named by its revision, and a run's place is made at it on whatever host
claims the run. GitHub shows no combination on a branch: the landing
commit carries the combination's tree on parents of its own, and is
the commit GitHub shows.

Path not taken: the App's bot on combinations too, as "every commit"
reads by its letter.

### D4. The bot is read with the App's own credential

`App::author(&self, repo) -> Result<Author>` reads, where nothing kept
stands (D5):

1. The login. Under a key, `slug()`, `GET /app` with the JWT, then
   `[bot]`. Under a credential command, `viewer()`, GraphQL with the
   repository's token. Both are read afresh, not from `App::bot`'s
   memory, so a renamed App is read by its new name.
2. A login that does not end in `[bot]` is no App's bot: the read fails
   naming it, as `app_of_token` fails.
3. The user id, `GET /users/<login>` with the repository's token, `[`
   and `]` written `%5B` and `%5D`, through `get`, so it runs under the
   retry policy as a read, counts against the installation's
   allowance, waits on no pause but fails while one stands, and carries
   the ETag the host keeps for its address (rules core.29, core.32).
   The answer's `login` and `id` are taken.

The author is the login, at `<id>+<login>@users.noreply.github.com`,
the address GitHub links to the bot's account.

Paths not taken: GraphQL's `viewer { databaseId }` under a command, one
read fewer there but two ways to one value; the App's own id in the
address, which GitHub links to no account; `<login>@users.noreply.github.com`,
GitHub's older form with no id.

### D5. Kept on the host for a day

`Kept` gains `authors/<app id>.json` under `forge/`: the login, the id
and when they were read, written whole and renamed over the last as
its other files are, a file that does not parse read as absent, and no
token in it. `App::author` uses a kept author read less than a day ago
by the App's own clock; otherwise it reads (D4) and keeps what it read.
Where that read fails and an author is kept, older than a day, the kept
one is used, nothing is written over it, and the next commit reads
again: a spent allowance or GitHub's outage then holds no landing a
push would make, and the bot used is the one GitHub last gave. An App
that keeps nothing on the host, a test's, keeps the author in its own
memory under the same rules. `kept.rs`'s module doc lists the file; the
sweep of unused answers leaves it.

Paths not taken: the author read once in each process, as `App::bot`
is, which asks GitHub at every process's first commit; kept for good,
which names a renamed App by its old name until someone removes the
file; a kept author dropped once a day has passed, which holds every
landing while the read fails; the answers kept by address, which are
swept after a day unused and asked again at each use.

### D6. A read that fails writes nothing

Where the host keeps no author for the App and its read fails,
`Identity::author`'s failure is the commit's: `commit_as` is never
reached, so no object is written and the branch is not moved. A
landing's step fails as a fetch it could not make fails, landing
nothing and saying why in GitHub's words; a records write fails as a
refused push does. Nothing is kept of a read that failed, and the next
commit asks again. No commit is written under sluice's name in its
place.

Path not taken: sluice's name where the bot cannot be read, which is
what record 0019's amendment rules out.

### D7. The tests

- `tests/profiles/forge.rs`'s fake answers
  `GET /users/sluice-test%5Bbot%5D` with
  `{"login": "sluice-test[bot]", "id": 900042, "type": "Bot"}`, and
  can be told to answer it `404 Not Found` or `500`, and to name
  `octocat` as GraphQL's `viewer`.
- `tests/profiles/forge/github.rs`'s fake answers
  `GET /users/sluice%5Bbot%5D` with `{"login": "sluice[bot]", "id":
  900042, "type": "Bot"}`, a constant `BOT` beside `APP`, since every
  landing and records write of the pull request tests now reads it.
- `tests/pushed_refs.rs`'s `FakeIdentity` names
  `a fake <fake@identity.invalid>`, or fails with words of its own
  when told to. `tests/remote_refs.rs` opens a host with such an
  identity, of its own or moved to `tests/support`.
- Every test reads the author from the commit's own `author` and
  `committer` lines, `git cat-file -p`, as `commit_writes_the_landing`
  does.
- A test that counts the `GET /app` reads of the pull requests' author,
  `logins` in `tests/profiles/pull_requests.rs`, keeps an author for
  the app in its state before its first command, so no commit of its
  reads one and the count stays the pull requests' alone.

## Risks / Trade-offs

- **A records write needs the bot once a day.** The first records
  commit of a host's day asks GitHub one or two more calls. With
  GitHub down the push fails too, so nothing is lost that a push would
  have kept.
- **A spent allowance on a host that keeps no author holds a
  landing.** The read waits on no pause and fails while one stands, so
  a host's first landing for an App waits for the pause to end. Once an
  author is kept, a failed read a day later uses it (D5).
- **A renamed App is named by its old name for up to a day.** GitHub
  links the address by the bot's id, which a rename keeps.
- **Tests that count calls to the fake.** A test that counts the calls
  of a host's first commit now sees the author's reads. Where such a
  test counts what another read makes, it keeps an author in its state
  first (D7), as a host that has run a day would hold one.
