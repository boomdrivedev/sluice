## MODIFIED Requirements

### Requirement: A run no host takes for its engine is not passed at its time limit

When a run turned down for its engine, or for a limit its executor
cannot hold, is still unclaimed once the branch's time limit has passed
since the run was opened, the run SHALL end with no outcome and the
combination SHALL be read as not passed, with no submission blamed and
no check written failed (run-life). Its submissions' status SHALL say
no host that could run its tests as its policy asks took them, with
each host's reason (66, 87; rule core.22).

#### Scenario: No host has an engine

- **WHEN** `main`'s time limit is 30 minutes, its policy says
  `docker = true`, and the only host serving it has no engine
- **THEN** 30 minutes after the run opened it ends with no outcome, the
  combination is not passed and no submission is blamed, and the
  submission's status says no host that could run its tests as its
  policy asks took them, naming the host and that no container engine
  answers there

#### Scenario: No host can hold the limit

- **WHEN** `main`'s time limit is 30 minutes, its policy states a
  memory limit, and the only host serving it runs the open executor
- **THEN** 30 minutes after the run opened it ends with no outcome, no
  submission is blamed, and the status names the host and that the open
  executor cannot hold a memory limit
