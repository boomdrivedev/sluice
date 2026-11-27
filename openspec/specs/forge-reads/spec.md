# forge-reads Specification

## Purpose
What sluice asks GitHub and when: every read conditional on the answer
the host keeps, the open pull requests asked for at the host's interval
or after a move on the host, a pull request read again only once it
moved, and sluice's postings made only when what they say changes (136;
rule core.32).

## Requirements

### Requirement: Every read of GitHub is conditional
Every `GET` sluice makes to GitHub's API SHALL carry the ETag and the
last-modified time of the last answer at the same address, kept in the
host's state for every process of sluice on the host. A `304` SHALL be
answered from the body kept with them; a `200` SHALL replace what is
kept. A kept answer SHALL NOT be used without GitHub's `304` for it, and
nothing kept SHALL hold a token (136; rule core.32).

#### Scenario: An unchanged list
- **WHEN** sluice lists main's open pull requests twice and GitHub answers the second `304 Not Modified`
- **THEN** the second read sent `If-None-Match` with the first answer's ETag, and its answer is the first's pull requests

#### Scenario: Another process on the host
- **WHEN** `sluice run` has read a pull request, and a `sluice wait` on the same host reads the same address
- **THEN** the wait's read carries the ETag `sluice run` kept

#### Scenario: GitHub answers 200 to a kept ETag
- **WHEN** GitHub answers a read carrying a kept ETag with `200` and a new ETag
- **THEN** the new answer is used and kept in place of the old

#### Scenario: No token on disk
- **WHEN** sluice has read and posted on GitHub as its app
- **THEN** no file under the host's state holds the token

### Requirement: The open pull requests are asked for at the host's interval
sluice SHALL ask for a repository's open pull requests in one list, for every branch of the repository. It SHALL ask at the interval the host's `[forge] discovery` sets, sixty seconds where it sets none. A tick with nothing due SHALL make no call to GitHub. sluice SHALL refuse a `discovery` below ten seconds (136, rule core.31, `one-forge-read-per-repository-per-interval@b3b6757cea2d`).

#### Scenario: Nothing moved
- **WHEN** `sluice run` passes main twenty times in a minute and nothing on the host moved
- **THEN** the repository's open pull requests were asked for once in that minute

#### Scenario: The host sets the interval
- **WHEN** the host's config holds `[forge] discovery = "2m"`
- **THEN** the repository's open pull requests are asked for at most once in two minutes while nothing on the host moves

#### Scenario: Too short an interval
- **WHEN** the host's config holds `[forge] discovery = "5s"`
- **THEN** sluice refuses the config, naming `discovery` and ten seconds

### Requirement: A move on the host asks at once
A branch's open pull requests SHALL be asked for at once after a submit
on the host opened, updated or marked ready a pull request into the
branch, after the branch moved, or after its records moved (136).

#### Scenario: A submit on the host asks at once
- **WHEN** `sluice submit main` opens a pull request ten seconds after main's last asking
- **THEN** the host's next pass asks for main's open pull requests

#### Scenario: A landing asks at once
- **WHEN** a round lands a submission on main
- **THEN** the next pass asks for main's open pull requests

### Requirement: The last asking is kept on the host
The host's state SHALL keep when sluice last asked for a repository's open pull requests. So a process of sluice that starts again asks nothing that the host shows as asked within the interval (136).

#### Scenario: A restart asks nothing new
- **WHEN** `sluice run` is stopped and started again ten seconds after it asked for the repository's open pull requests
- **THEN** the new process does not ask for them until sixty seconds after the last asking

### Requirement: A pull request is read again only once it moved
A pull request's reviews, other systems' checks, ready marks and
comments SHALL be read again only once its head, its last change on
GitHub (`updated_at`), its draft mark or its submission's state moved
since they were last read, or, for a pull request out of draft still
waiting on its conditions, at each asking of the open pull requests. A
draft SHALL be read by the list alone. What was read SHALL be kept in
the host's state (128, 136).

#### Scenario: A waiting pull request that stands
- **WHEN** a pull request out of draft meets every condition, its submission is queued, and main's list is asked for three times with nothing moved
- **THEN** its reviews, checks, timeline and comments were not read again

#### Scenario: A pull request waiting on a check
- **WHEN** a pull request out of draft waits for `qa/approved`, which another system then passes on its head
- **THEN** its checks are read again at the next asking, and the pass submits it

#### Scenario: A draft
- **WHEN** a draft pull request is open into main
- **THEN** each asking reads it from the list alone, and no review, check, timeline or comment of it is read

#### Scenario: A new head
- **WHEN** a pull request's head moves
- **THEN** its reviews and checks are read again at the next asking

### Requirement: sluice posts on a pull request only what changed
sluice's check runs and its report SHALL be posted only when what they
say differs from what sluice last posted or read of them, kept in the
host's state, and GitHub's copies SHALL be read back only when the host
holds none for that head. The report's comment SHALL be edited by the id
the host keeps, its pull request's comments read only when the host
keeps none (136; rule core.32).

#### Scenario: Nothing new to say
- **WHEN** a queued pull request's state is the same for ten passes
- **THEN** its check run was neither read nor posted after the first

#### Scenario: The state moves
- **WHEN** the pull request's submission goes from queued to testing
- **THEN** its check run is posted once, saying testing, with no read of GitHub's copy first

### Requirement: One query reads a branch's open pull requests whole
When the list of a branch's open pull requests came back changed, while
a pull request out of draft waits on its conditions, or when a pull
request has nothing kept, sluice SHALL ask GitHub's GraphQL API one query
for every open pull request into the branch, with its latest deciding
reviews, its head's checks and statuses, its ready marks and sluice's own
check runs, in place of a read of each per pull request (136; rule
core.32).

#### Scenario: Three pull requests, one query
- **WHEN** main has three open pull requests and its list comes back changed
- **THEN** one GraphQL query is made, and no REST read of any one's reviews, checks, timeline or check runs

#### Scenario: Reviews counted as before
- **WHEN** the query's reviews hold one reviewer's approval on an older head and another's on the head
- **THEN** the pull request counts one approval toward `reviews`

### Requirement: A refused query falls back to the reads per pull request
A query GitHub refuses other than for its allowance SHALL be followed,
for that pass, by the REST reads per pull request, said once (136).

#### Scenario: The query refused
- **WHEN** GitHub answers the query with an error that is not `RATE_LIMITED`
- **THEN** that pass reads each pull request by REST, and says so once

### Requirement: One list serves every branch of a repository
Each branch that takes pull requests SHALL read its open pull requests from the repository's one list. A pass SHALL ask for a repository's list one time at most, a move on the host among its causes. A list this process holds that is newer than a branch's last reading SHALL serve the branch with no call (`one-forge-read-per-repository-per-interval@b3b6757cea2d`).

#### Scenario: Three branches, one list
- **WHEN** main and two bolts take pull requests, and `sluice run` passes them twenty times in a minute while nothing on the host moves
- **THEN** the repository's open pull requests were asked for once, and no list of one branch was asked for

#### Scenario: Idle branches leave the allowance for a submit
- **WHEN** main and two bolts take pull requests, the allowance holds 18 calls, and 160 passes go in eight minutes with no move
- **THEN** the passes ask for eight lists, and a `sluice submit main --no-wait` opens its pull request and exits 0

### Requirement: A branch's list changed only where its requests moved
A branch SHALL read its list as changed only where GitHub sent the list afresh and the branch's requests differ from its last reading. sluice SHALL compare their numbers, heads, last changes and draft marks (136, rule core.32).

#### Scenario: Another branch's request moves
- **WHEN** a pull request into a bolt gets a new head, and main's requests stand
- **THEN** main makes no query of its requests whole for it
