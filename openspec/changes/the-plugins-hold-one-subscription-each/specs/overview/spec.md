## ADDED Requirements

### Requirement: A branch names the folder of its recent live submissions

In `sluice overview --json`, each branch SHALL have `recent_paths`. It SHALL map each submission that the `recent` lines name, and that the live records hold, to its folder, as its record's `from.path` gives it. A submission with no `from.path`, or in the finished records alone, SHALL have no key. `recent` and `recent_from` SHALL stay as they are. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A blame and a conflict

- **WHEN** `#2` from the folder `/w/unit-b` was blamed, and `#3` from the folder `/w/unit-c` conflicted
- **THEN** the branch's `recent_paths` is `{"2": "/w/unit-b", "3": "/w/unit-c"}`

#### Scenario: A landed submission moved to the finished records

- **WHEN** `#1` landed and moved to the finished records, and a recent line names it
- **THEN** `recent_paths` has no key `1`, and `recent_from` still names the worktree of `#1`

#### Scenario: A pull request's submission

- **WHEN** `#5` was taken in from a pull request, so its record has no `from.path`, and `#5` was blamed
- **THEN** `recent_paths` has no key `5`

### Requirement: A branch names the failed tests of its recent blames

In `sluice overview --json`, each branch SHALL have `recent_failed`. It SHALL map each submission that a `recent` line with `"event": "blamed"` names, and that the live records hold, to the names of its failed tests. The names SHALL be the record's `failed[].task`, in the record's order. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A blame with two failed tests

- **WHEN** `#2` was blamed, and its record's `failed` names `cli:test` and then `core:test`
- **THEN** the branch's `recent_failed` is `{"2": ["cli:test", "core:test"]}`

#### Scenario: No blame

- **WHEN** the branch's recent lines name only a landing and a conflict
- **THEN** its `recent_failed` is `{}`
