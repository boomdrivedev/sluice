## ADDED Requirements

### Requirement: Each call to GitHub goes through octocrab over tower
Each call sluice makes to GitHub's API SHALL go through octocrab, over a tower stack of hyper's client. The stack SHALL hold one limit on calls in flight for the process and a timeout of a try's limit. It SHALL also hold a layer that reads GitHub's budget from each answer's headers. Each try SHALL run under sluice's one retry policy (rule core.29, `sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97`).

#### Scenario: The budget on an answer
- **WHEN** GitHub answers with `x-ratelimit-remaining: 0`, `x-ratelimit-reset`, `x-ratelimit-resource: graphql` and `retry-after: 90`
- **THEN** the budget layer gives the answer those four values

### Requirement: Four calls to GitHub in flight at one time
A process of sluice SHALL have at most four calls to GitHub in flight at one time, whichever binding, app or thread sends them. A call past the limit SHALL wait for a place (`one-github-client-per-host@786dccccffe1`).

#### Scenario: Six calls at one time
- **WHEN** six threads of one process each call GitHub at one time, and GitHub answers no call until four calls wait on it
- **THEN** four calls reach GitHub, two calls wait for a place, and the two calls go after GitHub answers the four

### Requirement: sluice follows no redirect from GitHub
A redirect that GitHub answers SHALL be the call's answer, and SHALL NOT be followed (rule core.29).

#### Scenario: A renamed repository
- **WHEN** GitHub answers a call `301` with a new address
- **THEN** sluice makes no call to the new address, and says to point the remote at it
