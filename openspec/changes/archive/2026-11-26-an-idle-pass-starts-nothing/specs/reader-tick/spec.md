## ADDED Requirements

### Requirement: A folder's hooks folder is found once

`sluice host` SHALL find the hooks folder of a folder that a request
names one time, and SHALL keep it while it runs. A request that names
the folder while that finding goes SHALL wait for the finding and SHALL
start no git of its own. No git of a finding SHALL start after the
requests that name its folder are answered (129, 135, rule core.31). A
finding that fails SHALL NOT be kept.

#### Scenario: Two requests name one folder at the same time

- **WHEN** a watch's subscription and `sluice run --rounds 1` name one
  repository at the same time, and git on the path counts every start
- **THEN** git starts for the hooks folder only in one finding, and
  `sluice run --rounds 6` after them starts no git

#### Scenario: A finding that fails is found again

- **WHEN** a request names a folder that is in no repository, and the
  folder becomes a repository
- **THEN** the next request that names it finds its hooks folder

### Requirement: A worker that could not start is said once for each cause

Where the host process cannot bind a worker for a repository, it SHALL
log the cause one time. A command's ask SHALL bind the repository
again. Where that binding is refused for the same cause, the host
process SHALL log nothing more. After a worker binds the repository, a
later refusal SHALL be logged again (87, 129).

#### Scenario: Two asks, one cause

- **WHEN** the host's settings name a system that the host is not, a
  `--no-wait` submit queues work in the repository, and `sluice step
  main` runs after it
- **THEN** the host process's log holds `this host claims no run of it`
  for the repository one time
