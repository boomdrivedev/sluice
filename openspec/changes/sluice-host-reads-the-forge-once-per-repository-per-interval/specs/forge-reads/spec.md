## MODIFIED Requirements

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

### Requirement: The last asking is kept on the host
The host's state SHALL keep when sluice last asked for a repository's open pull requests. So a process of sluice that starts again asks nothing that the host shows as asked within the interval (136).

#### Scenario: A restart asks nothing new
- **WHEN** `sluice run` is stopped and started again ten seconds after it asked for the repository's open pull requests
- **THEN** the new process does not ask for them until sixty seconds after the last asking

## ADDED Requirements

### Requirement: One list serves every branch of a repository
Each branch that takes pull requests SHALL read its open pull requests from the repository's one list. A pass SHALL ask for a repository's list one time at most, a move on the host among its causes. A list this process holds that is newer than a branch's last reading SHALL serve the branch with no call (`one-forge-read-per-repository-per-interval@b3b6757cea2d`).

#### Scenario: Three branches, one list
- **WHEN** main and two bolts take pull requests, and `sluice run` passes them twenty times in a minute while nothing on the host moves
- **THEN** the repository's open pull requests were asked for once, and no list of one branch was asked for

#### Scenario: Idle branches leave the allowance for a submit
- **WHEN** main and two bolts take pull requests, the allowance holds seven calls, and forty passes go in two minutes with no move
- **THEN** the passes ask for two lists, and a `sluice submit main --no-wait` opens its pull request and exits 0

### Requirement: A branch's list changed only where its requests moved
A branch SHALL read its list as changed only where GitHub sent the list afresh and the branch's requests differ from its last reading. sluice SHALL compare their numbers, heads, last changes and draft marks (136, rule core.32).

#### Scenario: Another branch's request moves
- **WHEN** a pull request into a bolt gets a new head, and main's requests stand
- **THEN** main makes no query of its requests whole for it
