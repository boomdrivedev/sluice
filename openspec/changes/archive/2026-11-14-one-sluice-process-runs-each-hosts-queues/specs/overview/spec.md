## MODIFIED Requirements

### Requirement: A branch lists the pull requests another host takes in that this host follows
In `sluice overview`, a branch another host keeps, with pull requests
this host follows whose submissions the branch's queue does not hold at
their heads, SHALL have `taken_in_by` and `pull_requests`, as `sluice
status` gives them, and the same rows in its words. A one-shot overview
and the reader's document SHALL carry them as the host last kept them,
the host process reading each when due at the host's one bound (129,
137; rules core.25, core.31).

#### Scenario: A one-shot overview
- **WHEN** host `b` keeps main and `#13`, which host `a` follows, waits for a review, and `sluice overview --json` runs on `a`
- **THEN** `main` has `taken_in_by` `{"host":"b"}` and a `pull_requests` row for `#13` with its `queue` and a `read_at`

#### Scenario: A watch reads none of them itself
- **WHEN** `sluice overview --json --watch` follows host `a`'s host process for a minute while `a` follows `#13`, with `discovery` at sixty seconds
- **THEN** its documents carry `#13`'s row, the watch read nothing from GitHub, and the host process read `#13` and its check no more than four times each
