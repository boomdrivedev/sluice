## ADDED Requirements

### Requirement: A checkout git left locked mid-way is undone

Where git lists a checkout sluice made as locked with the reason
`initializing`, which git writes while its `worktree add` is going and
which an add that never finished leaves, sluice SHALL unlock it, remove
it and prune git's entry of it before it makes the checkout again or
releases it. A checkout locked for any other reason SHALL be held, named
with git's words (123; record 0122; rule core.16).

#### Scenario: A release after an add that never finished

- **WHEN** `sluice/try/main/6`'s `git worktree add` was stopped part way,
  leaving the checkout locked `initializing`, and its release runs
- **THEN** the checkout is unlocked, removed and pruned, its record goes,
  and git lists it no more
  (`a_checkout_git_left_locked_initializing_is_released` in
  `crates/sluice/tests/profiles/places.rs`)

#### Scenario: Made again after an add that never finished

- **WHEN** a claimed run's checkout is found locked `initializing` as
  its run is started
- **THEN** it is undone and made again at the combination, and the run
  starts in it
  (`a_checkout_git_left_locked_initializing_is_made_again` in
  `crates/sluice/tests/profiles/places.rs`)

#### Scenario: Another lock

- **WHEN** a checkout sluice made is locked with the reason `kept by
  hand`
- **THEN** its release fails and holds it, naming the lock and its
  words, and nothing unlocks it
  (`a_checkout_locked_for_another_reason_is_held` in
  `crates/sluice/tests/profiles/places.rs`)
